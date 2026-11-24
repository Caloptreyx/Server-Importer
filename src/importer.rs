//! Background SFTP → Wings import runner and live progress registry.
use crate::{
    db::{self, JobOutcome, NewJob},
    settings::Settings,
    sftp::{ConnectParams, SftpAuth, SftpSession, join_posix, normalize_destination_path},
};
use anyhow::{anyhow, bail};
use chrono::{DateTime, Utc};
use serde::Serialize;
use shared::{State, models::server::Server};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use wings_api::client::{AsyncRequestReader, WingsClient};

const DUPLEX_BUFFER_BYTES: usize = 256 * 1024;
const MAX_RECENT_ERRORS: usize = 20;

static ACTIVE: LazyLock<Mutex<HashMap<Uuid, Arc<ActiveImport>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Connecting,
    Scanning,
    Transferring,
}

#[derive(Clone, Debug, Serialize)]
pub struct ActiveImportSnapshot {
    pub id: Uuid,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: String,
    pub host_fingerprint: Option<String>,
    pub source_path: String,
    pub destination_path: String,
    pub phase: Phase,
    pub files_total: u64,
    pub files_transferred: u64,
    pub files_skipped: u64,
    pub files_failed: u64,
    pub bytes_total: u64,
    pub bytes_transferred: u64,
    pub current_file: Option<String>,
    pub bytes_per_second: u64,
    pub eta_seconds: Option<u64>,
    pub started_at: DateTime<Utc>,
    pub recent_errors: Vec<String>,
}

struct ProgressMutable {
    phase: Phase,
    host_fingerprint: Option<String>,
    source_path: String,
    current_file: Option<String>,
    transfer_started: Option<Instant>,
    recent_errors: Vec<String>,
}

struct ActiveImport {
    id: Uuid,
    host: String,
    port: u16,
    username: String,
    auth_method: String,
    destination_path: String,
    started_at: DateTime<Utc>,
    cancel: CancellationToken,
    files_total: AtomicU64,
    files_transferred: AtomicU64,
    files_skipped: AtomicU64,
    files_failed: AtomicU64,
    bytes_total: AtomicU64,
    bytes_transferred: AtomicU64,
    inner: Mutex<ProgressMutable>,
}

impl ActiveImport {
    fn snapshot(&self) -> ActiveImportSnapshot {
        let bytes_transferred = self.bytes_transferred.load(Ordering::Relaxed);
        let bytes_total = self.bytes_total.load(Ordering::Relaxed);
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());

        let (bytes_per_second, eta_seconds) = match guard.transfer_started {
            Some(started) => {
                let elapsed = started.elapsed().as_secs_f64();
                if elapsed >= 0.25 && bytes_transferred > 0 {
                    let speed = (bytes_transferred as f64 / elapsed) as u64;
                    let remaining = bytes_total.saturating_sub(bytes_transferred);
                    let eta = (speed > 0).then_some(remaining / speed);
                    (speed, eta)
                } else {
                    (0, None)
                }
            }
            None => (0, None),
        };

        ActiveImportSnapshot {
            id: self.id,
            host: self.host.clone(),
            port: self.port,
            username: self.username.clone(),
            auth_method: self.auth_method.clone(),
            host_fingerprint: guard.host_fingerprint.clone(),
            source_path: guard.source_path.clone(),
            destination_path: self.destination_path.clone(),
            phase: guard.phase,
            files_total: self.files_total.load(Ordering::Relaxed),
            files_transferred: self.files_transferred.load(Ordering::Relaxed),
            files_skipped: self.files_skipped.load(Ordering::Relaxed),
            files_failed: self.files_failed.load(Ordering::Relaxed),
            bytes_total,
            bytes_transferred,
            current_file: guard.current_file.clone(),
            bytes_per_second,
            eta_seconds,
            started_at: self.started_at,
            recent_errors: guard.recent_errors.clone(),
        }
    }

    fn record_file_error(&self, message: String) {
        self.files_failed.fetch_add(1, Ordering::Relaxed);
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if guard.recent_errors.len() < MAX_RECENT_ERRORS {
            guard.recent_errors.push(message);
        }
    }
}

struct ActiveGuard {
    server_uuid: Uuid,
    job_id: Uuid,
}

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        if let Ok(mut map) = ACTIVE.lock()
            && map.get(&self.server_uuid).is_some_and(|job| job.id == self.job_id)
        {
            map.remove(&self.server_uuid);
        }
    }
}

