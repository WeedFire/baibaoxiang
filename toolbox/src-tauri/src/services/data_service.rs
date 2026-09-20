use crate::models::{AddAppRequest, AppGroup, AppItem, UpdateAppRequest, WindowStyle};
use rusqlite::{params, Connection, Row};
use uuid::Uuid;

const APP_COLUMNS: &str = "id, group_id, name, executable_path, arguments, working_directory,
         startup_window_style, is_python_script, python_interpreter_path,
         show_console, run_as_admin, allow_multiple_instances, icon_path,
         sort_order, created_at, updated_at, launch_kind";

fn row_to_app(row: &Row) -> rusqlite::Result<AppItem> {
    Ok(AppItem {
        id: row.get(0)?,
        group_id: row.get(1)?,
        name: row.get(2)?,
        executable_path: row.get(3)?,
        arguments: opt_string(row.get::<_, Option<String>>(4)?),
        working_directory: opt_string(row.get::<_, Option<String>>(5)?),
        startup_window_style: WindowStyle::from_i32(row.get::<_, i32>(6)?),
        launch_kind: row.get::<_, i32>(16)?,
        is_python_script: row.get::<_, i32>(7)? == 1,
        python_interpreter_path: opt_string(row.get::<_, Option<String>>(8)?),
        show_console: row.get::<_, i32>(9)? == 1,
        run_as_admin: row.get::<_, i32>(10)? == 1,
        allow_multiple_instances: row.get::<_, i32>(11)? == 1,
        icon_path: opt_string(row.get::<_, Option<String>>(12)?),
        sort_order: row.get(13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}

/// 数据库里空串表示“未设置”，统一映射为 None，避免前端显示空白路径。
fn opt_string(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.is_empty())
}

pub fn get_app_by_id(conn: &Connection, app_id: &str) -> Result<AppItem, String> {
    conn.query_row(
        &format!("SELECT {} FROM app_items WHERE id = ?", APP_COLUMNS),
        params![app_id],
        row_to_app,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => format!("应用不存在: {}", app_id),
        other => other.to_string(),
    })
}

pub fn get_groups(conn: &Connection) -> Result<Vec<AppGroup>, String> {
    let mut stmt = conn
        .prepare("SELECT id, name, sort_order, is_default FROM app_groups ORDER BY sort_order, name")
        .map_err(|e| e.to_string())?;

    let groups = stmt
        .query_map([], |row| {
            Ok(AppGroup {
                id: row.get(0)?,
                name: row.get(1)?,
                sort_order: row.get(2)?,
                is_default: row.get::<_, i32>(3)? == 1,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(groups)
}

/// 返回默认分组的 id（用于插件市场自动添加应用）。
/// 找不到默认分组时兜底返回种子值 `default`，避免安装中断。
pub fn get_default_group_id(conn: &Connection) -> Result<String, String> {
    let id: Option<String> = conn
        .query_row(
            "SELECT id FROM app_groups WHERE is_default = 1 ORDER BY sort_order LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();
    Ok(id.unwrap_or_else(|| "default".to_string()))
}

pub fn create_group(conn: &Connection, name: &str) -> Result<AppGroup, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("分组名称不能为空".to_string());
    }
    let id = Uuid::new_v4().to_string();
    let next: i32 = conn
        .query_row("SELECT COALESCE(MAX(sort_order), 0) + 1 FROM app_groups", [], |r| {
            r.get(0)
        })
        .unwrap_or(0);
    conn.execute(
        "INSERT INTO app_groups (id, name, sort_order, is_default) VALUES (?, ?, ?, 0)",
        params![id, name, next],
    )
    .map_err(|e| e.to_string())?;

    Ok(AppGroup {
        id,
        name: name.to_string(),
        sort_order: next,
        is_default: false,
    })
}

pub fn delete_group(conn: &Connection, group_id: &str) -> Result<(), String> {
    let changed = conn
        .execute("DELETE FROM app_groups WHERE id = ? AND is_default = 0", params![group_id])
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err("分组不存在，或默认分组不可删除".to_string());
    }
    Ok(())
}

pub fn get_apps_by_group(conn: &Connection, group_id: &str) -> Result<Vec<AppItem>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {} FROM app_items WHERE group_id = ? ORDER BY sort_order, name",
            APP_COLUMNS
        ))
        .map_err(|e| e.to_string())?;

    let apps = stmt
        .query_map(params![group_id], row_to_app)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(apps)
}

