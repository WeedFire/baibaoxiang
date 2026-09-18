mod commands;
mod db;
mod models;
mod services;
mod utils;

use services::python_detector;
use std::path::PathBuf;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            db::init_database(&app_handle)?;
            setup_app_paths(&app_handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_cmd::get_apps_by_group,
            commands::app_cmd::get_app_by_id,
            commands::app_cmd::add_app,
            commands::app_cmd::update_app,
            commands::app_cmd::delete_app,
            commands::group_cmd::get_groups,
            commands::group_cmd::create_group,
            commands::group_cmd::delete_group,
            commands::group_cmd::update_group_sort,
            commands::group_cmd::rename_group,
            commands::system_cmd::launch_app,
            commands::system_cmd::launch_app_as_admin,
            commands::system_cmd::get_launch_stats,
            commands::system_cmd::detect_python,
            commands::system_cmd::detect_script_venv,
            commands::system_cmd::extract_icon,
            commands::system_cmd::open_file_location,
            commands::system_cmd::inspect_app_path,
            commands::system_cmd::export_config_to_file,
            commands::system_cmd::import_config_from_file,
            commands::layout_cmd::save_layout,
            commands::layout_cmd::get_layout,
            commands::layout_cmd::get_all_layouts,
            commands::layout_cmd::clear_group_layouts,
            commands::update_cmd::get_update_state,
            commands::update_cmd::save_update_settings,
            commands::update_cmd::check_update,
            commands::update_cmd::set_ignored_update_version,
            commands::update_cmd::open_external_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 登记程序自身的路径基准：
/// - 「程序运行目录」作为应用中相对路径的根；
/// - 在其中查找随程序安装的内置 Python（`python/python.exe`），
///   它随后会作为 Python 脚本的默认运行环境。
fn setup_app_paths(app: &tauri::AppHandle) {
    let roots = app_roots(app);

    if let Some(base) = roots.first() {
        utils::set_app_base_dir(base.clone());
    }
    if let Some(python) = python_detector::find_bundled_python(&roots) {
        python_detector::set_bundled_python(python);
    }
}

/// 程序自身的根目录候选：资源目录（Windows 上即安装目录）→ 可执行文件目录
/// → 开发调试期的仓库根目录（避免每次改完还要手动复制到 target/debug）。
fn app_roots(app: &tauri::AppHandle) -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(dir) = app.path().resource_dir() {
        roots.push(dir);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
        }
    }
    if cfg!(debug_assertions) {
        roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    }

    roots
}
