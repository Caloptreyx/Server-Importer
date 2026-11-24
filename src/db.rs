//! Persisted SFTP import job records. Credentials are never stored here.
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