pub struct StartRequest {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: SftpAuth,
    pub expected_fingerprint: Option<String>,
    pub source_path: String,
    pub destination_path: String,
}

pub enum StartError {
    AlreadyRunning,
    ConcurrencyLimit(u32),
    Invalid(String),
    Internal(anyhow::Error),
}

pub fn active_snapshot(server_uuid: Uuid) -> Option<ActiveImportSnapshot> {
    ACTIVE
        .lock()
        .ok()
        .and_then(|map| map.get(&server_uuid).cloned())
        .map(|job| job.snapshot())
}

pub fn cancel_import(server_uuid: Uuid) -> Option<Uuid> {
    let job = ACTIVE
        .lock()
        .ok()
        .and_then(|map| map.get(&server_uuid).cloned())?;
    job.cancel.cancel();
    Some(job.id)
}

pub async fn start_import(
    state: State,
    mut server: Server,
    user_uuid: Uuid,
    user_username: String,
    settings: Settings,
    request: StartRequest,
) -> Result<ActiveImportSnapshot, StartError> {
    let dest_path = normalize_destination_path(&request.destination_path)
        .map_err(|err| StartError::Invalid(err.to_string()))?;

    if server.is_ignored_subtree(&dest_path) {
        return Err(StartError::Invalid(
            "Destination path is restricted by your server permissions".to_string(),
        ));
    }

    let job_id = Uuid::new_v4();
    let auth_method = request.auth.method_name().to_string();
    let active = Arc::new(ActiveImport {
        id: job_id,
        host: request.host.clone(),
        port: request.port,
        username: request.username.clone(),
        auth_method: auth_method.clone(),
        destination_path: dest_path.clone(),
        started_at: Utc::now(),
        cancel: CancellationToken::new(),
        files_total: AtomicU64::new(0),
        files_transferred: AtomicU64::new(0),
        files_skipped: AtomicU64::new(0),
        files_failed: AtomicU64::new(0),
        bytes_total: AtomicU64::new(0),
        bytes_transferred: AtomicU64::new(0),
        inner: Mutex::new(ProgressMutable {
            phase: Phase::Connecting,
            host_fingerprint: request.expected_fingerprint.clone(),
            source_path: request.source_path.clone(),
            current_file: None,
            transfer_started: None,
            recent_errors: Vec::new(),
        }),
    });

    {
        let mut map = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        if map.contains_key(&server.uuid) {
            return Err(StartError::AlreadyRunning);
        }
        if map.len() >= settings.max_concurrent_imports as usize {
            return Err(StartError::ConcurrencyLimit(settings.max_concurrent_imports));
        }
        map.insert(server.uuid, Arc::clone(&active));
    }

    if let Err(err) = db::insert_job(
        &state.database,
        &NewJob {
            id: job_id,
            server_uuid: server.uuid,
            user_uuid: Some(user_uuid),
            user_username: Some(&user_username),
            host: &request.host,
            port: request.port,
            username: &request.username,
            auth_method: &auth_method,
            host_fingerprint: request.expected_fingerprint.as_deref(),
            source_path: &request.source_path,
            destination_path: &dest_path,
        },
    )
    .await
    {
        if let Ok(mut map) = ACTIVE.lock() {
            map.remove(&server.uuid);
        }
        return Err(StartError::Internal(err.into()));
    }

    let initial_snapshot = active.snapshot();

    tokio::spawn(async move {
        let _guard = ActiveGuard {
            server_uuid: server.uuid,
            job_id,
        };
        run_import_job(state, server, user_uuid, settings, request, dest_path, active).await;
    });

    Ok(initial_snapshot)
}

async fn run_import_job(
    state: State,
    mut server: Server,
    user_uuid: Uuid,
    settings: Settings,
    request: StartRequest,
    dest_path: String,
    active: Arc<ActiveImport>,
) {
    let result = execute_import(
        &state,
        &mut server,
        user_uuid,
        &settings,
        request,
        &dest_path,
        &active,
    )
    .await;

    let snap = active.snapshot();
    let (status, error) = match result {
        Ok(()) if active.cancel.is_cancelled() => ("cancelled", Some("Cancelled by user".to_string())),
        Ok(()) => ("completed", None),
        Err(_) if active.cancel.is_cancelled() => ("cancelled", Some("Cancelled by user".to_string())),
        Err(err) => {
            tracing::warn!(
                server = %server.uuid,
                job = %active.id,
                error = %err,
                "Server Importer: import failed"
            );
            ("failed", Some(err.to_string()))
        }
    };

    if let Err(err) = db::finish_job(
        &state.database,
        active.id,
        &JobOutcome {
            status,
            host_fingerprint: snap.host_fingerprint.as_deref(),
            files_total: snap.files_total,
            files_transferred: snap.files_transferred,
            files_skipped: snap.files_skipped,
            files_failed: snap.files_failed,
            bytes_total: snap.bytes_total,
            bytes_transferred: snap.bytes_transferred,
            error: error.as_deref(),
        },
    )
    .await
    {
        tracing::error!(
            job = %active.id,
            error = %err,
            "Server Importer: failed to persist finished import status"
        );
    }
}

