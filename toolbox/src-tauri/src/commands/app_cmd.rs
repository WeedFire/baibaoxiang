use crate::db;
use crate::models::{AddAppRequest, AppItem, LaunchKind, UpdateAppRequest};
use crate::services::{data_service, icon_service};
use tauri::AppHandle;

/// 提取并保存图标；命令与网页没有本地文件，跳过（失败也不影响应用本身）。
fn refresh_icon(app: &AppHandle, item: &AppItem) {
    if !matches!(
        LaunchKind::of(item),
        LaunchKind::Program | LaunchKind::Python
    ) {
        return;
    }
    match icon_service::extract_icon(
        app,
        &item.executable_path,
        item.python_interpreter_path.as_deref(),
    ) {
        Ok(Some(icon)) => {
            if let Ok(conn) = db::get_connection(app) {
                let _ = data_service::set_icon_path(&conn, &item.id, Some(&icon));
            }
        }
        Ok(None) => {}
        Err(e) => eprintln!("[icon] {}", e),
    }
}

#[tauri::command]
pub fn get_apps_by_group(app: AppHandle, group_id: String) -> Result<Vec<AppItem>, String> {
    let conn = db::get_connection(&app)?;
    data_service::get_apps_by_group(&conn, &group_id)
}

#[tauri::command]
pub fn get_app_by_id(app: AppHandle, app_id: String) -> Result<AppItem, String> {
    let conn = db::get_connection(&app)?;
    data_service::get_app_by_id(&conn, &app_id)
}

#[tauri::command]
pub fn add_app(app: AppHandle, req: AddAppRequest) -> Result<AppItem, String> {
    let conn = db::get_connection(&app)?;
    // 保存后自动提取图标：失败不影响应用本身
    let created = data_service::add_app(&conn, &req)?;
    refresh_icon(&app, &created);

    Ok(data_service::get_app_by_id(&conn, &created.id)?)
}

#[tauri::command]
pub fn update_app(app: AppHandle, req: UpdateAppRequest) -> Result<AppItem, String> {
    let conn = db::get_connection(&app)?;
    let updated = data_service::update_app(&conn, &req)?;
    refresh_icon(&app, &updated);

    Ok(data_service::get_app_by_id(&conn, &updated.id)?)
}

#[tauri::command]
pub fn delete_app(app: AppHandle, app_id: String) -> Result<(), String> {
    let conn = db::get_connection(&app)?;
    data_service::delete_app(&conn, &app_id)
}
