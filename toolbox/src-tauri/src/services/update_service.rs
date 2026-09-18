use crate::models::{UpdateCheckResult, UpdateManifest, UpdateSettings, UpdateState};
use crate::services::data_service;
use rusqlite::Connection;
use std::cmp::Ordering;
use std::time::Duration;

/// 单次请求超时时间，避免网络异常时界面长时间等待
const TIMEOUT: Duration = Duration::from_secs(8);
/// 自动检查的最小间隔：间隔内直接复用上次结果，不重复联网
pub const CHECK_INTERVAL_SECS: i64 = 6 * 60 * 60;

/// 预置更新源。
///
/// 留空时由用户在「设置 → 版本更新」中填写；若想新装的机器开箱即用，
/// 在这里填上更新清单地址即可（用户仍可在设置里覆盖）。
pub const DEFAULT_SOURCE_URL: &str = "";

const KEY_ENABLED: &str = "update_check_enabled";
const KEY_SOURCE: &str = "update_source_url";
const KEY_IGNORED: &str = "update_ignored_version";
const KEY_LATEST: &str = "update_latest_version";
const KEY_NOTES: &str = "update_latest_notes";
const KEY_URL: &str = "update_latest_url";
const KEY_MANDATORY: &str = "update_latest_mandatory";
const KEY_CHECKED_AT: &str = "update_checked_at";

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

// ---- 设置读写 ----

pub fn load_settings(conn: &Connection) -> UpdateSettings {
    UpdateSettings {
        enabled: data_service::get_setting_bool(conn, KEY_ENABLED, true),
        // 未配置（或用户清空）时回退到预置更新源
        source_url: data_service::get_setting(conn, KEY_SOURCE)
            .ok()
            .flatten()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_SOURCE_URL.to_string()),
    }
}

pub fn save_settings(conn: &Connection, settings: &UpdateSettings) -> Result<(), String> {
    data_service::set_setting(conn, KEY_ENABLED, if settings.enabled { "1" } else { "0" })?;
    data_service::set_setting(conn, KEY_SOURCE, settings.source_url.trim())
}

pub fn ignored_version(conn: &Connection) -> Option<String> {
    data_service::get_setting(conn, KEY_IGNORED)
        .ok()
        .flatten()
        .filter(|v| !v.trim().is_empty())
}

pub fn set_ignored_version(conn: &Connection, version: Option<&str>) -> Result<(), String> {
    match version.map(|v| v.trim()).filter(|v| !v.is_empty()) {
        Some(v) => data_service::set_setting(conn, KEY_IGNORED, v),
        None => data_service::remove_setting(conn, KEY_IGNORED),
    }
}

/// 设置页需要的完整状态。
pub fn load_state(conn: &Connection, current_version: &str) -> UpdateState {
    let cache = load_cache(conn);
    UpdateState {
        current_version: current_version.to_string(),
        settings: load_settings(conn),
        ignored_version: ignored_version(conn),
        latest_version: cache.version,
        checked_at: cache.checked_at,
    }
}

/// 上次检查结果，全部存在 user_settings 里。
struct Cache {
    version: Option<String>,
    notes: Option<String>,
    url: Option<String>,
    mandatory: bool,
    checked_at: Option<i64>,
}

fn load_cache(conn: &Connection) -> Cache {
    let text = |key: &str| {
        data_service::get_setting(conn, key)
            .ok()
            .flatten()
            .filter(|v| !v.trim().is_empty())
    };
    Cache {
        version: text(KEY_LATEST),
        notes: text(KEY_NOTES),
        url: text(KEY_URL),
        mandatory: data_service::get_setting_bool(conn, KEY_MANDATORY, false),
        checked_at: data_service::get_setting_i64(conn, KEY_CHECKED_AT),
    }
}

fn store_cache(conn: &Connection, manifest: &UpdateManifest, now: i64) -> Result<(), String> {
    data_service::set_setting(conn, KEY_LATEST, manifest.version.trim())?;
    data_service::set_setting(conn, KEY_CHECKED_AT, &now.to_string())?;
    data_service::set_setting(conn, KEY_MANDATORY, if manifest.mandatory { "1" } else { "0" })?;

    let optional = [
        (KEY_NOTES, manifest.notes.as_deref()),
        (KEY_URL, manifest.url.as_deref()),
    ];
    for (key, value) in optional {
        match value.map(|v| v.trim()).filter(|v| !v.is_empty()) {
            Some(v) => data_service::set_setting(conn, key, v)?,
            None => data_service::remove_setting(conn, key)?,
        }
    }
    Ok(())
}

