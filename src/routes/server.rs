//! Server-scoped SFTP importer API, mounted at `/api/client/servers/{server}/server-importer`.
use crate::{
    db,
    importer::{self, StartError, StartRequest},
    settings,
    sftp::{ConnectParams, SftpAuth, SftpSession, validate_host},
};
use axum::{
    http::StatusCode,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;
use shared::{
    GetState, Payload, State,
    models::{
        server::{GetServer, GetServerActivityLogger},
        user::{GetPermissionManager, GetUser},
    },
    response::{ApiResponse, ApiResponseResult},
};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use utoipa_axum::router::OpenApiRouter;

const HISTORY_LIMIT: i64 = 20;
const PREVIEW_TIMEOUT_SECS: u64 = 45;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AuthPayload {
    Password {
        password: String,
    },
    PrivateKey {
        private_key: String,
        #[serde(default)]
        passphrase: Option<String>,
    },
}

impl AuthPayload {
    fn into_auth(self) -> Result<SftpAuth, &'static str> {
        match self {
            Self::Password { password } => {
                if password.is_empty() {
                    return Err("Password is required");
                }
                Ok(SftpAuth::Password(password))
            }
            Self::PrivateKey {
                private_key,
                passphrase,
            } => {
                if private_key.trim().is_empty() {
                    return Err("SSH private key is required");
                }
                Ok(SftpAuth::PrivateKey {
                    private_key,
                    passphrase,
                })
            }
        }
    }
}

#[derive(Deserialize)]
struct BrowsePayload {
    host: String,
    #[serde(default = "default_port")]
    port: u16,
    username: String,
    auth: AuthPayload,
    #[serde(default)]
    path: String,
    #[serde(default)]
    expected_fingerprint: Option<String>,
}

#[derive(Deserialize)]
struct StartPayload {
    host: String,
    #[serde(default = "default_port")]
    port: u16,
    username: String,
    auth: AuthPayload,
    #[serde(default)]
    source_path: String,
    #[serde(default = "default_destination")]
    destination_path: String,
    #[serde(default)]
    expected_fingerprint: Option<String>,
}

fn default_port() -> u16 {
    22
}

fn default_destination() -> String {
    "/".to_string()
}

fn bad_request(message: impl AsRef<str>) -> ApiResponse {
    ApiResponse::error(message).with_status(StatusCode::BAD_REQUEST)
}

async fn status(
    state: GetState,
    permissions: GetPermissionManager,
    server: GetServer,
) -> ApiResponseResult {
    permissions.has_server_permission("server-importer.import")?;
    let cfg = settings::load(&state).await?;
    let active = importer::active_snapshot(server.uuid);
    let history = db::list_jobs(&state.database, server.uuid, HISTORY_LIMIT).await?;

    ApiResponse::new_serialized(json!({
        "active": active,
        "history": history,
        "limits": {
            "max_import_size_mb": cfg.max_import_size_mb,
            "max_import_files": cfg.max_import_files,
            "max_concurrent_imports": cfg.max_concurrent_imports,
        },
    }))
    .ok()
}

async fn browse(
    state: GetState,
    permissions: GetPermissionManager,
    _server: GetServer,
    Payload(data): Payload<BrowsePayload>,
) -> ApiResponseResult {
    permissions.has_server_permission("server-importer.import")?;
    let host = match validate_host(&data.host) {
        Ok(h) => h,
        Err(err) => return bad_request(err.to_string()).ok(),
    };
    let auth = match data.auth.into_auth() {
        Ok(a) => a,
        Err(msg) => return bad_request(msg).ok(),
    };
    let cfg = settings::load(&state).await?;

    let mut session = match SftpSession::connect(
        &state.env,
        ConnectParams {
            host,
            port: data.port,
            username: data.username,
            auth,
            expected_fingerprint: data.expected_fingerprint,
            timeout: Duration::from_secs(u64::from(cfg.connection_timeout_secs)),
            allow_private_networks: cfg.allow_private_networks,
        },
    )
    .await
    {
        Ok(s) => s,
        Err(err) => return bad_request(err.to_string()).ok(),
    };

    let (resolved_path, entries) = match session.client.list_dir(&data.path).await {
        Ok(v) => v,
        Err(err) => return bad_request(err.to_string()).ok(),
    };

    ApiResponse::new_serialized(json!({
        "host_fingerprint": session.fingerprint,
        "resolved_path": resolved_path,
        "entries": entries,
    }))
    .ok()
}

