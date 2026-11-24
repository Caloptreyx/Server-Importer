//! Persisted SFTP import job records and user-owned encrypted saved logins.
use chrono::{DateTime, Utc};
use serde::Serialize;
use shared::database::Database;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct JobRecord {
    pub id: Uuid,
    pub server_uuid: Uuid,
    pub user_uuid: Option<Uuid>,
    pub user_username: Option<String>,
    pub host: String,
    pub port: i32,
    pub username: String,
    pub auth_method: String,
    pub host_fingerprint: Option<String>,
    pub source_path: String,
    pub destination_path: String,
    pub status: String,
    pub files_total: i64,
    pub files_transferred: i64,
    pub files_skipped: i64,
    pub files_failed: i64,
    pub bytes_total: i64,
    pub bytes_transferred: i64,
    pub error: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// Public representation of a saved SFTP login. Encrypted secrets are never serialized or returned.
#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct SavedLogin {
    pub id: Uuid,
    pub name: String,
    pub host: String,
    pub port: i32,
    pub username: String,
    pub auth_method: String,
    pub has_passphrase: bool,
    pub default_source_path: String,
    pub host_fingerprint: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Internal record including encrypted credentials, used only inside the backend to connect.
#[derive(sqlx::FromRow)]
pub struct SavedLoginSecret {
    pub id: Uuid,
    pub host: String,
    pub port: i32,
    pub username: String,
    pub auth_method: String,
    pub secret_encrypted: Vec<u8>,
    pub passphrase_encrypted: Option<Vec<u8>>,
    pub host_fingerprint: Option<String>,
}

pub struct NewSavedLogin<'a> {
    pub id: Uuid,
    pub user_uuid: Uuid,
    pub name: &'a str,
    pub host: &'a str,
    pub port: u16,
    pub username: &'a str,
    pub auth_method: &'a str,
    pub secret_encrypted: &'a [u8],
    pub passphrase_encrypted: Option<&'a [u8]>,
    pub default_source_path: &'a str,
    pub host_fingerprint: Option<&'a str>,
}

pub struct UpdateSavedLogin<'a> {
    pub id: Uuid,
    pub user_uuid: Uuid,
    pub name: &'a str,
    pub host: &'a str,
    pub port: u16,
    pub username: &'a str,
    pub default_source_path: &'a str,
    pub host_fingerprint: Option<&'a str>,
    pub credentials: Option<(&'a str, &'a [u8], Option<&'a [u8]>)>,
}

pub struct NewJob<'a> {
    pub id: Uuid,
    pub server_uuid: Uuid,
    pub user_uuid: Option<Uuid>,
    pub user_username: Option<&'a str>,
    pub host: &'a str,
    pub port: u16,
    pub username: &'a str,
    pub auth_method: &'a str,
    pub host_fingerprint: Option<&'a str>,
    pub source_path: &'a str,
    pub destination_path: &'a str,
}

pub struct JobOutcome<'a> {
    pub status: &'a str,
    pub host_fingerprint: Option<&'a str>,
    pub files_total: u64,
    pub files_transferred: u64,
    pub files_skipped: u64,
    pub files_failed: u64,
    pub bytes_total: u64,
    pub bytes_transferred: u64,
    pub error: Option<&'a str>,
}

pub async fn insert_job(db: &Database, job: &NewJob<'_>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO dev_caloptreyx_serverimporter_jobs (
            id, server_uuid, user_uuid, user_username, host, port, username,
            auth_method, host_fingerprint, source_path, destination_path, status
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'running')",
    )
    .bind(job.id)
    .bind(job.server_uuid)
    .bind(job.user_uuid)
    .bind(job.user_username)
    .bind(job.host)
    .bind(i32::from(job.port))
    .bind(job.username)
    .bind(job.auth_method)
    .bind(job.host_fingerprint)
    .bind(job.source_path)
    .bind(job.destination_path)
    .execute(db.write())
    .await?;
    Ok(())
}

