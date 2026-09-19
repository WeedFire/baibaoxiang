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
        launch_kind INTEGER NOT NULL DEFAULT 0,
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
    migrate_schema(conn)?;
    Ok(())
}

/// 老库升级：`CREATE TABLE IF NOT EXISTS` 不会给已存在的表补列，
/// 这里按需 ALTER，并保证幂等（单元测试里直接作用于内存库）。
pub fn migrate_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    if !has_column(conn, "app_items", "launch_kind")? {
        conn.execute(
            "ALTER TABLE app_items ADD COLUMN launch_kind INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
        // 旧数据里 is_python_script 就是唯一的种类标志，迁移过来
        conn.execute(
            "UPDATE app_items SET launch_kind = 1 WHERE is_python_script = 1",
            [],
        )?;
    }
    Ok(())
}

fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        if row.get::<_, String>(1)? == column {
            return Ok(true);
        }
    }
    Ok(false)
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

    /// 旧版本建的表没有 launch_kind，升级时要补列并把 Python 脚本标成 1。
    #[test]
    fn migrates_legacy_table_without_launch_kind() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE app_items (
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
                updated_at TEXT NOT NULL DEFAULT ''
            )",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO app_items (id, group_id, name, executable_path, is_python_script)
             VALUES ('x', 'default', '脚本', 'a.py', 1)",
            [],
        )
        .unwrap();

        migrate_schema(&conn).unwrap();
        migrate_schema(&conn).unwrap();

        let kind: i32 = conn
            .query_row("SELECT launch_kind FROM app_items WHERE id = 'x'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(kind, 1);
    }
}
