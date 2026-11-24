//! Server-scoped SFTP importer API, mounted at `/api/client/servers/{server}/server-importer`.
use crate::{
    db::{self, NewSavedLogin, UpdateSavedLogin},
    importer::{self, StartError, StartRequest},
    settings::{self, Settings},
    sftp::{ConnectParams, SftpAuth, SftpSession, normalize_remote_path, validate_host},
};
use axum::{
    extract::Path,
    http::StatusCode,
    routing::{get, post, put},
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
use uuid::Uuid;

const HISTORY_LIMIT: i64 = 20;
const MAX_SAVED_LOGINS_PER_USER: i64 = 50;
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
    #[serde(default)]
    saved_login_id: Option<Uuid>,
    #[serde(default)]
    host: String,
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default)]
    username: String,
    #[serde(default)]
    auth: Option<AuthPayload>,
    #[serde(default)]
    path: String,
    #[serde(default)]
    expected_fingerprint: Option<String>,
}

#[derive(Deserialize)]
struct StartPayload {
    #[serde(default)]
    saved_login_id: Option<Uuid>,
    #[serde(default)]
    host: String,
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default)]
    username: String,
    #[serde(default)]
    auth: Option<AuthPayload>,
    #[serde(default)]
    source_path: String,
    #[serde(default = "default_destination")]
    destination_path: String,
    #[serde(default)]
    expected_fingerprint: Option<String>,
}

#[derive(Deserialize)]
struct CreateSavedLoginPayload {
    name: String,
    host: String,
    #[serde(default = "default_port")]
    port: u16,
    username: String,
    auth: AuthPayload,
    #[serde(default)]
    default_source_path: String,
    #[serde(default)]
    host_fingerprint: Option<String>,
}

#[derive(Deserialize)]
struct UpdateSavedLoginPayload {
    name: String,
    host: String,
    #[serde(default = "default_port")]
    port: u16,
    username: String,
    #[serde(default)]
    auth: Option<AuthPayload>,
    #[serde(default)]
    default_source_path: String,
    #[serde(default)]
    host_fingerprint: Option<String>,
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

fn validate_login_name(name: &str) -> Result<String, &'static str> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Login name is required");
    }
    if trimmed.chars().count() > 128 {
        return Err("Login name must be 128 characters or fewer");
    }
    Ok(trimmed.to_string())
}

fn sanitize_fingerprint(fp: Option<String>) -> Option<String> {
    fp.map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.len() <= 128)
}

struct ResolvedConnection {
    host: String,
    port: u16,
    username: String,
    auth: SftpAuth,
    expected_fingerprint: Option<String>,
    saved_login_id: Option<Uuid>,
}