async fn execute_import(
    state: &State,
    server: &mut Server,
    user_uuid: Uuid,
    settings: &Settings,
    request: StartRequest,
    dest_path: &str,
    active: &Arc<ActiveImport>,
) -> Result<(), anyhow::Error> {
    let node = server.node.fetch_cached(&state.database).await?;
    let ignored_files = server.subuser_ignored_files.clone();
    let wings = node
        .api_client(&state.database)
        .await?
        .ignoring(ignored_files.clone().unwrap_or_default());

    let mut session = SftpSession::connect(
        &state.env,
        ConnectParams {
            host: request.host,
            port: request.port,
            username: request.username,
            auth: request.auth,
            expected_fingerprint: request.expected_fingerprint,
            timeout: Duration::from_secs(u64::from(settings.connection_timeout_secs)),
            allow_private_networks: settings.allow_private_networks,
        },
    )
    .await?;

    if active.cancel.is_cancelled() {
        bail!("Import cancelled");
    }

    {
        let mut guard = active.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !session.fingerprint.is_empty() {
            guard.host_fingerprint = Some(session.fingerprint.clone());
        }
        guard.phase = Phase::Scanning;
    }

    let scan = session
        .client
        .scan_tree(
            &request.source_path,
            settings.max_import_files_limit(),
            settings.max_import_bytes(),
            &active.cancel,
            false,
        )
        .await?;

    active
        .files_total
        .store(scan.files.len() as u64, Ordering::Relaxed);
    active
        .bytes_total
        .store(scan.total_bytes, Ordering::Relaxed);
    active
        .files_skipped
        .fetch_add(scan.symlink_count + scan.skipped_entries, Ordering::Relaxed);

    {
        let mut guard = active.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.source_path = scan.resolved_root.clone();
        guard.phase = Phase::Transferring;
        guard.transfer_started = Some(Instant::now());
    }

    let mut created_dirs: HashSet<String> = HashSet::new();
    created_dirs.insert("/".to_string());

    ensure_dir_chain(
        &wings,
        server,
        dest_path,
        &mut created_dirs,
    )
    .await?;

    for rel_dir in &scan.directories {
        if active.cancel.is_cancelled() {
            bail!("Import cancelled");
        }
        let target_dir = join_posix(dest_path, rel_dir);
        if server.is_ignored(&target_dir, true) {
            continue;
        }
        if let Err(err) = ensure_dir_chain(&wings, server, &target_dir, &mut created_dirs).await {
            tracing::debug!(
                dir = %target_dir,
                error = %err,
                "Server Importer: could not pre-create directory"
            );
        }
    }

    for file in &scan.files {
        if active.cancel.is_cancelled() {
            bail!("Import cancelled");
        }

        let target_file = join_posix(dest_path, &file.relative_path);
        if server.is_ignored(&target_file, false) {
            active.files_skipped.fetch_add(1, Ordering::Relaxed);
            continue;
        }

        {
            let mut guard = active.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.current_file = Some(file.relative_path.clone());
        }

        if let Some((parent, _)) = split_parent(&target_file)
            && !parent.is_empty()
            && parent != "/"
            && let Err(err) = ensure_dir_chain(&wings, server, parent, &mut created_dirs).await
        {
            active.record_file_error(format!("{}: {err}", file.relative_path));
            continue;
        }

        let (mut writer, reader) = tokio::io::duplex(DUPLEX_BUFFER_BYTES);
        let query = wings_api::servers_server_files_write::post::Query {
            file: Some(target_file.clone().into()),
            user: Some(user_uuid),
            ignored: ignored_files.clone(),
            ..Default::default()
        };

        let sftp_fut = async {
            let res = session
                .client
                .stream_file_to(
                    &file.remote_path,
                    file.size,
                    &mut writer,
                    &active.cancel,
                    |n| {
                        active.bytes_transferred.fetch_add(n, Ordering::Relaxed);
                    },
                )
                .await;
            let _ = writer.shutdown().await;
            drop(writer);
            res
        };

        let wings_fut = wings.post_servers_server_files_write(
            server.uuid,
            AsyncRequestReader::new(reader),
            &query,
        );

        let (sftp_res, wings_res) = tokio::join!(sftp_fut, wings_fut);

        if active.cancel.is_cancelled() {
            if wings_res.is_ok() {
                cleanup_partial_file(&wings, server.uuid, &target_file).await;
            }
            bail!("Import cancelled");
        }

        match (sftp_res, wings_res) {
            (Ok(_), Ok(_)) => {
                active.files_transferred.fetch_add(1, Ordering::Relaxed);
            }
            (Err(sftp_err), wings_res) => {
                if wings_res.is_ok() {
                    cleanup_partial_file(&wings, server.uuid, &target_file).await;
                }
                let msg = sftp_err.to_string();
                if msg.contains("Connection closed") || msg.contains("broken pipe") {
                    return Err(anyhow!(
                        "Lost SFTP connection while transferring {}: {msg}",
                        file.relative_path
                    ));
                }
                active.record_file_error(format!("{}: {msg}", file.relative_path));
            }
            (Ok(_), Err(wings_err)) => {
                let msg = anyhow::Error::from(wings_err).to_string();
                if msg.contains("no space left") || msg.contains("disk") {
                    return Err(anyhow!(
                        "Server storage error while writing {}: {msg}",
                        file.relative_path
                    ));
                }
                active.record_file_error(format!("{}: {msg}", file.relative_path));
            }
        }
    }

    {
        let mut guard = active.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.current_file = None;
    }

    Ok(())
}