pub fn get_all_apps(conn: &Connection) -> Result<Vec<AppItem>, String> {
    let mut stmt = conn
        .prepare(&format!("SELECT {} FROM app_items", APP_COLUMNS))
        .map_err(|e| e.to_string())?;
    let apps = stmt
        .query_map([], row_to_app)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(apps)
}

fn next_sort_order(conn: &Connection, group_id: &str) -> i32 {
    conn.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM app_items WHERE group_id = ?",
        params![group_id],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

pub fn add_app(conn: &Connection, req: &AddAppRequest) -> Result<AppItem, String> {
    if req.name.trim().is_empty() {
        return Err("应用名称不能为空".to_string());
    }
    if req.executable_path.trim().is_empty() {
        return Err("应用路径不能为空".to_string());
    }
    if !conn
        .query_row(
            "SELECT 1 FROM app_groups WHERE id = ?",
            params![req.group_id],
            |_| Ok(()),
        )
        .is_ok()
    {
        return Err(format!("分组不存在: {}", req.group_id));
    }

    let id = Uuid::new_v4().to_string();
    let now = now_string();
    let sort = next_sort_order(conn, &req.group_id);

    conn.execute(
        "INSERT INTO app_items (id, group_id, name, executable_path, arguments, working_directory,
         startup_window_style, is_python_script, python_interpreter_path, show_console,
         run_as_admin, allow_multiple_instances, icon_path, sort_order, created_at, updated_at,
         launch_kind)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            id,
            req.group_id,
            req.name.trim(),
            req.executable_path.trim(),
            req.arguments.as_deref().unwrap_or("").trim(),
            req.working_directory.as_deref().unwrap_or("").trim(),
            req.startup_window_style,
            req.is_python_script as i32,
            req.python_interpreter_path.as_deref().unwrap_or("").trim(),
            req.show_console as i32,
            req.run_as_admin as i32,
            req.allow_multiple_instances as i32,
            req.icon_path.as_deref().unwrap_or(""),
            sort,
            now,
            now,
            req.launch_kind,
        ],
    )
    .map_err(|e| e.to_string())?;

    get_app_by_id(conn, &id)
}

pub fn update_app(conn: &Connection, req: &UpdateAppRequest) -> Result<AppItem, String> {
    if req.name.trim().is_empty() {
        return Err("应用名称不能为空".to_string());
    }
    if req.executable_path.trim().is_empty() {
        return Err("应用路径不能为空".to_string());
    }

    let now = now_string();
    let changed = conn
        .execute(
            "UPDATE app_items SET group_id=?, name=?, executable_path=?, arguments=?,
             working_directory=?, startup_window_style=?, is_python_script=?,
             python_interpreter_path=?, show_console=?, run_as_admin=?,
             allow_multiple_instances=?, icon_path=?, sort_order=?, updated_at=?,
             launch_kind=?
             WHERE id=?",
            params![
                req.group_id,
                req.name.trim(),
                req.executable_path.trim(),
                req.arguments.as_deref().unwrap_or("").trim(),
                req.working_directory.as_deref().unwrap_or("").trim(),
                req.startup_window_style,
                req.is_python_script as i32,
                req.python_interpreter_path.as_deref().unwrap_or("").trim(),
                req.show_console as i32,
                req.run_as_admin as i32,
                req.allow_multiple_instances as i32,
                req.icon_path.as_deref().unwrap_or(""),
                req.sort_order,
                now,
                req.launch_kind,
                req.id,
            ],
        )
        .map_err(|e| e.to_string())?;

    if changed == 0 {
        return Err(format!("应用不存在: {}", req.id));
    }

    get_app_by_id(conn, &req.id)
}

pub fn set_icon_path(conn: &Connection, app_id: &str, icon_path: Option<&str>) -> Result<(), String> {
    conn.execute(
        "UPDATE app_items SET icon_path=? WHERE id=?",
        params![icon_path.unwrap_or(""), app_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_app(conn: &Connection, app_id: &str) -> Result<(), String> {
    let changed = conn
        .execute("DELETE FROM app_items WHERE id = ?", params![app_id])
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err(format!("应用不存在: {}", app_id));
    }
    Ok(())
}

fn now_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

// ---- user_settings：键值配置（更新检查等） ----

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT value FROM user_settings WHERE key = ?",
        params![key],
        |row| row.get::<_, String>(0),
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(other.to_string()),
    })
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO user_settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn remove_setting(conn: &Connection, key: &str) -> Result<(), String> {
    conn.execute("DELETE FROM user_settings WHERE key = ?", params![key])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 读取整数配置项，缺失或格式不对时返回 None。
pub fn get_setting_i64(conn: &Connection, key: &str) -> Option<i64> {
    get_setting(conn, key)
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse().ok())
}