async fn resolve_connection(
    state: &State,
    user_uuid: Uuid,
    cfg: &Settings,
    saved_login_id: Option<Uuid>,
    host: String,
    port: u16,
    username: String,
    auth: Option<AuthPayload>,
    expected_fingerprint: Option<String>,
) -> Result<ResolvedConnection, ApiResponse> {
    if let Some(auth_payload) = auth {
        let host = validate_host(&host).map_err(|err| bad_request(err.to_string()))?;
        let username = username.trim().to_string();
        if username.is_empty() {
            return Err(bad_request("Username is required"));
        }
        if port == 0 {
            return Err(bad_request("Port must be between 1 and 65535"));
        }
        let auth = auth_payload.into_auth().map_err(bad_request)?;
        return Ok(ResolvedConnection {
            host,
            port,
            username,
            auth,
            expected_fingerprint: sanitize_fingerprint(expected_fingerprint),
            saved_login_id: None,
        });
    }

    let Some(login_id) = saved_login_id else {
        return Err(bad_request(
            "Enter your SFTP credentials or select a saved login",
        ));
    };

    if !cfg.allow_saved_logins {
        return Err(ApiResponse::error(
            "Saved logins have been disabled by the panel administrator",
        )
        .with_status(StatusCode::FORBIDDEN));
    }

    let stored = db::get_saved_login_secret(&state.database, login_id, user_uuid)
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "Server Importer: failed to load saved login");
            ApiResponse::error("Failed to load saved login")
                .with_status(StatusCode::INTERNAL_SERVER_ERROR)
        })?
        .ok_or_else(|| {
            ApiResponse::error("Saved login not found").with_status(StatusCode::NOT_FOUND)
        })?;

    let secret = state
        .database
        .decrypt(stored.secret_encrypted)
        .await
        .map_err(|err| {
            tracing::error!(error = %err, login = %login_id, "Server Importer: failed to decrypt saved login secret");
            ApiResponse::error("Could not decrypt saved credentials. Update or re-save this login.")
                .with_status(StatusCode::INTERNAL_SERVER_ERROR)
        })?
        .to_string();

    let auth = match stored.auth_method.as_str() {
        "password" => SftpAuth::Password(secret),
        "private_key" => {
            let passphrase = match stored.passphrase_encrypted {
                Some(enc) => Some(
                    state
                        .database
                        .decrypt(enc)
                        .await
                        .map_err(|err| {
                            tracing::error!(error = %err, login = %login_id, "Server Importer: failed to decrypt saved key passphrase");
                            ApiResponse::error(
                                "Could not decrypt saved passphrase. Update or re-save this login.",
                            )
                            .with_status(StatusCode::INTERNAL_SERVER_ERROR)
                        })?
                        .to_string(),
                ),
                None => None,
            };
            SftpAuth::PrivateKey {
                private_key: secret,
                passphrase,
            }
        }
        _ => {
            return Err(
                ApiResponse::error("Saved login has an unsupported auth method")
                    .with_status(StatusCode::BAD_REQUEST),
            );
        }
    };

    let stored_port = u16::try_from(stored.port).unwrap_or(22);
    let resolved_host = if host.trim().is_empty() {
        stored.host.clone()
    } else {
        validate_host(&host).map_err(|err| bad_request(err.to_string()))?
    };
    let resolved_port = if port == 0 { stored_port } else { port };
    let resolved_username = if username.trim().is_empty() {
        stored.username
    } else {
        username.trim().to_string()
    };

    let resolved_fingerprint = sanitize_fingerprint(expected_fingerprint).or_else(|| {
        (resolved_host == stored.host && resolved_port == stored_port)
            .then_some(stored.host_fingerprint)
            .flatten()
    });

    Ok(ResolvedConnection {
        host: resolved_host,
        port: resolved_port,
        username: resolved_username,
        auth,
        expected_fingerprint: resolved_fingerprint,
        saved_login_id: Some(stored.id),
    })
}

async fn encrypt_auth(
    state: &State,
    auth: SftpAuth,
) -> Result<(&'static str, Vec<u8>, Option<Vec<u8>>), anyhow::Error> {
    match auth {
        SftpAuth::Password(password) => {
            let encrypted = state.database.encrypt(password).await?;
            Ok(("password", encrypted, None))
        }
        SftpAuth::PrivateKey {
            private_key,
            passphrase,
        } => {
            let encrypted = state.database.encrypt(private_key).await?;
            let enc_pass = match passphrase.filter(|p| !p.is_empty()) {
                Some(pass) => Some(state.database.encrypt(pass).await?),
                None => None,
            };
            Ok(("private_key", encrypted, enc_pass))
        }
    }
}

async fn status(
    state: GetState,
    permissions: GetPermissionManager,
    user: GetUser,
    server: GetServer,
) -> ApiResponseResult {
    permissions.has_server_permission("server-importer.import")?;
    let cfg = settings::load(&state).await?;
    let active = importer::active_snapshot(server.uuid);
    let history = db::list_jobs(&state.database, server.uuid, HISTORY_LIMIT).await?;
    let saved_logins = if cfg.allow_saved_logins {
        db::list_saved_logins(&state.database, user.uuid).await?
    } else {
        Vec::new()
    };

    ApiResponse::new_serialized(json!({
        "active": active,
        "history": history,
        "saved_logins": saved_logins,
        "allow_saved_logins": cfg.allow_saved_logins,
        "limits": {
            "max_import_size_mb": cfg.max_import_size_mb,
            "max_import_files": cfg.max_import_files,
            "max_concurrent_imports": cfg.max_concurrent_imports,
        },
    }))
    .ok()
}

