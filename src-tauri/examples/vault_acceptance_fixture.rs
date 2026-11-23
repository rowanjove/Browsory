//! Creates a brand-new, synthetic portable fixture. Never accepts an existing root.
use app_lib::{
    archive::{migrate_legacy_disk_archives, save_offline_archive, SaveArchivePayload},
    database::repository::get_or_create_source,
    history::sync::{commit_source_records, prepare_source_records},
    security::{backup::execute_encrypted_backup, vault::VaultManager},
};
use rusqlite::{params, Connection};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};
fn main() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("new fixture directory required")?,
    );
    fs::create_dir(&root)?;
    fs::write(root.join(".browsory-synthetic-fixture"), b"synthetic-v1")?;
    fs::write(root.join("portable.txt"), b"isolated acceptance fixture")?;
    let data = root.join("data");
    fs::create_dir(&data)?;
    let browser = root.join("profile-local/Google/Chrome/User Data/Default/History");
    fs::create_dir_all(browser.parent().unwrap())?;
    fs::create_dir(root.join("profile-roaming"))?;
    let now = chrono::Utc::now().timestamp_millis();
    {
        let conn = Connection::open(&browser)?;
        conn.execute_batch("CREATE TABLE urls(id INTEGER PRIMARY KEY,url TEXT,title TEXT);CREATE TABLE visits(id INTEGER PRIMARY KEY,url INTEGER,visit_time INTEGER,from_visit INTEGER,transition INTEGER,visit_duration INTEGER);")?;
        conn.execute("INSERT INTO urls VALUES(1,'https://vault-fixture.test/report','合成档案验收 PRIVATE-DESKTOP-SENTINEL')",[])?;
        for i in 1..=12 {
            conn.execute(
                "INSERT INTO visits VALUES(?1,1,?2,0,1,1000)",
                params![
                    i,
                    app_lib::history::unix_millis_to_webkit_micros(now - i * 60000)
                ],
            )?;
        }
    }
    let vault = VaultManager::new(data.clone());
    let recovery = zeroize::Zeroizing::new(vault.create_vault("Synthetic-fixture-password")?);
    vault.confirm_recovery_key(&recovery)?;
    let (page,backup)=vault.with_conn_mut(|conn|{
        let source=get_or_create_source(conn,"chrome","Default","sqlite",&browser.to_string_lossy())?;
        let prepared=prepare_source_records(source.id,"chrome",&browser,0,None,&data.join("temp"))?;
        let imported=commit_source_records(conn,source.id,"chrome","Default",0,prepared,Instant::now())?;
        assert_eq!(imported.inserted_count,12);
        let url_id:i64=conn.query_row("SELECT id FROM urls WHERE url='https://vault-fixture.test/report'",[],|r|r.get(0))?;
        let page=save_offline_archive(&data,conn,SaveArchivePayload{url_id,archive_level:"full".into(),markdown:Some("# 合成档案验收\nPRIVATE-DESKTOP-SENTINEL\n\n仅供本次桌面验收。".into()),html:Some("<html><body><h1>合成离线页面</h1><p>PRIVATE-DESKTOP-SENTINEL</p></body></html>".into()),title:None,author:None})?;
        let old=data.join("archive/pages").join(&page.page_uuid);fs::create_dir_all(old.join("assets"))?;
        fs::write(old.join("content.md"),b"# PRIVATE-DESKTOP-SENTINEL")?;
        fs::write(old.join("metadata.json"),b"{\"purpose\":\"synthetic acceptance\"}")?;
        fs::write(old.join("assets/note.txt"),b"Synthetic attachment export PRIVATE-DESKTOP-SENTINEL")?;
        let png:&[u8]=&[137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82,0,0,0,1,0,0,0,1,8,6,0,0,0,31,21,196,137,0,0,0,13,73,68,65,84,120,156,99,96,96,96,96,0,0,0,5,0,1,165,246,39,154,0,0,0,0,73,69,78,68,174,66,96,130];
        fs::write(old.join("assets/screenshot.png"),png)?;
        migrate_legacy_disk_archives(&data,conn)?;
        let backup=execute_encrypted_backup(conn,&data,&data.join("backups"),"acceptance",Some("synthetic desktop acceptance"),&vault.master_key().unwrap(),&vault.envelope().unwrap()).map_err(app_lib::error::AppError::Security)?;
        Ok((page,backup))
    })?;
    {
        let old = Connection::open(data.join("archive.db.plaintext.bak"))?;
        old.execute_batch("CREATE TABLE preserved_note(value TEXT);INSERT INTO preserved_note VALUES('PRIVATE-DESKTOP-SENTINEL');")?;
    }
    fs::write(
        root.join("takeout.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"Browser History":[{"url":"https://takeout-fixture.test/","title":"合成 Takeout 导入","time_usec":now*1000}]}),
        )?,
    )?;
    vault.lock()?;
    let report = serde_json::json!({"format_version":1,"synthetic_only":true,"visits":12,"page_uuid":page.page_uuid,"backup_file":Path::new(&backup.file_path).file_name().unwrap().to_string_lossy(),"vault_finishes_locked":true,"keys_in_report":false});
    fs::write(
        root.join("fixture.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("Synthetic portable fixture ready: {}", root.display());
    Ok(())
}
