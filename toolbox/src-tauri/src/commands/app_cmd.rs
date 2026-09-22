use crate::db;
use crate::models::{AddAppRequest, AppItem, UpdateAppRequest};
use crate::services::{data_service, icon_service, marketplace_service};
use tauri::AppHandle;

#[tauri::command]
pub fn get_apps_by_group(app: AppHandle, group_id: String) -> Result<Vec<AppItem>, String> {
    let conn = db::get_connection(&app)?;
    data_service::get_apps_by_group(&conn, &group_id)
}

#[tauri::command]
pub fn get_all_apps(app: AppHandle) -> Result<Vec<AppItem>, String> {
    let conn = db::get_connection(&app)?;
    data_service::get_all_apps(&conn)
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
    icon_service::refresh_app_icon(&app, &created);

    Ok(data_service::get_app_by_id(&conn, &created.id)?)
}

#[tauri::command]
pub fn update_app(app: AppHandle, req: UpdateAppRequest) -> Result<AppItem, String> {
    let conn = db::get_connection(&app)?;
    let updated = data_service::update_app(&conn, &req)?;
    icon_service::refresh_app_icon(&app, &updated);

    Ok(data_service::get_app_by_id(&conn, &updated.id)?)
}

#[tauri::command]
pub fn delete_app(app: AppHandle, app_id: String) -> Result<(), String> {
    let conn = db::get_connection(&app)?;
    data_service::delete_app(&conn, &app_id)?;

    // 由插件市场安装（自动登记）的应用被删除后，清除其插件标记，
    // 使市场恢复为「未安装」，用户可以再次下载安装。
    if let Some(base) = crate::utils::app_base_dir() {
        let removed = marketplace_service::remove_markers_for_app(base, &app_id);
        if !removed.is_empty() {
            eprintln!("已清除插件安装标记: {:?}", removed);
        }
    }
    Ok(())
}