async fn create_saved_login(
    state: GetState,
    permissions: GetPermissionManager,
    user: GetUser,
    _server: GetServer,
    activity_logger: GetServerActivityLogger,
    Payload(data): Payload<CreateSavedLoginPayload>,
) -> ApiResponseResult {
    permissions.has_server_permission("server-importer.import")?;
    let cfg = settings::load(&state).await?;
    if !cfg.allow_saved_logins {
        return ApiResponse::error("Saved logins have been disabled by the panel administrator")
            .with_status(StatusCode::FORBIDDEN)
            .ok();
    }

    let name = match validate_login_name(&data.name) {
        Ok(n) => n,
        Err(msg) => return bad_request(msg).ok(),
    };
    let host = match validate_host(&data.host) {
        Ok(h) => h,
        Err(err) => return bad_request(err.to_string()).ok(),
    };
    let username = data.username.trim().to_string();
    if username.is_empty() || username.len() > 255 {
        return bad_request("Username is required and must be at most 255 characters").ok();
    }
    if data.port == 0 {
        return bad_request("Port must be between 1 and 65535").ok();
    }
    let auth = match data.auth.into_auth() {
        Ok(a) => a,
        Err(msg) => return bad_request(msg).ok(),
    };
    let default_source_path = match normalize_remote_path(&data.default_source_path) {
        Ok(p) if p == "." => "/".to_string(),
        Ok(p) => p,
        Err(err) => return bad_request(err.to_string()).ok(),
    };

    let existing_count = db::count_saved_logins(&state.database, user.uuid).await?;
    if existing_count >= MAX_SAVED_LOGINS_PER_USER {
        return bad_request(format!(
            "You can save at most {MAX_SAVED_LOGINS_PER_USER} SFTP logins. Delete an unused one first."
        ))
        .ok();
    }

    let (auth_method, secret_encrypted, passphrase_encrypted) = encrypt_auth(&state, auth).await?;
    let fingerprint = sanitize_fingerprint(data.host_fingerprint);
    let login = db::create_saved_login(
        &state.database,
        &NewSavedLogin {
            id: Uuid::new_v4(),
            user_uuid: user.uuid,
            name: &name,
            host: &host,
            port: data.port,
            username: &username,
            auth_method,
            secret_encrypted: &secret_encrypted,
            passphrase_encrypted: passphrase_encrypted.as_deref(),
            default_source_path: &default_source_path,
            host_fingerprint: fingerprint.as_deref(),
        },
    )
    .await?;

    activity_logger
        .log(
            "server:importer.saved-login.create",
            json!({
                "login_id": login.id,
                "name": login.name,
                "host": login.host,
                "port": login.port,
                "auth_method": login.auth_method,
            }),
        )
        .await;

    ApiResponse::new_serialized(json!({ "login": login }))
        .with_status(StatusCode::CREATED)
        .ok()
}

