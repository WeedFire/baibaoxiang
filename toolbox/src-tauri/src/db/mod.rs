use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::AppHandle;
use tauri::Manager;

static DB_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// 建表语句，幂等（全部使用 IF NOT EXISTS），可在测试中直接作用于内存数据库。
const SCHEMA_SQL: &str = "
    CREATE TABLE IF NOT EXISTS app_groups (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        sort_order INTEGER NOT NULL DEFAULT 0,
        is_default INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE IF NOT EXISTS app_items (
        id TEXT PRIMARY KEY,
        group_id TEXT NOT NULL,
        name TEXT NOT NULL,
        executable_path TEXT NOT NULL,
        arguments TEXT DEFAULT '',
        working_directory TEXT DEFAULT '',
        startup_window_style INTEGER NOT NULL DEFAULT 0,
        is_python_script INTEGER NOT NULL DEFAULT 0,
        python_interpreter_path TEXT DEFAULT '',
        show_console INTEGER NOT NULL DEFAULT 0,
        run_as_admin INTEGER NOT NULL DEFAULT 0,
        allow_multiple_instances INTEGER NOT NULL DEFAULT 1,
        icon_path TEXT DEFAULT '',
        sort_order INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL DEFAULT '',
        updated_at TEXT NOT NULL DEFAULT '',
        FOREIGN KEY(group_id) REFERENCES app_groups(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS layout_info (
        app_id TEXT PRIMARY KEY,
        pos_x REAL NOT NULL DEFAULT 0,
        pos_y REAL NOT NULL DEFAULT 0,
        FOREIGN KEY(app_id) REFERENCES app_items(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS user_settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS launch_history (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        app_id TEXT NOT NULL,
        launched_at TEXT NOT NULL DEFAULT (datetime('now')),
        FOREIGN KEY(app_id) REFERENCES app_items(id) ON DELETE CASCADE
    );

    CREATE INDEX IF NOT EXISTS idx_app_items_group ON app_items(group_id);
    CREATE INDEX IF NOT EXISTS idx_launch_history_app_id ON launch_history(app_id);
    CREATE INDEX IF NOT EXISTS idx_launch_history_launched_at ON launch_history(launched_at DESC);
";

pub fn init_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(())
}

pub fn get_db_path(app: &AppHandle) -> Result<PathBuf, String> {
    {
        let guard = DB_PATH.lock().map_err(|e| e.to_string())?;
        if let Some(ref path) = *guard {
            return Ok(path.clone());
        }
    }
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法定位应用数据目录: {}", e))?;
    fs::create_dir_all(&app_dir).map_err(|e| format!("无法创建应用数据目录: {}", e))?;
    let path = app_dir.join("toolbox.db");
    let mut guard = DB_PATH.lock().map_err(|e| e.to_string())?;
    *guard = Some(path.clone());
    Ok(path)
}

pub fn get_connection(app: &AppHandle) -> Result<Connection, String> {
    let db_path = get_db_path(app)?;
    let conn = Connection::open(db_path).map_err(|e| format!("打开数据库失败: {}", e))?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .map_err(|e| e.to_string())?;
    Ok(conn)
}

pub fn init_database(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let conn = get_connection(app)?;
    init_schema(&conn)?;

    let count: i32 = conn.query_row("SELECT COUNT(*) FROM app_groups", [], |row| row.get(0))?;
    if count == 0 {
        conn.execute(
            "INSERT INTO app_groups (id, name, sort_order, is_default) VALUES (?, ?, ?, ?)",
            rusqlite::params!["default", "默认分组", 0, 1],
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        init_schema(&conn).unwrap();
        let n: i32 = conn
            .query_row("SELECT COUNT(*) FROM app_groups", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }
}
