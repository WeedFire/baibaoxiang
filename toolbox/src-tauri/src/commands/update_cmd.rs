use crate::db;
use crate::models::{
    UpdateCheckResult, UpdateInstallResult, UpdateProgress, UpdateSettings, UpdateState,
};
use crate::services::{process_launcher, update_service};
use tauri::{AppHandle, Emitter};

/// 前端监听的下载/安装进度事件名
const PROGRESS_EVENT: &str = "update://progress";

/// 当前程序版本（来自 Cargo.toml / tauri.conf.json 的 version 字段）。
fn current_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

/// 设置页需要的更新状态：当前版本、开关、更新源、公钥、上次检查结果。
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
    pubkey: String,
    auto_install: bool,
) -> Result<(), String> {
    let conn = db::get_connection(&app)?;
    update_service::save_settings(
        &conn,
        &UpdateSettings {
            enabled,
            source_url,
            pubkey,
            auto_install,
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

/// 自动下载并安装更新：下载（带进度事件）→ 验签 → 启动安装程序或原地替换。
///
/// 若启动的是外部安装程序，会在返回结果后自动退出应用，让安装程序完成替换。
#[tauri::command]
pub async fn download_and_install_update(app: AppHandle) -> Result<UpdateInstallResult, String> {
    let current = current_version(&app);
    let emitter = app.clone();
    let exit_handle = app.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        let conn = db::get_connection(&emitter)?;
        update_service::install_update(&conn, &current, &mut |progress: UpdateProgress| {
            // 进度事件：前端据此显示下载百分比与当前阶段
            let _ = emitter.emit(PROGRESS_EVENT, progress);
        })
    })
    .await
    .map_err(|e| format!("安装更新失败: {}", e))??;

    // 安装程序已启动：稍等片刻再退出，好让前端收到结果并提示用户
    if result.installer_started {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            exit_handle.exit(0);
        });
    }

    Ok(result)
}

/// 忽略某个版本（传 None 表示取消忽略）。
#[tauri::command]
pub fn set_ignored_update_version(app: AppHandle, version: Option<String>) -> Result<(), String> {
    let conn = db::get_connection(&app)?;
    update_service::set_ignored_version(&conn, version.as_deref())
}

/// 用系统默认浏览器打开网址（仅允许 http/https，校验在 `open_url` 内）。
#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    process_launcher::open_url(&url)
}