async fn update_saved_login(
    state: GetState,
    permissions: GetPermissionManager,
    user: GetUser,
    _server: GetServer,
    activity_logger: GetServerActivityLogger,
    Path((_server_param, login_id)): Path<(String, Uuid)>,
    Payload(data): Payload<UpdateSavedLoginPayload>,
) -> ApiResponseResult {
    permissions.has_server_permission("server-importer.import")?;
    let cfg = settings::load(&state).await?;
    if !cfg.allow_saved_logins {
        return ApiResponse::error("Saved logins have been disabled by the panel administrator")
            .with_status(StatusCode::FORBIDDEN)
            .ok();
    }

    let name = match validate_login_name(&data.name) {
        Ok(n) => n,
        Err(msg) => return bad_request(msg).ok(),
    };
    let host = match validate_host(&data.host) {
        Ok(h) => h,
        Err(err) => return bad_request(err.to_string()).ok(),
    };
    let username = data.username.trim().to_string();
    if username.is_empty() || username.len() > 255 {
        return bad_request("Username is required and must be at most 255 characters").ok();
    }
    if data.port == 0 {
        return bad_request("Port must be between 1 and 65535").ok();
    }
    let default_source_path = match normalize_remote_path(&data.default_source_path) {
        Ok(p) if p == "." => "/".to_string(),
        Ok(p) => p,
        Err(err) => return bad_request(err.to_string()).ok(),
    };

    let encrypted_creds = match data.auth {
        Some(auth_payload) => {
            let auth = match auth_payload.into_auth() {
                Ok(a) => a,
                Err(msg) => return bad_request(msg).ok(),
            };
            Some(encrypt_auth(&state, auth).await?)
        }
        None => None,
    };

    let fingerprint = sanitize_fingerprint(data.host_fingerprint);
    let updated = db::update_saved_login(
        &state.database,
        &UpdateSavedLogin {
            id: login_id,
            user_uuid: user.uuid,
            name: &name,
            host: &host,
            port: data.port,
            username: &username,
            default_source_path: &default_source_path,
            host_fingerprint: fingerprint.as_deref(),
            credentials: encrypted_creds
                .as_ref()
                .map(|(m, s, p)| (*m, s.as_slice(), p.as_deref())),
        },
    )
    .await?;

    let Some(login) = updated else {
        return ApiResponse::error("Saved login not found")
            .with_status(StatusCode::NOT_FOUND)
            .ok();
    };

    activity_logger
        .log(
            "server:importer.saved-login.update",
            json!({
                "login_id": login.id,
                "name": login.name,
                "host": login.host,
                "port": login.port,
            }),
        )
        .await;

    ApiResponse::new_serialized(json!({ "login": login })).ok()
}

async fn delete_saved_login(
    state: GetState,
    permissions: GetPermissionManager,
    user: GetUser,
    _server: GetServer,
    activity_logger: GetServerActivityLogger,
    Path((_server_param, login_id)): Path<(String, Uuid)>,
) -> ApiResponseResult {
    permissions.has_server_permission("server-importer.import")?;
    let deleted = db::delete_saved_login(&state.database, login_id, user.uuid).await?;
    if !deleted {
        return ApiResponse::error("Saved login not found")
            .with_status(StatusCode::NOT_FOUND)
            .ok();
    }

    activity_logger
        .log(
            "server:importer.saved-login.delete",
            json!({ "login_id": login_id }),
        )
        .await;

    ApiResponse::new_serialized(json!({ "deleted": true })).ok()
}