/// 读取布尔配置项，缺失时用 `default`。
pub fn get_setting_bool(conn: &Connection, key: &str, default: bool) -> bool {
    match get_setting(conn, key).ok().flatten() {
        Some(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        None => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_schema;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO app_groups (id, name, sort_order, is_default) VALUES ('default', '默认分组', 0, 1)",
            [],
        )
        .unwrap();
        conn
    }

    fn req() -> AddAppRequest {
        AddAppRequest {
            group_id: "default".into(),
            name: "记事本".into(),
            executable_path: r"C:\Windows\notepad.exe".into(),
            arguments: Some("--x".into()),
            working_directory: None,
            startup_window_style: 0,
            launch_kind: 0,
            is_python_script: false,
            python_interpreter_path: None,
            show_console: false,
            run_as_admin: false,
            allow_multiple_instances: true,
            icon_path: None,
        }
    }

    #[test]
    fn crud_roundtrip() {
        let conn = db();
        let added = add_app(&conn, &req()).unwrap();
        assert_eq!(added.name, "记事本");
        assert_eq!(added.sort_order, 0);
        assert_eq!(added.arguments.as_deref(), Some("--x"));

        // 空串字段应被映射为 None
        assert_eq!(added.working_directory, None);
        assert_eq!(added.icon_path, None);

        let fetched = get_app_by_id(&conn, &added.id).unwrap();
        assert_eq!(fetched.id, added.id);

        let list = get_apps_by_group(&conn, "default").unwrap();
        assert_eq!(list.len(), 1);

        let mut update = UpdateAppRequest {
            id: added.id.clone(),
            group_id: "default".into(),
            name: "记事本2".into(),
            executable_path: r"C:\Windows\notepad.exe".into(),
            arguments: None,
            working_directory: None,
            startup_window_style: 1,
            launch_kind: 0,
            is_python_script: false,
            python_interpreter_path: None,
            show_console: false,
            run_as_admin: false,
            allow_multiple_instances: true,
            icon_path: None,
            sort_order: 0,
        };
        let updated = update_app(&conn, &update).unwrap();
        assert_eq!(updated.name, "记事本2");
        assert_eq!(updated.startup_window_style, WindowStyle::Maximized);
        assert_eq!(updated.arguments, None);

        delete_app(&conn, &added.id).unwrap();
        assert!(get_apps_by_group(&conn, "default").unwrap().is_empty());
        assert!(get_app_by_id(&conn, &added.id).is_err());

        update.id = "ghost".into();
        assert!(update_app(&conn, &update).is_err());
    }

    #[test]
    fn sort_order_increments() {
        let conn = db();
        let a = add_app(&conn, &req()).unwrap();
        let b = add_app(&conn, &req()).unwrap();
        assert_eq!(a.sort_order, 0);
        assert_eq!(b.sort_order, 1);
    }

    #[test]
    fn rejects_invalid_group() {
        let conn = db();
        let mut r = req();
        r.group_id = "ghost".into();
        assert!(add_app(&conn, &r).is_err());
    }

    #[test]
    fn groups_created_and_deleted() {
        let conn = db();
        let g = create_group(&conn, "开发工具").unwrap();
        assert_eq!(g.name, "开发工具");
        assert_eq!(get_groups(&conn).unwrap().len(), 2);
        delete_group(&conn, &g.id).unwrap();
        assert!(delete_group(&conn, "default").is_err(), "默认分组不可删除");
    }

    #[test]
    fn blank_names_rejected() {
        let conn = db();
        let mut r = req();
        r.name = "  ".into();
        assert!(add_app(&conn, &r).is_err());
        let mut r2 = req();
        r2.executable_path = String::new();
        assert!(add_app(&conn, &r2).is_err());
        assert!(create_group(&conn, "  ").is_err());
    }
}
