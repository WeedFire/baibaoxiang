use tauri::AppHandle;
use crate::db;
use crate::models::LayoutInfo;
use crate::services::layout_engine;

#[tauri::command]
pub fn save_layout(app: AppHandle, app_id: String, pos_x: f64, pos_y: f64) -> Result<(), String> {
    let conn = db::get_connection(&app)?;
    layout_engine::save_layout(&conn, &app_id, pos_x, pos_y)
}

#[tauri::command]
pub fn get_layout(app: AppHandle, app_id: String) -> Result<Option<LayoutInfo>, String> {
    let conn = db::get_connection(&app)?;
    layout_engine::get_layout(&conn, &app_id)
}

#[tauri::command]
pub fn get_all_layouts(app: AppHandle) -> Result<Vec<LayoutInfo>, String> {
    let conn = db::get_connection(&app)?;
    layout_engine::get_all_layouts(&conn)
}

#[tauri::command]
pub fn clear_group_layouts(app: AppHandle, group_id: String) -> Result<(), String> {
    let conn = db::get_connection(&app)?;
    layout_engine::clear_group_layout(&conn, &group_id)
}
