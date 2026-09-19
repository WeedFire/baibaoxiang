use crate::db;
use crate::models::{AppGroup, AppItem, LaunchResult, PythonInstallation};
use crate::services::{data_service, icon_service, process_launcher, python_detector};
use crate::utils;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::AppHandle;

#[derive(Serialize, Deserialize)]
pub struct ConfigExport {
    pub version: u32,
    pub groups: Vec<AppGroup>,
    pub apps: Vec<AppItem>,
    /// app_id -> base64 编码的图标 PNG；导入时写进本机图标缓存目录，实现「换机器也有图标」
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub icons: HashMap<String, String>,
}

#[derive(Serialize, Deserialize)]
pub struct LaunchStats {
    pub recent: Vec<AppItem>,
    pub frequent: Vec<AppItem>,
}

#[tauri::command]
pub fn launch_app(app: AppHandle, app_id: String) -> Result<LaunchResult, String> {
    let conn = db::get_connection(&app)?;
    let app_item = data_service::get_app_by_id(&conn, &app_id)?;

    // 右键菜单里可以临时以管理员身份运行，走独立的命令
    let result = process_launcher::launch_app(&app_item)?;

    if result.ok {
        let _ = conn.execute(
            "INSERT INTO launch_history (app_id) VALUES (?)",
            params![app_id],
        );
    }

    Ok(result)
}

#[tauri::command]
pub fn launch_app_as_admin(app: AppHandle, app_id: String) -> Result<LaunchResult, String> {
    let conn = db::get_connection(&app)?;
    let mut app_item = data_service::get_app_by_id(&conn, &app_id)?;
    app_item.run_as_admin = true;

    let result = process_launcher::launch_app(&app_item)?;
    if result.ok {
        let _ = conn.execute(
            "INSERT INTO launch_history (app_id) VALUES (?)",
            params![app_id],
        );
    }
    Ok(result)
}

#[tauri::command]
pub fn get_launch_stats(app: AppHandle) -> Result<LaunchStats, String> {
    let conn = db::get_connection(&app)?;

    let mut recent_stmt = conn
        .prepare(
            "SELECT app_id, MAX(launched_at) AS t FROM launch_history
             GROUP BY app_id ORDER BY t DESC LIMIT 10",
        )
        .map_err(|e| e.to_string())?;
    let recent_ids: Vec<String> = recent_stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let mut freq_stmt = conn
        .prepare(
            "SELECT app_id, COUNT(*) AS c FROM launch_history
             GROUP BY app_id ORDER BY c DESC, app_id LIMIT 10",
        )
        .map_err(|e| e.to_string())?;
    let freq_ids: Vec<String> = freq_stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    // 应用可能已被删除，逐个跳过失效记录
    let recent = recent_ids
        .iter()
        .filter_map(|id| data_service::get_app_by_id(&conn, id).ok())
        .collect();
    let frequent = freq_ids
        .iter()
        .filter_map(|id| data_service::get_app_by_id(&conn, id).ok())
        .collect();

    Ok(LaunchStats { recent, frequent })
}

#[tauri::command]
pub fn detect_python() -> Result<Vec<PythonInstallation>, String> {
    Ok(python_detector::detect_python_installations())
}

/// 为指定脚本查找就近的虚拟环境解释器（脚本路径可为相对路径）。
#[tauri::command]
pub fn detect_script_venv(script_path: String) -> Result<Option<PythonInstallation>, String> {
    let script = utils::resolve_path(&script_path);
    Ok(python_detector::find_venv_for_script(
        &script.to_string_lossy(),
    ))
}

/// 提取文件图标为 PNG，返回绝对路径（前端需经 convertFileSrc 转换）。
#[tauri::command]
pub fn extract_icon(
    app: AppHandle,
    file_path: String,
    interpreter: Option<String>,
) -> Result<Option<String>, String> {
    icon_service::extract_icon(&app, &file_path, interpreter.as_deref())
}

#[tauri::command]
pub fn open_file_location(path: String) -> Result<(), String> {
    process_launcher::reveal_in_explorer(&path)
}

#[derive(Serialize, Deserialize)]
pub struct PathInspection {
    /// 解析后的绝对路径（相对路径以「程序运行目录」为根展开）
    pub resolved: String,
    pub exists: bool,
    /// 输入是否为相对路径，供界面提示
    pub is_relative: bool,
}

/// 解析路径并判断是否存在：相对路径以程序运行目录（安装目录）为根。
#[tauri::command]
pub fn inspect_app_path(path: String) -> PathInspection {
    let resolved = utils::resolve_path(&path);
    PathInspection {
        resolved: resolved.to_string_lossy().to_string(),
        exists: resolved.exists(),
        is_relative: utils::is_relative_path(&path),
    }
}

