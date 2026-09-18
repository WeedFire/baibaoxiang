use crate::db;
use crate::models::{UpdateCheckResult, UpdateSettings, UpdateState};
use crate::services::{process_launcher, update_service};
use tauri::AppHandle;

/// 当前程序版本（来自 Cargo.toml / tauri.conf.json 的 version 字段）。
fn current_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

/// 设置页需要的更新状态：当前版本、开关、更新源、上次检查结果。
#[tauri::command]
pub fn get_update_state(app: AppHandle) -> Result<UpdateState, String> {
    let conn = db::get_connection(&app)?;
    Ok(update_service::load_state(&conn, &current_version(&app)))
}

#[tauri::command]
pub fn save_update_settings(
    app: AppHandle,
    enabled: bool,
    source_url: String,
) -> Result<(), String> {
    let conn = db::get_connection(&app)?;
    update_service::save_settings(
        &conn,
        &UpdateSettings {
            enabled,
            source_url,
        },
    )
}

/// 检查更新。`force = false` 时遵守自动检查间隔，直接用上次结果。
#[tauri::command]
pub async fn check_update(app: AppHandle, force: bool) -> Result<UpdateCheckResult, String> {
    let current = current_version(&app);
    // 联网是阻塞操作，丢到阻塞线程池，避免卡住界面
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db::get_connection(&app)?;
        Ok(update_service::check_update(
            &conn,
            &current,
            force,
            update_service::now_secs(),
        ))
    })
    .await
    .map_err(|e| format!("检查更新失败: {}", e))?
}

/// 忽略某个版本（传 None 表示取消忽略）。
#[tauri::command]
pub fn set_ignored_update_version(app: AppHandle, version: Option<String>) -> Result<(), String> {
    let conn = db::get_connection(&app)?;
    update_service::set_ignored_version(&conn, version.as_deref())
}

/// 用系统默认浏览器打开网址（仅允许 http/https）。
#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("只支持打开 http/https 链接".to_string());
    }
    process_launcher::open_url(url)
}