async fn browse(
    state: GetState,
    permissions: GetPermissionManager,
    user: GetUser,
    _server: GetServer,
    Payload(data): Payload<BrowsePayload>,
) -> ApiResponseResult {
    permissions.has_server_permission("server-importer.import")?;
    let cfg = settings::load(&state).await?;

    let conn = match resolve_connection(
        &state,
        user.uuid,
        &cfg,
        data.saved_login_id,
        data.host,
        data.port,
        data.username,
        data.auth,
        data.expected_fingerprint,
    )
    .await
    {
        Ok(c) => c,
        Err(resp) => return resp.ok(),
    };

    let mut session = match SftpSession::connect(
        &state.env,
        ConnectParams {
            host: conn.host,
            port: conn.port,
            username: conn.username,
            auth: conn.auth,
            expected_fingerprint: conn.expected_fingerprint,
            timeout: Duration::from_secs(u64::from(cfg.connection_timeout_secs)),
            allow_private_networks: cfg.allow_private_networks,
        },
    )
    .await
    {
        Ok(s) => s,
        Err(err) => return bad_request(err.to_string()).ok(),
    };

    if let Some(login_id) = conn.saved_login_id {
        let _ = db::touch_saved_login_usage(
            &state.database,
            login_id,
            user.uuid,
            Some(&session.fingerprint),
        )
        .await;
    }

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
    user: GetUser,
    _server: GetServer,
    Payload(data): Payload<BrowsePayload>,
) -> ApiResponseResult {
    permissions.has_server_permission("server-importer.import")?;
    let cfg = settings::load(&state).await?;

    let conn = match resolve_connection(
        &state,
        user.uuid,
        &cfg,
        data.saved_login_id,
        data.host,
        data.port,
        data.username,
        data.auth,
        data.expected_fingerprint,
    )
    .await
    {
        Ok(c) => c,
        Err(resp) => return resp.ok(),
    };

    let mut session = match SftpSession::connect(
        &state.env,
        ConnectParams {
            host: conn.host,
            port: conn.port,
            username: conn.username,
            auth: conn.auth,
            expected_fingerprint: conn.expected_fingerprint,
            timeout: Duration::from_secs(u64::from(cfg.connection_timeout_secs)),
            allow_private_networks: cfg.allow_private_networks,
        },
    )
    .await
    {
        Ok(s) => s,
        Err(err) => return bad_request(err.to_string()).ok(),
    };

    if let Some(login_id) = conn.saved_login_id {
        let _ = db::touch_saved_login_usage(
            &state.database,
            login_id,
            user.uuid,
            Some(&session.fingerprint),
        )
        .await;
    }

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
    let cfg = settings::load(&state).await?;

    let conn = match resolve_connection(
        &state,
        user.uuid,
        &cfg,
        data.saved_login_id,
        data.host,
        data.port,
        data.username,
        data.auth,
        data.expected_fingerprint,
    )
    .await
    {
        Ok(c) => c,
        Err(resp) => return resp.ok(),
    };

    let source_path = if data.source_path.trim().is_empty() {
        ".".to_string()
    } else {
        data.source_path.trim().to_string()
    };

    let host_for_log = conn.host.clone();
    let port_for_log = conn.port;
    let used_login_id = conn.saved_login_id;

    let snapshot = match importer::start_import(
        state.0.clone(),
        server.0,
        user.uuid,
        user.username.to_string(),
        cfg,
        StartRequest {
            host: conn.host,
            port: conn.port,
            username: conn.username,
            auth: conn.auth,
            expected_fingerprint: conn.expected_fingerprint,
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

    if let Some(login_id) = used_login_id {
        let _ = db::touch_saved_login_usage(&state.database, login_id, user.uuid, None).await;
    }

    activity_logger
        .log(
            "server:importer.start",
            json!({
                "job_id": snapshot.id,
                "host": host_for_log,
                "port": port_for_log,
                "source_path": source_path,
                "destination_path": snapshot.destination_path,
                "saved_login_id": used_login_id,
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
        .route("/saved-logins", post(create_saved_login))
        .route(
            "/saved-logins/{login_id}",
            put(update_saved_login).delete(delete_saved_login),
        )
        .route("/browse", post(browse))
        .route("/preview", post(preview))
        .route("/start", post(start))
        .route("/cancel", post(cancel))
        .with_state(state.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_name_validation_trims_and_bounds() {
        assert_eq!(validate_login_name("  Old Host  "), Ok("Old Host".into()));
        assert!(validate_login_name("   ").is_err());
        assert!(validate_login_name(&"a".repeat(129)).is_err());
        assert!(validate_login_name(&"a".repeat(128)).is_ok());
    }

    #[test]
    fn fingerprint_sanitization_drops_empty_and_overlong() {
        assert_eq!(sanitize_fingerprint(None), None);
        assert_eq!(sanitize_fingerprint(Some("   ".into())), None);
        assert_eq!(
            sanitize_fingerprint(Some(" SHA256:abc123 ".into())),
            Some("SHA256:abc123".into())
        );
        assert_eq!(sanitize_fingerprint(Some("x".repeat(129))), None);
    }
}
