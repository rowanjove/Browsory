use chrono::Utc;
use rusqlite::{params, Connection, Transaction};
use tracing::info;

use crate::error::AppResult;

struct Migration {
    version: i32,
    description: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "Initial schema: sources, urls, visits, fts5, import_jobs, settings",
        sql: r#"
        -- 1. Sources table
        CREATE TABLE IF NOT EXISTS sources (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            browser TEXT NOT NULL,
            profile TEXT NOT NULL,
            source_type TEXT NOT NULL,
            history_path TEXT NOT NULL,
            db_fingerprint TEXT,
            last_visit_id INTEGER DEFAULT 0,
            last_visit_time INTEGER DEFAULT 0,
            last_sync_at INTEGER,
            enabled INTEGER NOT NULL DEFAULT 1,
            UNIQUE(browser, profile, history_path)
        );

        -- 2. URLs table
        CREATE TABLE IF NOT EXISTS urls (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            url TEXT UNIQUE NOT NULL,
            title TEXT,
            domain TEXT,
            normalized_url TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_urls_domain ON urls(domain);
        CREATE INDEX IF NOT EXISTS idx_urls_normalized ON urls(normalized_url);

        -- 3. Visits table
        CREATE TABLE IF NOT EXISTS visits (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
            url_id INTEGER NOT NULL REFERENCES urls(id) ON DELETE RESTRICT,
            source_visit_id INTEGER NOT NULL,
            visit_time INTEGER NOT NULL,
            transition INTEGER DEFAULT 0,
            from_visit INTEGER DEFAULT 0,
            visit_duration INTEGER DEFAULT 0,
            event_hash TEXT UNIQUE NOT NULL,
            imported_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_visits_time_id ON visits(visit_time DESC, id DESC);
        CREATE INDEX IF NOT EXISTS idx_visits_source_time ON visits(source_id, visit_time DESC);
        CREATE INDEX IF NOT EXISTS idx_visits_url_id ON visits(url_id);

        -- 4. URLs Full-Text Search table
        CREATE VIRTUAL TABLE IF NOT EXISTS urls_fts USING fts5(
            title,
            url,
            domain,
            content='urls',
            content_rowid='id'
        );

        -- Triggers to synchronize urls with urls_fts
        CREATE TRIGGER IF NOT EXISTS urls_ai AFTER INSERT ON urls BEGIN
            INSERT INTO urls_fts(rowid, title, url, domain) VALUES (new.id, new.title, new.url, new.domain);
        END;

        CREATE TRIGGER IF NOT EXISTS urls_ad AFTER DELETE ON urls BEGIN
            INSERT INTO urls_fts(urls_fts, rowid, title, url, domain) VALUES('delete', old.id, old.title, old.url, old.domain);
        END;

        CREATE TRIGGER IF NOT EXISTS urls_au AFTER UPDATE ON urls BEGIN
            INSERT INTO urls_fts(urls_fts, rowid, title, url, domain) VALUES('delete', old.id, old.title, old.url, old.domain);
            INSERT INTO urls_fts(rowid, title, url, domain) VALUES (new.id, new.title, new.url, new.domain);
        END;

        -- 5. Import Jobs table
        CREATE TABLE IF NOT EXISTS import_jobs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id INTEGER REFERENCES sources(id) ON DELETE SET NULL,
            started_at INTEGER NOT NULL,
            finished_at INTEGER,
            read_count INTEGER DEFAULT 0,
            inserted_count INTEGER DEFAULT 0,
            duplicate_count INTEGER DEFAULT 0,
            failed_count INTEGER DEFAULT 0,
            status TEXT NOT NULL,
            error TEXT
        );

        -- 6. Settings table
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        INSERT OR IGNORE INTO settings (key, value) VALUES ('language', 'zh-CN');
        INSERT OR IGNORE INTO settings (key, value) VALUES ('theme', 'system');
        "#,
    },
    Migration {
        version: 2,
        description: "Add app_security, privacy_rules, and backups tables",
        sql: r#"
        -- 7. App Security table
        CREATE TABLE IF NOT EXISTS app_security (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            pin_enabled INTEGER NOT NULL DEFAULT 0,
            pin_salt TEXT,
            pin_hash TEXT,
            auto_lock_minutes INTEGER NOT NULL DEFAULT 15,
            lock_on_minimize INTEGER NOT NULL DEFAULT 0,
            lock_on_sleep INTEGER NOT NULL DEFAULT 1,
            failed_attempts INTEGER NOT NULL DEFAULT 0,
            locked_until INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0
        );

        INSERT OR IGNORE INTO app_security (id, pin_enabled, auto_lock_minutes, lock_on_minimize, lock_on_sleep, failed_attempts, locked_until, updated_at)
        VALUES (1, 0, 15, 0, 1, 0, 0, 0);

        -- 8. Privacy Rules table
        CREATE TABLE IF NOT EXISTS privacy_rules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            pattern TEXT UNIQUE NOT NULL,
            rule_type TEXT NOT NULL DEFAULT 'private',
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at INTEGER NOT NULL
        );

