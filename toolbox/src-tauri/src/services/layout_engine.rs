use rusqlite::{Connection, params};
use crate::models::LayoutInfo;

pub fn save_layout(conn: &Connection, app_id: &str, pos_x: f64, pos_y: f64) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO layout_info (app_id, pos_x, pos_y) VALUES (?, ?, ?)",
        params![app_id, pos_x, pos_y],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_layout(conn: &Connection, app_id: &str) -> Result<Option<LayoutInfo>, String> {
    let result = conn.query_row(
        "SELECT app_id, pos_x, pos_y FROM layout_info WHERE app_id = ?",
        params![app_id],
        |row| {
            Ok(LayoutInfo {
                app_id: row.get(0)?,
                pos_x: row.get(1)?,
                pos_y: row.get(2)?,
            })
        },
    );

    match result {
        Ok(info) => Ok(Some(info)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn get_all_layouts(conn: &Connection) -> Result<Vec<LayoutInfo>, String> {
    let mut stmt = conn
        .prepare("SELECT app_id, pos_x, pos_y FROM layout_info")
        .map_err(|e| e.to_string())?;

    let layouts = stmt
        .query_map([], |row| {
            Ok(LayoutInfo {
                app_id: row.get(0)?,
                pos_x: row.get(1)?,
                pos_y: row.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(layouts)
}

pub fn clear_group_layout(conn: &Connection, group_id: &str) -> Result<(), String> {
    conn.execute(
        "DELETE FROM layout_info WHERE app_id IN (SELECT id FROM app_items WHERE group_id = ?)",
        params![group_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
