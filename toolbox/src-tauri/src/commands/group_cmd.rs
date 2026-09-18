use crate::db;
use crate::models::AppGroup;
use crate::services::data_service;
use rusqlite::params;
use tauri::AppHandle;

#[tauri::command]
pub fn get_groups(app: AppHandle) -> Result<Vec<AppGroup>, String> {
    let conn = db::get_connection(&app)?;
    data_service::get_groups(&conn)
}

#[tauri::command]
pub fn create_group(app: AppHandle, name: String) -> Result<AppGroup, String> {
    let conn = db::get_connection(&app)?;
    data_service::create_group(&conn, &name)
}

#[tauri::command]
pub fn delete_group(app: AppHandle, group_id: String) -> Result<(), String> {
    let conn = db::get_connection(&app)?;
    data_service::delete_group(&conn, &group_id)
}

#[tauri::command]
pub fn update_group_sort(app: AppHandle, group_id: String, sort_order: i32) -> Result<(), String> {
    let conn = db::get_connection(&app)?;
    conn.execute(
        "UPDATE app_groups SET sort_order = ? WHERE id = ?",
        params![sort_order, group_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn rename_group(app: AppHandle, group_id: String, name: String) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("分组名称不能为空".to_string());
    }
    let conn = db::get_connection(&app)?;
    conn.execute(
        "UPDATE app_groups SET name = ? WHERE id = ?",
        params![name, group_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
