use tauri::State;

use crate::database::DbState;
use crate::error::AppResult;
use crate::license::{
    load_active_license, revoke_license, save_license_key, LicenseCertificate, LicenseInfo,
};

#[tauri::command]
pub fn activate_license(
    license_key: String,
    db: State<'_, DbState>,
) -> AppResult<LicenseCertificate> {
    save_license_key(&db.app_dir, &license_key)
}

#[tauri::command]
pub fn get_license_info(db: State<'_, DbState>) -> AppResult<LicenseInfo> {
    Ok(load_active_license(&db.app_dir))
}

#[tauri::command]
pub fn deactivate_license(db: State<'_, DbState>) -> AppResult<()> {
    revoke_license(&db.app_dir)
}
