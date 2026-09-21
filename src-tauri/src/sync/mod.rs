pub mod crypto;
pub mod webdav;

use chrono::Utc;
use rand::RngCore;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tracing::info;

use crate::error::{AppError, AppResult};
use crate::sync::crypto::{compress_and_encrypt, derive_sync_key};
use crate::sync::webdav::{WebDavClient, WebDavConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncItem {
    pub uuid: String,
    pub item_type: String, // "bookmark", "collection", "rule"
    pub payload: serde_json::Value,
    pub updated_at: i64,
    #[serde(default)]
    pub deleted_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceManifest {
    pub device_id: String,
    pub device_name: String,
    pub app_version: String,
    pub last_sync_time: i64,
    pub items_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatusReport {
    pub success: bool,
    pub uploaded_items: usize,
    pub downloaded_items: usize,
    pub message: String,
    pub timestamp: i64,
}

const SYNC_SECRET_NAME: &str = "sync_root_key";

/// 获取或生成持久化的端到端同步根密钥 (Hex 编码)。
///
/// 密钥优先保存到系统 Secret Store；旧版本留下的明文文件只在成功迁移
/// 后删除。任何安全存储或清理失败都会中止同步，不能继续返回“成功”。
pub fn get_or_create_sync_secret(app_dir: &Path) -> AppResult<String> {
    if let Some(key) = crate::security::secret_store::get_secret(SYNC_SECRET_NAME, app_dir)
        .map_err(|e| AppError::Security(format!("读取同步密钥失败: {}", e)))?
    {
        let trimmed = key.trim();
        if trimmed.len() == 64 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(trimmed.to_string());
        }
        return Err(AppError::Security(
            "系统安全存储中的同步密钥格式无效".into(),
        ));
    }

    let path = app_dir.join("sync_secret.key");
    if let Ok(key) = fs::read_to_string(&path) {
        let trimmed = key.trim();
        if trimmed.len() != 64 || !trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(AppError::Security(
                "明文同步密钥格式无效，已拒绝继续同步".into(),
            ));
        }
        crate::security::secret_store::set_secret(SYNC_SECRET_NAME, trimmed, app_dir)
            .map_err(|e| AppError::Security(format!("迁移同步密钥到安全存储失败: {}", e)))?;
        fs::remove_file(&path)
            .map_err(|e| AppError::Security(format!("迁移后删除明文同步密钥失败: {}", e)))?;
        return Ok(trimmed.to_string());
    }

    let mut raw_bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut raw_bytes);
    let new_key = hex::encode(raw_bytes);
    crate::security::secret_store::set_secret(SYNC_SECRET_NAME, &new_key, app_dir)
        .map_err(|e| AppError::Security(format!("保存同步密钥到安全存储失败: {}", e)))?;
    Ok(new_key)
}