async fn ensure_dir_chain(
    wings: &WingsClient,
    server: &mut Server,
    dir_path: &str,
    created: &mut HashSet<String>,
) -> Result<(), anyhow::Error> {
    let trimmed = dir_path.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "/" || created.contains(trimmed) {
        return Ok(());
    }

    let mut current = String::new();
    for segment in trimmed.trim_start_matches('/').split('/') {
        if segment.is_empty() {
            continue;
        }
        let parent = if current.is_empty() {
            "/".to_string()
        } else {
            current.clone()
        };
        current = format!("{}/{}", current.trim_end_matches('/'), segment);
        if created.contains(&current) {
            continue;
        }
        if server.is_ignored(&current, true) {
            continue;
        }
        let body = wings_api::servers_server_files_create_directory::post::RequestBody {
            root: parent.into(),
            name: segment.into(),
        };
        match wings
            .post_servers_server_files_create_directory(server.uuid, &body)
            .await
        {
            Ok(_) => {
                created.insert(current.clone());
            }
            Err(wings_api::client::ApiHttpError::Http(status, err))
                if status == axum::http::StatusCode::CONFLICT
                    || err.error.to_lowercase().contains("exist") =>
            {
                created.insert(current.clone());
            }
            Err(err) => return Err(anyhow!("Failed to create directory {current}: {}", anyhow::Error::from(err))),
        }
    }

    Ok(())
}

async fn cleanup_partial_file(wings: &WingsClient, server_uuid: Uuid, full_path: &str) {
    let Some((parent, name)) = split_parent(full_path) else {
        return;
    };
    let root = if parent.is_empty() { "/" } else { parent };
    let body = wings_api::servers_server_files_delete::post::RequestBody {
        root: root.into(),
        files: vec![name.into()],
    };
    let _ = wings
        .post_servers_server_files_delete(server_uuid, &body)
        .await;
}

fn split_parent(path: &str) -> Option<(&str, &str)> {
    let trimmed = path.trim_end_matches('/');
    let idx = trimmed.rfind('/')?;
    let name = &trimmed[idx + 1..];
    if name.is_empty() {
        return None;
    }
    let parent = if idx == 0 { "/" } else { &trimmed[..idx] };
    Some((parent, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_parent_handles_root_and_nested_paths() {
        assert_eq!(split_parent("/server.jar"), Some(("/", "server.jar")));
        assert_eq!(
            split_parent("/plugins/essentials/config.yml"),
            Some(("/plugins/essentials", "config.yml"))
        );
        assert_eq!(split_parent("/"), None);
    }
}