pub async fn finish_job(
    db: &Database,
    job_id: Uuid,
    outcome: &JobOutcome<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE dev_caloptreyx_serverimporter_jobs
         SET status = $2,
             host_fingerprint = COALESCE($3, host_fingerprint),
             files_total = $4,
             files_transferred = $5,
             files_skipped = $6,
             files_failed = $7,
             bytes_total = $8,
             bytes_transferred = $9,
             error = $10,
             finished_at = now()
         WHERE id = $1",
    )
    .bind(job_id)
    .bind(outcome.status)
    .bind(outcome.host_fingerprint)
    .bind(outcome.files_total.min(i64::MAX as u64) as i64)
    .bind(outcome.files_transferred.min(i64::MAX as u64) as i64)
    .bind(outcome.files_skipped.min(i64::MAX as u64) as i64)
    .bind(outcome.files_failed.min(i64::MAX as u64) as i64)
    .bind(outcome.bytes_total.min(i64::MAX as u64) as i64)
    .bind(outcome.bytes_transferred.min(i64::MAX as u64) as i64)
    .bind(outcome.error)
    .execute(db.write())
    .await?;
    Ok(())
}

/// On panel startup, mark any previously running imports as interrupted so history never gets stuck.
pub async fn fail_stale_jobs(db: &Database) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE dev_caloptreyx_serverimporter_jobs
         SET status = 'failed',
             error = 'Interrupted by panel restart',
             finished_at = now()
         WHERE status = 'running'",
    )
    .execute(db.write())
    .await?;
    Ok(result.rows_affected())
}

pub async fn list_jobs(
    db: &Database,
    server_uuid: Uuid,
    limit: i64,
) -> Result<Vec<JobRecord>, sqlx::Error> {
    sqlx::query_as::<_, JobRecord>(
        "SELECT id, server_uuid, user_uuid, user_username, host, port, username,
                auth_method, host_fingerprint, source_path, destination_path, status,
                files_total, files_transferred, files_skipped, files_failed,
                bytes_total, bytes_transferred, error, started_at, finished_at
         FROM dev_caloptreyx_serverimporter_jobs
         WHERE server_uuid = $1
         ORDER BY started_at DESC
         LIMIT $2",
    )
    .bind(server_uuid)
    .bind(limit)
    .fetch_all(db.read())
    .await
}

pub async fn list_saved_logins(
    db: &Database,
    user_uuid: Uuid,
) -> Result<Vec<SavedLogin>, sqlx::Error> {
    sqlx::query_as::<_, SavedLogin>(
        "SELECT id, name, host, port, username, auth_method,
                (passphrase_encrypted IS NOT NULL) AS has_passphrase,
                default_source_path, host_fingerprint, created_at, updated_at, last_used_at
         FROM dev_caloptreyx_serverimporter_logins
         WHERE user_uuid = $1
         ORDER BY COALESCE(last_used_at, updated_at) DESC, name ASC",
    )
    .bind(user_uuid)
    .fetch_all(db.read())
    .await
}

pub async fn count_saved_logins(db: &Database, user_uuid: Uuid) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM dev_caloptreyx_serverimporter_logins WHERE user_uuid = $1",
    )
    .bind(user_uuid)
    .fetch_one(db.read())
    .await?;
    Ok(count)
}

pub async fn get_saved_login_secret(
    db: &Database,
    id: Uuid,
    user_uuid: Uuid,
) -> Result<Option<SavedLoginSecret>, sqlx::Error> {
    sqlx::query_as::<_, SavedLoginSecret>(
        "SELECT id, host, port, username, auth_method, secret_encrypted,
                passphrase_encrypted, host_fingerprint
         FROM dev_caloptreyx_serverimporter_logins
         WHERE id = $1 AND user_uuid = $2",
    )
    .bind(id)
    .bind(user_uuid)
    .fetch_optional(db.read())
    .await
}

