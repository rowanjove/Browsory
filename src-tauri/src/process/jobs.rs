use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundJob {
    pub id: String,
    pub job_type: String,
    pub title: String,
    pub status: String,
    pub progress_current: u64,
    pub progress_total: u64,
    pub message: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn create_job(conn: &Connection, job_type: &str, title: &str, total: u64) -> AppResult<String> {
    let job_id = format!("job_{}", Uuid::new_v4());
    let now = Utc::now().timestamp_millis();

    conn.execute(
        r#"
        INSERT INTO background_jobs (id, job_type, title, status, progress_current, progress_total, message, created_at, updated_at)
        VALUES (?1, ?2, ?3, 'running', 0, ?4, '任务已启动', ?5, ?5)
        "#,
        params![job_id, job_type, title, total as i64, now],
    )?;

    Ok(job_id)
}

pub fn update_job(
    conn: &Connection,
    job_id: &str,
    current: u64,
    status: Option<&str>,
    message: Option<&str>,
) -> AppResult<()> {
    let now = Utc::now().timestamp_millis();
    let status_val = status.unwrap_or("running");

    conn.execute(
        r#"
        UPDATE background_jobs
        SET progress_current = ?1,
            status = ?2,
            message = COALESCE(?3, message),
            updated_at = ?4
        WHERE id = ?5
        "#,
        params![current as i64, status_val, message, now, job_id],
    )?;

    Ok(())
}

pub fn cancel_job(conn: &Connection, job_id: &str) -> AppResult<()> {
    let now = Utc::now().timestamp_millis();
    conn.execute(
        r#"
        UPDATE background_jobs
        SET status = 'cancelled',
            message = '任务已被用户手动取消',
            updated_at = ?1
        WHERE id = ?2 AND status IN ('queued', 'running')
        "#,
        params![now, job_id],
    )?;
    Ok(())
}

pub fn list_jobs(conn: &Connection, limit: u32) -> AppResult<Vec<BackgroundJob>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, job_type, title, status, progress_current, progress_total, message, created_at, updated_at
        FROM background_jobs
        ORDER BY updated_at DESC
        LIMIT ?1
        "#,
    )?;

    let rows = stmt.query_map(params![limit], |r| {
        Ok(BackgroundJob {
            id: r.get(0)?,
            job_type: r.get(1)?,
            title: r.get(2)?,
            status: r.get(3)?,
            progress_current: r.get::<_, i64>(4)? as u64,
            progress_total: r.get::<_, i64>(5)? as u64,
            message: r.get(6)?,
            created_at: r.get(7)?,
            updated_at: r.get(8)?,
        })
    })?;

    Ok(rows.flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::migrations::run_migrations;

    #[test]
    fn test_background_jobs_crud() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        let job_id = create_job(&conn, "embedding", "生成页面向量索引", 100).unwrap();

        let jobs = list_jobs(&conn, 10).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, "running");
        assert_eq!(jobs[0].progress_total, 100);

        update_job(&conn, &job_id, 45, Some("running"), Some("已处理 45%")).unwrap();
        let jobs2 = list_jobs(&conn, 10).unwrap();
        assert_eq!(jobs2[0].progress_current, 45);
        assert_eq!(jobs2[0].message.as_deref(), Some("已处理 45%"));

        cancel_job(&conn, &job_id).unwrap();
        let jobs3 = list_jobs(&conn, 10).unwrap();
        assert_eq!(jobs3[0].status, "cancelled");
    }
}