/// 获取或生成本地唯一的设备 UUID
pub fn get_or_create_device_id(app_dir: &Path) -> String {
    let path = app_dir.join("device_id.txt");
    if let Ok(id) = fs::read_to_string(&path) {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let new_id = format!("device_{}", uuid::Uuid::new_v4());
    let _ = fs::write(&path, &new_id);
    new_id
}

/// 从本地数据库提取待同步的业务实体
pub fn collect_local_sync_items(conn: &Connection) -> AppResult<Vec<SyncItem>> {
    let mut local_items = Vec::new();

    // 收藏真实存储在 favorites + urls，而不是不存在的 bookmarks 表。
    let mut stmt = conn.prepare(
        "SELECT f.url_id, u.url, COALESCE(u.title, ''), f.created_at
         FROM favorites f JOIN urls u ON u.id = f.url_id
         ORDER BY f.url_id",
    )?;
    let rows = stmt.query_map([], |r| {
        let url_id: i64 = r.get(0)?;
        Ok(SyncItem {
            uuid: format!("bookmark:{}", url_id),
            item_type: "bookmark".to_string(),
            payload: serde_json::json!({
                "url_id": url_id,
                "url": r.get::<_, String>(1)?,
                "title": r.get::<_, String>(2)?,
                "is_bookmarked": true,
            }),
            updated_at: r.get(3)?,
            deleted_at: None,
        })
    })?;
    for item in rows {
        local_items.push(item?);
    }

    // 标签关系是真实的 tags + url_tags schema；按 URL/标签关系上传。
    let mut stmt = conn.prepare(
        "SELECT ut.url_id, ut.tag_id, t.name, COALESCE(t.color, ''), ut.created_at
         FROM url_tags ut JOIN tags t ON t.id = ut.tag_id
         ORDER BY ut.url_id, ut.tag_id",
    )?;
    let rows = stmt.query_map([], |r| {
        let url_id: i64 = r.get(0)?;
        let tag_id: i64 = r.get(1)?;
        Ok(SyncItem {
            uuid: format!("tag:{}:{}", url_id, tag_id),
            item_type: "tag".to_string(),
            payload: serde_json::json!({
                "url_id": url_id,
                "tag_id": tag_id,
                "name": r.get::<_, String>(2)?,
                "color": r.get::<_, String>(3)?,
            }),
            updated_at: r.get(4)?,
            deleted_at: None,
        })
    })?;
    for item in rows {
        local_items.push(item?);
    }

    // smart_collections 的真实字段是 id/name/query/filter_json/created_at。
    let mut stmt = conn.prepare(
        "SELECT id, name, query, filter_json, created_at
         FROM smart_collections ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        let id: i64 = r.get(0)?;
        Ok(SyncItem {
            uuid: format!("collection:{}", id),
            item_type: "collection".to_string(),
            payload: serde_json::json!({
                "id": id,
                "name": r.get::<_, String>(1)?,
                "query": r.get::<_, String>(2)?,
                "filter_json": r.get::<_, Option<String>>(3)?,
            }),
            updated_at: r.get(4)?,
            deleted_at: None,
        })
    })?;
    for item in rows {
        local_items.push(item?);
    }

    Ok(local_items)
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::database::migrations::run_migrations;

    #[test]
    fn collect_local_sync_items_uses_real_schema() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO urls (id, url, title, domain) VALUES (1, 'https://example.com', 'Example', 'example.com')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO favorites (url_id, created_at) VALUES (1, 100)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tags (id, name, color, created_at) VALUES (2, 'read', '#fff', 101)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO url_tags (url_id, tag_id, created_at) VALUES (1, 2, 102)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO smart_collections (id, name, query, filter_json, created_at) VALUES (3, 'Docs', 'rust', '{}', 103)",
            [],
        )
        .unwrap();

        let items = collect_local_sync_items(&conn).unwrap();
        assert_eq!(items.len(), 3);
        assert!(items.iter().any(|item| item.item_type == "bookmark"));
        assert!(items.iter().any(|item| item.item_type == "tag"));
        assert!(items.iter().any(|item| item.item_type == "collection"));
    }
}

/// 执行未来 WebDAV 双向同步协议。当前命令层会在调用前 fail-closed，
/// 因此不会把单向密文上传报告为同步成功。
pub async fn upload_and_sync(
    app_dir: &Path,
    config: &WebDavConfig,
    sync_secret: &str,
    local_items: Vec<SyncItem>,
) -> AppResult<SyncStatusReport> {
    if !config.enabled {
        return Err(AppError::Other("WebDAV 同步未启用".into()));
    }

    let client = WebDavClient::new(config.clone())?;

    // 1. 验证联通性
    client.test_connection().await?;

    let remote_base = config.remote_dir.trim_end_matches('/');
    let devices_dir = format!("{}/devices", remote_base);
    client.ensure_directory(remote_base).await?;
    client.ensure_directory(&devices_dir).await?;

    let device_id = get_or_create_device_id(app_dir);
    let sync_key = derive_sync_key(sync_secret)?;

    let items_count = local_items.len();
    if items_count == 0 {
        return Ok(SyncStatusReport {
            success: false,
            uploaded_items: 0,
            downloaded_items: 0,
            message: "本地没有可同步的收藏、标签或智能集合；未上传空同步包".into(),
            timestamp: Utc::now().timestamp_millis(),
        });
    }

    // 2. 序列化、压缩并 AES-256-GCM 加密
    let plaintext_bytes = serde_json::to_vec(&local_items)?;
    let encrypted_blob = compress_and_encrypt(&plaintext_bytes, &sync_key)?;

    // 3. 上传当前设备 blob 与 manifest 到远端
    let blob_path = format!("{}/{}.blob", devices_dir, device_id);
    let manifest_path = format!("{}/{}.manifest.json", devices_dir, device_id);

    client.put_file(&blob_path, encrypted_blob).await?;

    let manifest = DeviceManifest {
        device_id: device_id.clone(),
        device_name: whoami_host_name(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        last_sync_time: Utc::now().timestamp_millis(),
        items_count,
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    client.put_file(&manifest_path, manifest_bytes).await?;

    info!(
        "本地端到端加密同步包已上传至 WebDAV: {} 条数据",
        items_count
    );

    Ok(SyncStatusReport {
        // 当前协议仅上传本设备 blob，尚未实现远端设备发现、下载、冲突合并和落库。
        // 明确报告为未完成，避免把单向备份冒充双向同步。
        success: false,
        uploaded_items: items_count,
        downloaded_items: 0,
        message: format!(
            "已加密上传本设备 {} 项数据；双向远端下载与合并尚未实现，本次不视为同步完成",
            items_count
        ),
        timestamp: Utc::now().timestamp_millis(),
    })
}

fn whoami_host_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "Browsory-Client".to_string())
}