pub async fn create_saved_login(
    db: &Database,
    login: &NewSavedLogin<'_>,
) -> Result<SavedLogin, sqlx::Error> {
    sqlx::query_as::<_, SavedLogin>(
        "INSERT INTO dev_caloptreyx_serverimporter_logins (
            id, user_uuid, name, host, port, username, auth_method,
            secret_encrypted, passphrase_encrypted, default_source_path, host_fingerprint
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         RETURNING id, name, host, port, username, auth_method,
                   (passphrase_encrypted IS NOT NULL) AS has_passphrase,
                   default_source_path, host_fingerprint, created_at, updated_at, last_used_at",
    )
    .bind(login.id)
    .bind(login.user_uuid)
    .bind(login.name)
    .bind(login.host)
    .bind(i32::from(login.port))
    .bind(login.username)
    .bind(login.auth_method)
    .bind(login.secret_encrypted)
    .bind(login.passphrase_encrypted)
    .bind(login.default_source_path)
    .bind(login.host_fingerprint)
    .fetch_one(db.write())
    .await
}

pub async fn update_saved_login(
    db: &Database,
    update: &UpdateSavedLogin<'_>,
) -> Result<Option<SavedLogin>, sqlx::Error> {
    if let Some((auth_method, secret_encrypted, passphrase_encrypted)) = update.credentials {
        sqlx::query_as::<_, SavedLogin>(
            "UPDATE dev_caloptreyx_serverimporter_logins
             SET name = $3,
                 host = $4,
                 port = $5,
                 username = $6,
                 default_source_path = $7,
                 host_fingerprint = $8,
                 auth_method = $9,
                 secret_encrypted = $10,
                 passphrase_encrypted = $11,
                 updated_at = now()
             WHERE id = $1 AND user_uuid = $2
             RETURNING id, name, host, port, username, auth_method,
                       (passphrase_encrypted IS NOT NULL) AS has_passphrase,
                       default_source_path, host_fingerprint, created_at, updated_at, last_used_at",
        )
        .bind(update.id)
        .bind(update.user_uuid)
        .bind(update.name)
        .bind(update.host)
        .bind(i32::from(update.port))
        .bind(update.username)
        .bind(update.default_source_path)
        .bind(update.host_fingerprint)
        .bind(auth_method)
        .bind(secret_encrypted)
        .bind(passphrase_encrypted)
        .fetch_optional(db.write())
        .await
    } else {
        sqlx::query_as::<_, SavedLogin>(
            "UPDATE dev_caloptreyx_serverimporter_logins
             SET name = $3,
                 host = $4,
                 port = $5,
                 username = $6,
                 default_source_path = $7,
                 host_fingerprint = $8,
                 updated_at = now()
             WHERE id = $1 AND user_uuid = $2
             RETURNING id, name, host, port, username, auth_method,
                       (passphrase_encrypted IS NOT NULL) AS has_passphrase,
                       default_source_path, host_fingerprint, created_at, updated_at, last_used_at",
        )
        .bind(update.id)
        .bind(update.user_uuid)
        .bind(update.name)
        .bind(update.host)
        .bind(i32::from(update.port))
        .bind(update.username)
        .bind(update.default_source_path)
        .bind(update.host_fingerprint)
        .fetch_optional(db.write())
        .await
    }
}

pub async fn touch_saved_login_usage(
    db: &Database,
    id: Uuid,
    user_uuid: Uuid,
    fingerprint: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE dev_caloptreyx_serverimporter_logins
         SET last_used_at = now(),
             host_fingerprint = COALESCE($3, host_fingerprint)
         WHERE id = $1 AND user_uuid = $2",
    )
    .bind(id)
    .bind(user_uuid)
    .bind(fingerprint.filter(|f| !f.trim().is_empty()))
    .execute(db.write())
    .await?;
    Ok(())
}

pub async fn delete_saved_login(
    db: &Database,
    id: Uuid,
    user_uuid: Uuid,
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        "DELETE FROM dev_caloptreyx_serverimporter_logins WHERE id = $1 AND user_uuid = $2",
    )
    .bind(id)
    .bind(user_uuid)
    .execute(db.write())
    .await?;
    Ok(res.rows_affected() > 0)
}