pub fn build_export(conn: &rusqlite::Connection) -> Result<ConfigExport, String> {
    let groups = data_service::get_groups(conn)?;
    let apps = data_service::get_all_apps(conn)?;

    // 图标一并内嵌，跨机器导入后无需重新提取（读不出内容的直接跳过，不阻断导出）
    let mut icons = HashMap::new();
    for item in &apps {
        let Some(path) = item.icon_path.as_deref() else {
            continue;
        };
        if let Some(bytes) = icon_service::read_icon_bytes(path) {
            icons.insert(item.id.clone(), utils::base64::encode(&bytes));
        }
    }

    Ok(ConfigExport {
        version: 1,
        groups,
        apps,
        icons,
    })
}

/// 把当前配置写入指定文件（路径由前端文件对话框提供）。
#[tauri::command]
pub fn export_config_to_file(app: AppHandle, path: String) -> Result<(), String> {
    let conn = db::get_connection(&app)?;
    let config = build_export(&conn)?;
    let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("写入文件失败: {}", e))
}

/// 从指定 JSON 文件恢复配置。
#[tauri::command]
pub fn import_config_from_file(app: AppHandle, path: String) -> Result<usize, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读取文件失败: {}", e))?;
    let config: ConfigExport =
        serde_json::from_str(&text).map_err(|e| format!("配置文件格式无效: {}", e))?;
    import_config(app, config)
}

#[tauri::command]
pub fn import_config(app: AppHandle, config: ConfigExport) -> Result<usize, String> {
    let conn = db::get_connection(&app)?;

    if config.version != 1 {
        return Err(format!("不支持的配置版本: {}", config.version));
    }

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

    tx.execute("DELETE FROM launch_history", [])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM layout_info", [])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM app_items", [])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM app_groups WHERE is_default = 0", [])
        .map_err(|e| e.to_string())?;

    for group in &config.groups {
        tx.execute(
            "INSERT OR REPLACE INTO app_groups (id, name, sort_order, is_default)
             VALUES (?, ?, ?, ?)",
            params![group.id, group.name, group.sort_order, group.is_default as i32],
        )
        .map_err(|e| e.to_string())?;
    }

    // 分组必须存在，否则外键约束会让后续插入失败
    tx.execute(
        "INSERT OR IGNORE INTO app_groups (id, name, sort_order, is_default)
         VALUES ('default', '默认分组', 0, 1)",
        [],
    )
    .map_err(|e| e.to_string())?;

    let mut imported = 0usize;
    // 配置里没带图标、本机也找不到图标的记录，导入完成后按可执行文件重新提取
    let mut need_extract: Vec<String> = Vec::new();

    for item in &config.apps {
        let group_exists: bool = tx
            .query_row(
                "SELECT 1 FROM app_groups WHERE id = ?",
                params![item.group_id],
                |_| Ok(()),
            )
            .is_ok();
        let group_id = if group_exists {
            item.group_id.clone()
        } else {
            "default".to_string()
        };

        let icon_path = import_icon_path(&app, item, &config.icons, &mut need_extract);

        tx.execute(
            "INSERT OR REPLACE INTO app_items (id, group_id, name, executable_path, arguments,
             working_directory, startup_window_style, is_python_script, python_interpreter_path,
             show_console, run_as_admin, allow_multiple_instances, icon_path, sort_order,
             created_at, updated_at, launch_kind)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                item.id,
                group_id,
                item.name,
                item.executable_path,
                item.arguments.as_deref().unwrap_or(""),
                item.working_directory.as_deref().unwrap_or(""),
                item.startup_window_style as i32,
                item.is_python_script as i32,
                item.python_interpreter_path.as_deref().unwrap_or(""),
                item.show_console as i32,
                item.run_as_admin as i32,
                item.allow_multiple_instances as i32,
                icon_path,
                item.sort_order,
                item.created_at,
                item.updated_at,
                item.launch_kind,
            ],
        )
        .map_err(|e| e.to_string())?;
        imported += 1;
    }

    tx.commit().map_err(|e| e.to_string())?;

    // 事务提交后再提取图标：提取失败只是没有图标，不影响导入结果
    for app_id in &need_extract {
        if let Ok(item) = data_service::get_app_by_id(&conn, app_id) {
            icon_service::refresh_app_icon(&app, &item);
        }
    }

    Ok(imported)
}