        INSERT OR IGNORE INTO privacy_rules (pattern, rule_type, enabled, created_at)
        VALUES ('localhost', 'private', 1, 0);
        INSERT OR IGNORE INTO privacy_rules (pattern, rule_type, enabled, created_at)
        VALUES ('127.0.0.1', 'private', 1, 0);
        INSERT OR IGNORE INTO privacy_rules (pattern, rule_type, enabled, created_at)
        VALUES ('*.local', 'private', 1, 0);

        -- 9. Backups table
        CREATE TABLE IF NOT EXISTS backups (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_name TEXT NOT NULL,
            file_path TEXT NOT NULL,
            file_size_bytes INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            status TEXT NOT NULL DEFAULT 'success',
            notes TEXT
        );

        -- Additional default settings
        INSERT OR IGNORE INTO settings (key, value) VALUES ('auto_sync_strategy', 'on_change');
        INSERT OR IGNORE INTO settings (key, value) VALUES ('auto_sync_on_startup_unlock', '1');
        "#,
    },
    Migration {
        version: 3,
        description: "Add tags, url_tags, and favorites tables",
        sql: r#"
        -- 10. Tags & Bookmarks
        CREATE TABLE IF NOT EXISTS tags (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT UNIQUE NOT NULL,
            color TEXT,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS url_tags (
            url_id INTEGER NOT NULL REFERENCES urls(id) ON DELETE CASCADE,
            tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
            created_at INTEGER NOT NULL,
            PRIMARY KEY(url_id, tag_id)
        );

        CREATE TABLE IF NOT EXISTS favorites (
            url_id INTEGER PRIMARY KEY REFERENCES urls(id) ON DELETE CASCADE,
            created_at INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_url_tags_tag ON url_tags(tag_id);
        CREATE INDEX IF NOT EXISTS idx_favorites_time ON favorites(created_at DESC);
        "#,
    },
    Migration {
        version: 4,
        description: "Add master_key_envelope table for Argon2id and Master Key envelope",
        sql: r#"
        CREATE TABLE IF NOT EXISTS master_key_envelope (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            envelope_json TEXT NOT NULL,
            recovery_key_hash TEXT,
            updated_at INTEGER NOT NULL
        );
        "#,
    },
    Migration {
        version: 5,
        description: "Add smart_collections table",
        sql: r#"
        CREATE TABLE IF NOT EXISTS smart_collections (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            query TEXT NOT NULL,
            filter_json TEXT,
            created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_smart_collections_name ON smart_collections(name);
        "#,
    },
    Migration {
        version: 6,
        description: "Add page_embeddings table for semantic search",
        sql: r#"
        CREATE TABLE IF NOT EXISTS page_embeddings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            url_id INTEGER NOT NULL REFERENCES urls(id) ON DELETE CASCADE,
            text_sample TEXT NOT NULL,
            embedding BLOB NOT NULL,
            dimension INTEGER NOT NULL,
            model TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            UNIQUE(url_id, model)
        );
        CREATE INDEX IF NOT EXISTS idx_page_embeddings_url ON page_embeddings(url_id);
        CREATE INDEX IF NOT EXISTS idx_page_embeddings_model ON page_embeddings(model);
        "#,
    },
    Migration {
        version: 7,
        description: "Add topics and url_topics tables for interest intelligence",
        sql: r#"
        CREATE TABLE IF NOT EXISTS topics (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT UNIQUE NOT NULL,
            category TEXT NOT NULL,
            icon TEXT,
            color TEXT,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS url_topics (
            url_id INTEGER NOT NULL REFERENCES urls(id) ON DELETE CASCADE,
            topic_id INTEGER NOT NULL REFERENCES topics(id) ON DELETE CASCADE,
            confidence REAL NOT NULL DEFAULT 1.0,
            PRIMARY KEY (url_id, topic_id)
        );
        CREATE INDEX IF NOT EXISTS idx_url_topics_topic ON url_topics(topic_id);
        "#,
    },
    Migration {
        version: 8,
        description: "Add link_health table for archive verification",
        sql: r#"
        CREATE TABLE IF NOT EXISTS link_health (
            url_id INTEGER PRIMARY KEY REFERENCES urls(id) ON DELETE CASCADE,
            status_code INTEGER,
            is_alive INTEGER NOT NULL DEFAULT 1,
            last_checked_at INTEGER NOT NULL,
            error_message TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_link_health_status ON link_health(is_alive);
        "#,
    },
    Migration {
        version: 9,
        description:
            "Add index on visits(url_id) to eliminate full-table scans during aggregation and joins",
        sql: r#"
        CREATE INDEX IF NOT EXISTS idx_visits_url_id ON visits(url_id);
        "#,
    },
    Migration {
        version: 10,
        description: "Add page_contents and page_contents_fts for full-text article archive",
        sql: r#"
        CREATE TABLE IF NOT EXISTS page_contents (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            url_id INTEGER NOT NULL REFERENCES urls(id) ON DELETE CASCADE,
            title TEXT,
            author TEXT,
            description TEXT,
            markdown TEXT,
            plain_text TEXT NOT NULL,
            word_count INTEGER DEFAULT 0,
            extracted_at INTEGER NOT NULL,
            UNIQUE(url_id)
        );
        CREATE INDEX IF NOT EXISTS idx_page_contents_url ON page_contents(url_id);

        CREATE VIRTUAL TABLE IF NOT EXISTS page_contents_fts USING fts5(
            title,
            author,
            description,
            plain_text,
            content='page_contents',
            content_rowid='id'
        );

        CREATE TRIGGER IF NOT EXISTS page_contents_ai AFTER INSERT ON page_contents BEGIN
            INSERT INTO page_contents_fts(rowid, title, author, description, plain_text)
            VALUES (new.id, new.title, new.author, new.description, new.plain_text);
        END;

        CREATE TRIGGER IF NOT EXISTS page_contents_ad AFTER DELETE ON page_contents BEGIN
            INSERT INTO page_contents_fts(page_contents_fts, rowid, title, author, description, plain_text)
            VALUES ('delete', old.id, old.title, old.author, old.description, old.plain_text);
        END;

        CREATE TRIGGER IF NOT EXISTS page_contents_au AFTER UPDATE ON page_contents BEGIN
            INSERT INTO page_contents_fts(page_contents_fts, rowid, title, author, description, plain_text)
            VALUES ('delete', old.id, old.title, old.author, old.description, old.plain_text);
            INSERT INTO page_contents_fts(rowid, title, author, description, plain_text)
            VALUES (new.id, new.title, new.author, new.description, new.plain_text);
        END;
        "#,
    },
    Migration {
        version: 11,
        description:
            "Add offline_archives and background_jobs tables for Milestone G offline archive",
        sql: r#"
        CREATE TABLE IF NOT EXISTS offline_archives (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            url_id INTEGER NOT NULL REFERENCES urls(id) ON DELETE CASCADE,
            page_uuid TEXT NOT NULL UNIQUE,
            archive_level TEXT NOT NULL DEFAULT 'content',
            relative_path TEXT NOT NULL,
            title TEXT,
            domain TEXT,
            has_markdown INTEGER NOT NULL DEFAULT 0,
            has_snapshot_html INTEGER NOT NULL DEFAULT 0,
            has_screenshot INTEGER NOT NULL DEFAULT 0,
            size_bytes INTEGER NOT NULL DEFAULT 0,
            archived_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_offline_archives_url ON offline_archives(url_id);
        CREATE INDEX IF NOT EXISTS idx_offline_archives_uuid ON offline_archives(page_uuid);
        CREATE INDEX IF NOT EXISTS idx_offline_archives_time ON offline_archives(archived_at DESC);

        CREATE TABLE IF NOT EXISTS background_jobs (
            id TEXT PRIMARY KEY,
            job_type TEXT NOT NULL,
            title TEXT NOT NULL,
            status TEXT NOT NULL,
            progress_current INTEGER NOT NULL DEFAULT 0,
            progress_total INTEGER NOT NULL DEFAULT 0,
            message TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_background_jobs_status ON background_jobs(status, updated_at DESC);
        "#,
    },
];

pub fn run_migrations(conn: &mut Connection) -> AppResult<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            description TEXT NOT NULL,
            applied_at INTEGER NOT NULL
        );
        "#,
    )?;

    let current_version: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    info!("Current database schema version: {}", current_version);

    for migration in MIGRATIONS {
        if migration.version > current_version {
            info!(
                "Applying migration v{}: {}",
                migration.version, migration.description
            );
            let tx = conn.transaction()?;
            apply_migration(&tx, migration)?;
            tx.commit()?;
            info!("Migration v{} applied successfully", migration.version);
        }
    }

    Ok(())
}

fn apply_migration(tx: &Transaction, migration: &Migration) -> AppResult<()> {
    tx.execute_batch(migration.sql)?;
    let now = Utc::now().timestamp_millis();
    tx.execute(
        "INSERT INTO schema_migrations (version, description, applied_at) VALUES (?1, ?2, ?3)",
        params![migration.version, migration.description, now],
    )?;
    Ok(())
}
