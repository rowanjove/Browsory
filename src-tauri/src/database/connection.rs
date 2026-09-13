use directories::ProjectDirs;
use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing::info;

use super::migrations::run_migrations;
use crate::error::{AppError, AppResult};

#[derive(Clone)]
pub struct DbState {
    pub conn: Arc<Mutex<Connection>>,
    pub app_dir: PathBuf,
    pub temp_dir: PathBuf,
    pub logs_dir: PathBuf,
}

pub fn get_app_directories() -> AppResult<(PathBuf, PathBuf, PathBuf, PathBuf)> {
    let proj_dirs = ProjectDirs::from("com", "browsory", "app")
        .ok_or_else(|| AppError::System("Failed to resolve project directories".into()))?;

    let data_dir = proj_dirs.data_local_dir().to_path_buf();
    let temp_dir = data_dir.join("temp");
    let logs_dir = data_dir.join("logs");
    let db_path = data_dir.join("archive.db");

    fs::create_dir_all(&data_dir)?;
    fs::create_dir_all(&temp_dir)?;
    fs::create_dir_all(&logs_dir)?;

    // Safe migration: copy existing database from legacy path if present
    if !db_path.exists() {
        if let Some(old_dirs) = ProjectDirs::from("com", "browserhistory", "archive") {
            let old_db = old_dirs.data_local_dir().join("archive.db");
            if old_db.exists() {
                info!("Migrating legacy archive database from {:?}", old_db);
                let _ = fs::copy(&old_db, &db_path);
                let old_wal = old_dirs.data_local_dir().join("archive.db-wal");
                if old_wal.exists() {
                    let _ = fs::copy(&old_wal, data_dir.join("archive.db-wal"));
                }
                let old_shm = old_dirs.data_local_dir().join("archive.db-shm");
                if old_shm.exists() {
                    let _ = fs::copy(&old_shm, data_dir.join("archive.db-shm"));
                }
            }
        }
    }

    Ok((data_dir, temp_dir, logs_dir, db_path))
}

pub fn init_database() -> AppResult<DbState> {
    let (app_dir, temp_dir, logs_dir, db_path) = get_app_directories()?;
    info!("Initializing archive database at: {:?}", db_path);

    let mut conn = Connection::open(&db_path)?;
    info!("Step 1: Opened connection to {:?}", db_path);

    // Configure SQLite performance and concurrency
    let _journal_mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    info!("Step 2: journal_mode is {}", _journal_mode);

    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    let _: Result<String, _> = conn.query_row("PRAGMA synchronous = NORMAL", [], |r| r.get(0));
    let _: Result<String, _> = conn.query_row("PRAGMA foreign_keys = ON", [], |r| r.get(0));
    info!("Step 3: Applied PRAGMAs");

    // Run schema migrations
    info!("Step 4: Starting schema migrations");
    run_migrations(&mut conn)?;
    info!("Step 5: Migrations completed successfully");

    // Auto-migrate legacy plain secrets from settings into OS Secret Store
    crate::security::secret_store::migrate_plain_secrets_from_db(&conn, &app_dir);

    Ok(DbState {
        conn: Arc::new(Mutex::new(conn)),
        app_dir,
        temp_dir,
        logs_dir,
    })
}