async fn preview(
    state: GetState,
    permissions: GetPermissionManager,
    _server: GetServer,
    Payload(data): Payload<BrowsePayload>,
) -> ApiResponseResult {
    permissions.has_server_permission("server-importer.import")?;
    let host = match validate_host(&data.host) {
        Ok(h) => h,
        Err(err) => return bad_request(err.to_string()).ok(),
    };
    let auth = match data.auth.into_auth() {
        Ok(a) => a,
        Err(msg) => return bad_request(msg).ok(),
    };
    let cfg = settings::load(&state).await?;

    let mut session = match SftpSession::connect(
        &state.env,
        ConnectParams {
            host,
            port: data.port,
            username: data.username,
            auth,
            expected_fingerprint: data.expected_fingerprint,
            timeout: Duration::from_secs(u64::from(cfg.connection_timeout_secs)),
            allow_private_networks: cfg.allow_private_networks,
        },
    )
    .await
    {
        Ok(s) => s,
        Err(err) => return bad_request(err.to_string()).ok(),
    };

    let cancel = CancellationToken::new();
    let scan = match tokio::time::timeout(
        Duration::from_secs(PREVIEW_TIMEOUT_SECS),
        session.client.scan_tree(
            &data.path,
            cfg.max_import_files_limit(),
            cfg.max_import_bytes(),
            &cancel,
            true,
        ),
    )
    .await
    {
        Ok(Ok(scan)) => scan,
        Ok(Err(err)) => return bad_request(err.to_string()).ok(),
        Err(_) => {
            cancel.cancel();
            return bad_request(
                "Folder scan timed out after 45 seconds. You can still start the import directly or pick a smaller subfolder.",
            )
            .ok();
        }
    };

    ApiResponse::new_serialized(json!({
        "host_fingerprint": session.fingerprint,
        "resolved_path": scan.resolved_root,
        "file_count": scan.files.len(),
        "directory_count": scan.directories.len(),
        "symlink_count": scan.symlink_count,
        "total_bytes": scan.total_bytes,
        "truncated": scan.truncated,
        "truncation_reason": scan.truncation_reason,
    }))
    .ok()
}

async fn start(
    state: GetState,
    permissions: GetPermissionManager,
    user: GetUser,
    server: GetServer,
    activity_logger: GetServerActivityLogger,
    Payload(data): Payload<StartPayload>,
) -> ApiResponseResult {
    permissions.has_server_permission("server-importer.import")?;
    permissions.has_server_permission("files.create")?;

    let host = match validate_host(&data.host) {
        Ok(h) => h,
        Err(err) => return bad_request(err.to_string()).ok(),
    };
    let auth = match data.auth.into_auth() {
        Ok(a) => a,
        Err(msg) => return bad_request(msg).ok(),
    };
    let cfg = settings::load(&state).await?;

    let source_path = if data.source_path.trim().is_empty() {
        ".".to_string()
    } else {
        data.source_path.trim().to_string()
    };

    let snapshot = match importer::start_import(
        state.0.clone(),
        server.0,
        user.uuid,
        user.username.to_string(),
        cfg,
        StartRequest {
            host: host.clone(),
            port: data.port,
            username: data.username,
            auth,
            expected_fingerprint: data.expected_fingerprint,
            source_path: source_path.clone(),
            destination_path: data.destination_path.clone(),
        },
    )
    .await
    {
        Ok(snap) => snap,
        Err(StartError::AlreadyRunning) => {
            return ApiResponse::error("An import is already running for this server")
                .with_status(StatusCode::CONFLICT)
                .ok();
        }
        Err(StartError::ConcurrencyLimit(max)) => {
            return ApiResponse::error(format!(
                "The panel is already running the maximum number of concurrent imports ({max}). Please try again shortly."
            ))
            .with_status(StatusCode::TOO_MANY_REQUESTS)
            .ok();
        }
        Err(StartError::Invalid(msg)) => return bad_request(msg).ok(),
        Err(StartError::Internal(err)) => return Err(err.into()),
    };

    activity_logger
        .log(
            "server:importer.start",
            json!({
                "job_id": snapshot.id,
                "host": host,
                "port": data.port,
                "source_path": source_path,
                "destination_path": snapshot.destination_path,
            }),
        )
        .await;

    ApiResponse::new_serialized(json!({ "active": snapshot }))
        .with_status(StatusCode::ACCEPTED)
        .ok()
}

async fn cancel(
    permissions: GetPermissionManager,
    server: GetServer,
    activity_logger: GetServerActivityLogger,
) -> ApiResponseResult {
    permissions.has_server_permission("server-importer.import")?;
    let Some(job_id) = importer::cancel_import(server.uuid) else {
        return ApiResponse::error("No active import for this server")
            .with_status(StatusCode::NOT_FOUND)
            .ok();
    };

    activity_logger
        .log("server:importer.cancel", json!({ "job_id": job_id }))
        .await;

    ApiResponse::new_serialized(json!({ "cancelled": true, "job_id": job_id })).ok()
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .route("/status", get(status))
        .route("/browse", post(browse))
        .route("/preview", post(preview))
        .route("/start", post(start))
        .route("/cancel", post(cancel))
        .with_state(state.clone())
}