// ---- 版本比较 ----

/// "v1.2.3-beta.1" -> [1, 2, 3]：去掉前缀 v 与预发布后缀，非数字片段按 0 处理。
fn version_parts(version: &str) -> Vec<u64> {
    let trimmed = version.trim().trim_start_matches(['v', 'V']);
    let core = trimmed
        .split(['-', '+'])
        .next()
        .unwrap_or(trimmed)
        .trim();
    core.split('.')
        .map(|part| {
            part.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u64>()
                .unwrap_or(0)
        })
        .collect()
}

/// 比较版本号大小（按段比较，缺失的段视为 0）。
pub fn compare_versions(latest: &str, current: &str) -> Ordering {
    let latest_parts = version_parts(latest);
    let current_parts = version_parts(current);
    let len = latest_parts.len().max(current_parts.len());
    for index in 0..len {
        let a = latest_parts.get(index).copied().unwrap_or(0);
        let b = current_parts.get(index).copied().unwrap_or(0);
        match a.cmp(&b) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

/// `latest` 是否比 `current` 新。
pub fn is_newer(latest: &str, current: &str) -> bool {
    compare_versions(latest, current) == Ordering::Greater
}

// ---- 更新清单获取 ----

/// 解析更新清单 JSON（容忍 UTF-8 BOM）。
pub fn parse_manifest(text: &str) -> Result<UpdateManifest, String> {
    let text = text.trim_start_matches('\u{feff}').trim();
    if text.is_empty() {
        return Err("更新清单内容为空".to_string());
    }
    let manifest: UpdateManifest =
        serde_json::from_str(text).map_err(|e| format!("更新清单格式无效: {}", e))?;
    if manifest.version.trim().is_empty() {
        return Err("更新清单缺少 version 字段".to_string());
    }
    Ok(manifest)
}

/// 读取更新清单：`http(s)://` 走网络，其它按本地路径读取（支持局域网共享目录）。
pub fn fetch_manifest(source: &str) -> Result<UpdateManifest, String> {
    let source = source.trim();
    if source.is_empty() {
        return Err("尚未配置更新源地址".to_string());
    }

    let text = if source.starts_with("http://") || source.starts_with("https://") {
        fetch_over_http(source)?
    } else {
        fetch_over_file(source)?
    };
    parse_manifest(&text)
}

fn fetch_over_http(url: &str) -> Result<String, String> {
    let config = ureq::config::Config::builder()
        .timeout_global(Some(TIMEOUT))
        .build();
    let agent = ureq::Agent::new_with_config(config);

    let mut response = agent
        .get(url)
        .call()
        .map_err(|e| format!("请求更新源失败: {}", e))?;
    response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("读取更新源内容失败: {}", e))
}

fn fetch_over_file(source: &str) -> Result<String, String> {
    // 相对路径同样以「程序运行目录」为根，便于放到安装目录里随包分发
    let path = crate::utils::resolve_path(source);
    std::fs::read_to_string(&path)
        .map_err(|e| format!("读取更新清单失败（{}）: {}", path.display(), e))
}

// ---- 检查更新 ----

/// 检查更新。
///
/// - `force = false` 时，若开启自动检查且距上次检查不足 [`CHECK_INTERVAL_SECS`]，
///   直接返回上次结果（`from_cache = true`），不发起网络请求；
/// - 拉取失败时若存在历史结果，则沿用历史结果并附带 `status = "error"`。
pub fn check_update(
    conn: &Connection,
    current_version: &str,
    force: bool,
    now: i64,
) -> UpdateCheckResult {
    let settings = load_settings(conn);
    let ignored = ignored_version(conn);
    let cache = load_cache(conn);
    let skipped_ignored = cache.version.is_some() && cache.version == ignored;

    if !settings.enabled && !force {
        return build_result(
            "disabled",
            &settings,
            current_version,
            &cache,
            skipped_ignored,
            false,
            Some("已关闭自动检查更新".to_string()),
        );
    }

    if settings.source_url.trim().is_empty() {
        return build_result(
            "unconfigured",
            &settings,
            current_version,
            &cache,
            skipped_ignored,
            false,
            Some("尚未配置更新源地址，请在「设置 → 版本更新」中填写".to_string()),
        );
    }

    let cache_is_fresh = cache
        .checked_at
        .map(|ts| now >= ts && now - ts < CHECK_INTERVAL_SECS)
        .unwrap_or(false);
    if !force && cache_is_fresh {
        return build_result(
            "ok",
            &settings,
            current_version,
            &cache,
            skipped_ignored,
            true,
            None,
        );
    }

    match fetch_manifest(&settings.source_url) {
        Ok(manifest) => {
            let _ = store_cache(conn, &manifest, now);
            let cache = load_cache(conn);
            let skipped_ignored = cache.version.is_some() && cache.version == ignored;
            build_result(
                "ok",
                &settings,
                current_version,
                &cache,
                skipped_ignored,
                false,
                None,
            )
        }
        Err(error) => build_result(
            "error",
            &settings,
            current_version,
            &cache,
            skipped_ignored,
            true,
            Some(error),
        ),
    }
}

fn build_result(
    status: &str,
    settings: &UpdateSettings,
    current_version: &str,
    cache: &Cache,
    ignored: bool,
    from_cache: bool,
    message: Option<String>,
) -> UpdateCheckResult {
    let has_update = cache
        .version
        .as_deref()
        .map(|version| is_newer(version, current_version))
        .unwrap_or(false)
        && !ignored;

    UpdateCheckResult {
        status: status.to_string(),
        has_update,
        current_version: current_version.to_string(),
        latest_version: cache.version.clone(),
        notes: cache.notes.clone(),
        download_url: cache.url.clone(),
        mandatory: cache.mandatory,
        ignored,
        from_cache,
        checked_at: cache.checked_at,
        source_url: settings.source_url.clone(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_schema;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    /// 把更新清单写到临时文件，返回可直接作为更新源使用的路径。
    fn manifest_file(json: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update.json");
        std::fs::write(&path, json).unwrap();
        let source = path.to_string_lossy().to_string();
        (dir, source)
    }

    #[test]
    fn compares_versions_segment_by_segment() {
        assert!(is_newer("1.0.2", "1.0.1"));
        assert!(is_newer("1.1", "1.0.9"));
        assert!(is_newer("v1.2.0", "1.1.9"));
        assert!(is_newer("1.0.0.1", "1.0.0"));
        assert!(!is_newer("1.0.1", "1.0.1"));
        assert!(!is_newer("1.0.0", "1.0.1"));
        assert_eq!(compare_versions("1.0", "1.0.0"), Ordering::Equal);
        // 预发布后缀忽略，只比数字段
        assert!(is_newer("1.1.0-beta.1", "1.0.0"));
        assert_eq!(compare_versions("1.0.0-rc1", "1.0.0"), Ordering::Equal);
    }

    #[test]
    fn parses_manifest_with_aliases_and_bom() {
        let text = "\u{feff}{ \"tag_name\": \"v2.0.0\", \"changelog\": \"新功能\", \"html_url\": \"https://example.com/dl\", \"mandatory\": true }";
        let manifest = parse_manifest(text).unwrap();
        assert_eq!(manifest.version, "v2.0.0");
        assert_eq!(manifest.notes.as_deref(), Some("新功能"));
        assert_eq!(manifest.url.as_deref(), Some("https://example.com/dl"));
        assert!(manifest.mandatory);
    }

    #[test]
    fn rejects_manifest_without_version() {
        assert!(parse_manifest("{}").is_err());
        assert!(parse_manifest("not json").is_err());
        assert!(parse_manifest("   ").is_err());
    }

    /// 起一个一次性的本地 HTTP 服务，验证网络分支确实能取回并解析清单。
    #[test]
    fn fetches_manifest_over_http() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 1024];
                let _ = stream.read(&mut buffer);
                let body = r#"{"version":"9.9.9","notes":"来自网络"}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });

        let source = format!("http://127.0.0.1:{}/update.json", port);
        let manifest = fetch_manifest(&source).unwrap();
        assert_eq!(manifest.version, "9.9.9");
        assert_eq!(manifest.notes.as_deref(), Some("来自网络"));
        let _ = server.join();
    }

    #[test]
    fn reports_update_available_from_local_source() {
        let conn = db();
        let (_dir, source) = manifest_file(
            r#"{ "version": "1.0.2", "notes": "修复若干问题", "url": "https://example.com/a.exe" }"#,
        );
        save_settings(
            &conn,
            &UpdateSettings {
                enabled: true,
                source_url: source,
            },
        )
        .unwrap();

        let result = check_update(&conn, "1.0.1", false, 1_000);
        assert_eq!(result.status, "ok");
        assert!(result.has_update);
        assert_eq!(result.latest_version.as_deref(), Some("1.0.2"));
        assert_eq!(result.notes.as_deref(), Some("修复若干问题"));
        assert!(!result.from_cache);
        assert_eq!(result.checked_at, Some(1_000));
    }

    #[test]
    fn reports_up_to_date_when_versions_match() {
        let conn = db();
        let (_dir, source) = manifest_file(r#"{ "version": "1.0.1" }"#);
        save_settings(
            &conn,
            &UpdateSettings {
                enabled: true,
                source_url: source,
            },
        )
        .unwrap();

        let result = check_update(&conn, "1.0.1", false, 1_000);
        assert!(!result.has_update);
        assert_eq!(result.status, "ok");
    }

    /// 距上次检查不到间隔时直接用缓存，连无效更新源都不会去读。
    #[test]
    fn reuses_cache_within_interval() {
        let conn = db();
        let (_dir, source) = manifest_file(r#"{ "version": "1.0.2" }"#);
        save_settings(
            &conn,
            &UpdateSettings {
                enabled: true,
                source_url: source,
            },
        )
        .unwrap();
        assert!(check_update(&conn, "1.0.1", false, 1_000).has_update);

        // 把更新源改成一个不存在的文件，缓存有效期内不应触发读取
        save_settings(
            &conn,
            &UpdateSettings {
                enabled: true,
                source_url: "Z:/__no_such_dir__/update.json".to_string(),
            },
        )
        .unwrap();

        let cached = check_update(&conn, "1.0.1", false, 2_000);
        assert_eq!(cached.status, "ok");
        assert!(cached.from_cache);
        assert!(cached.has_update);

        // 强制检查才会真正去读（此时必然失败，但历史结果仍会带上）
        let forced = check_update(&conn, "1.0.1", true, 2_000);
        assert_eq!(forced.status, "error");
        assert!(forced.has_update);
        assert!(forced.message.is_some());
    }

    #[test]
    fn interval_expiry_triggers_refresh() {
        let conn = db();
        let (_dir, source) = manifest_file(r#"{ "version": "1.0.2" }"#);
        save_settings(
            &conn,
            &UpdateSettings {
                enabled: true,
                source_url: source,
            },
        )
        .unwrap();
        assert!(check_update(&conn, "1.0.1", false, 1_000).has_update);

        let later = 1_000 + CHECK_INTERVAL_SECS + 1;
        let refreshed = check_update(&conn, "1.0.1", false, later);
        assert!(!refreshed.from_cache, "超过间隔后应重新读取更新源");
        assert_eq!(refreshed.checked_at, Some(later));
    }

    #[test]
    fn ignored_version_hides_the_update() {
        let conn = db();
        let (_dir, source) = manifest_file(r#"{ "version": "1.0.2" }"#);
        save_settings(
            &conn,
            &UpdateSettings {
                enabled: true,
                source_url: source,
            },
        )
        .unwrap();

        set_ignored_version(&conn, Some("1.0.2")).unwrap();
        let result = check_update(&conn, "1.0.1", true, 1_000);
        assert!(result.ignored);
        assert!(!result.has_update);

        set_ignored_version(&conn, None).unwrap();
        assert!(check_update(&conn, "1.0.1", true, 1_000).has_update);
    }

    #[test]
    fn disabled_or_unconfigured_checks_do_not_fail() {
        let conn = db();
        save_settings(
            &conn,
            &UpdateSettings {
                enabled: false,
                source_url: String::new(),
            },
        )
        .unwrap();

        let disabled = check_update(&conn, "1.0.1", false, 1_000);
        assert_eq!(disabled.status, "disabled");
        assert!(!disabled.has_update);

        save_settings(
            &conn,
            &UpdateSettings {
                enabled: true,
                source_url: String::new(),
            },
        )
        .unwrap();
        let unconfigured = check_update(&conn, "1.0.1", false, 1_000);
        assert_eq!(unconfigured.status, "unconfigured");
        assert!(!unconfigured.has_update);

        // 设置默认值：开启自动检查
        let state = load_state(&conn, "1.0.1");
        assert_eq!(state.current_version, "1.0.1");
    }
}