/// 决定导入后该应用使用的图标路径：
/// 1. 配置里带了图标数据 → 写入本机图标缓存目录（换机器也能显示）；
/// 2. 没带但原路径在本机有效 → 沿用（同一台机器上恢复配置）；
/// 3. 都没有 → 留空并登记，导入结束后按可执行文件重新提取。
fn import_icon_path(
    app: &AppHandle,
    item: &AppItem,
    icons: &HashMap<String, String>,
    need_extract: &mut Vec<String>,
) -> String {
    if let Some(encoded) = icons.get(&item.id) {
        match utils::base64::decode(encoded) {
            Ok(bytes) => {
                match icon_service::store_icon_bytes(app, item.icon_path.as_deref(), &bytes) {
                    Ok(path) => return path,
                    Err(e) => eprintln!("[icon] 写入导入图标失败（{}）: {}", item.name, e),
                }
            }
            Err(e) => eprintln!("[icon] 图标数据无效（{}）: {}", item.name, e),
        }
    }

    if let Some(existing) = item.icon_path.as_deref() {
        let path = utils::resolve_path(existing);
        if path.is_file() {
            return path.to_string_lossy().to_string();
        }
    }

    need_extract.push(item.id.clone());
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_schema;
    use crate::models::{AppGroup, WindowStyle};

    fn db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    fn app_item(id: &str, group: &str) -> AppItem {
        AppItem {
            id: id.into(),
            group_id: group.into(),
            name: format!("app-{}", id),
            executable_path: format!("C:/{}.exe", id),
            arguments: None,
            working_directory: None,
            startup_window_style: WindowStyle::Normal,
            launch_kind: 0,
            is_python_script: false,
            python_interpreter_path: None,
            show_console: false,
            run_as_admin: false,
            allow_multiple_instances: true,
            icon_path: None,
            sort_order: 0,
            created_at: "1".into(),
            updated_at: "1".into(),
        }
    }

    #[test]
    fn export_contains_groups_and_apps() {
        let conn = db();
        conn.execute(
            "INSERT INTO app_groups (id, name, sort_order, is_default) VALUES ('default', '默认分组', 0, 1)",
            [],
        )
        .unwrap();
        data_service::add_app(
            &conn,
            &crate::models::AddAppRequest {
                group_id: "default".into(),
                name: "a".into(),
                executable_path: "C:/a.exe".into(),
                arguments: None,
                working_directory: None,
                startup_window_style: 0,
                launch_kind: 0,
                is_python_script: false,
                python_interpreter_path: None,
                show_console: false,
                run_as_admin: false,
                allow_multiple_instances: true,
                icon_path: None,
            },
        )
        .unwrap();

        let export = build_export(&conn).unwrap();
        assert_eq!(export.version, 1);
        assert_eq!(export.groups.len(), 1);
        assert_eq!(export.apps.len(), 1);
    }

    #[test]
    fn export_roundtrip_survives_json() {
        let cfg = ConfigExport {
            version: 1,
            groups: vec![AppGroup {
                id: "default".into(),
                name: "默认分组".into(),
                sort_order: 0,
                is_default: true,
            }],
            apps: vec![app_item("x", "default")],
            icons: HashMap::new(),
        };
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let back: ConfigExport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.apps.len(), 1);
        assert_eq!(back.apps[0].executable_path, "C:/x.exe");
    }

    /// 旧版本导出的 JSON 没有 icons 字段，必须仍能导入。
    #[test]
    fn legacy_export_without_icons_still_parses() {
        let json = r#"{
            "version": 1,
            "groups": [],
            "apps": [{
                "id": "x", "group_id": "default", "name": "记事本",
                "executable_path": "C:/x.exe", "arguments": null, "working_directory": null,
                "startup_window_style": 0, "launch_kind": 0, "is_python_script": false,
                "python_interpreter_path": null, "show_console": false, "run_as_admin": false,
                "allow_multiple_instances": true, "icon_path": "C:/icons/x.png",
                "sort_order": 0, "created_at": "1", "updated_at": "1"
            }]
        }"#;
        let back: ConfigExport = serde_json::from_str(json).unwrap();
        assert!(back.icons.is_empty());
        assert_eq!(back.apps.len(), 1);
    }

    /// 导出时把图标文件内容内嵌进 JSON，导入端才好还原到本机图标目录。
    #[test]
    fn export_embeds_icon_data() {
        let conn = db();
        conn.execute(
            "INSERT INTO app_groups (id, name, sort_order, is_default) VALUES ('default', '默认分组', 0, 1)",
            [],
        )
        .unwrap();
        let created = data_service::add_app(
            &conn,
            &crate::models::AddAppRequest {
                group_id: "default".into(),
                name: "记事本".into(),
                executable_path: "C:/notepad.exe".into(),
                arguments: None,
                working_directory: None,
                startup_window_style: 0,
                launch_kind: 0,
                is_python_script: false,
                python_interpreter_path: None,
                show_console: false,
                run_as_admin: false,
                allow_multiple_instances: true,
                icon_path: None,
            },
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let icon = dir.path().join("0123456789abcdef.png");
        std::fs::write(&icon, b"PNG-BYTES").unwrap();
        conn.execute(
            "UPDATE app_items SET icon_path = ? WHERE id = ?",
            params![icon.to_string_lossy(), created.id],
        )
        .unwrap();

        let export = build_export(&conn).unwrap();
        let encoded = export.icons.get(&created.id).expect("应内嵌图标");
        assert_eq!(utils::base64::decode(encoded).unwrap(), b"PNG-BYTES");

        // JSON 往返后图标数据仍然可用
        let json = serde_json::to_string(&export).unwrap();
        let back: ConfigExport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.icons.get(&created.id), Some(encoded));
    }

    /// 图标文件不存在时导出不报错，只是不内嵌图标。
    #[test]
    fn export_skips_missing_icon_file() {
        let conn = db();
        conn.execute(
            "INSERT INTO app_groups (id, name, sort_order, is_default) VALUES ('default', '默认分组', 0, 1)",
            [],
        )
        .unwrap();
        let created = data_service::add_app(
            &conn,
            &crate::models::AddAppRequest {
                group_id: "default".into(),
                name: "记事本".into(),
                executable_path: "C:/notepad.exe".into(),
                arguments: None,
                working_directory: None,
                startup_window_style: 0,
                launch_kind: 0,
                is_python_script: false,
                python_interpreter_path: None,
                show_console: false,
                run_as_admin: false,
                allow_multiple_instances: true,
                icon_path: Some("C:/definitely/missing.png".into()),
            },
        )
        .unwrap();

        let export = build_export(&conn).unwrap();
        assert!(export.icons.is_empty());
        // 仍然保留原路径（同机恢复时还有机会用上）
        assert_eq!(export.apps[0].id, created.id);
        assert_eq!(
            export.apps[0].icon_path.as_deref(),
            Some("C:/definitely/missing.png")
        );
    }

    /// 完整链路：入库 -> 读回 -> 解析启动计划 -> 真实执行 -> 记录历史。
    #[test]
    fn add_then_launch_a_python_app_end_to_end() {
        let Some(py) = python_detector::detect_python_installations().into_iter().next() else {
            eprintln!("skip: 本机未安装 Python");
            return;
        };

        let conn = db();
        conn.execute(
            "INSERT INTO app_groups (id, name, sort_order, is_default) VALUES ('default', '默认分组', 0, 1)",
            [],
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("job.py");
        let output = dir.path().join("result.txt");
        std::fs::write(
            &script,
            "import sys\nopen(sys.argv[1], 'w', encoding='utf-8').write('done')\n",
        )
        .unwrap();

        let created = data_service::add_app(
            &conn,
            &crate::models::AddAppRequest {
                group_id: "default".into(),
                name: "每晚备份".into(),
                executable_path: script.to_string_lossy().to_string(),
                arguments: Some(output.to_string_lossy().to_string()),
                working_directory: None,
                startup_window_style: 0,
                launch_kind: 1,
                is_python_script: true,
                python_interpreter_path: Some(py.path.clone()),
                show_console: false,
                run_as_admin: false,
                allow_multiple_instances: true,
                icon_path: None,
            },
        )
        .unwrap();

        let stored = data_service::get_app_by_id(&conn, &created.id).unwrap();
        assert!(stored.is_python_script);
        assert_eq!(stored.python_interpreter_path.as_deref(), Some(py.path.as_str()));

        // 未勾选“显示控制台”时，会优先改用同目录的 pythonw.exe（若存在）
        let plan = process_launcher::build_launch_plan(&stored).unwrap();
        assert!(
            plan.program.eq_ignore_ascii_case(&py.path)
                || plan.program
                    == crate::services::python_detector::apply_console_preference(&py.path, false),
            "使用了非预期的解释器: {}",
            plan.program
        );
        assert_eq!(plan.args.len(), 2);

        let result = process_launcher::launch_app(&stored).unwrap();
        assert!(result.ok, "{}", result.message);

        conn.execute(
            "INSERT INTO launch_history (app_id) VALUES (?)",
            params![created.id],
        )
        .unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM launch_history WHERE app_id = ?",
                params![created.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        for _ in 0..100 {
            if output.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert_eq!(std::fs::read_to_string(&output).unwrap(), "done");
    }

    #[test]
    fn window_style_survives_export_roundtrip() {
        let mut item = app_item("y", "default");
        item.startup_window_style = WindowStyle::Minimized;
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"startup_window_style\":2"), "{}", json);
        let back: AppItem = serde_json::from_str(&json).unwrap();
        assert_eq!(back.startup_window_style, WindowStyle::Minimized);
    }
}
