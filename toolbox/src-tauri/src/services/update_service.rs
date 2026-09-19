use crate::models::{
    UpdateCheckResult, UpdateInstallResult, UpdateManifest, UpdateProgress, UpdateSettings,
    UpdateState,
};
use crate::services::{data_service, update_install};
use rusqlite::Connection;
use std::cmp::Ordering;
use std::path::PathBuf;
use std::time::Duration;

/// 单次请求超时时间，避免网络异常时界面长时间等待
const TIMEOUT: Duration = Duration::from_secs(8);
/// 自动检查的最小间隔：间隔内直接复用上次结果，不重复联网
pub const CHECK_INTERVAL_SECS: i64 = 6 * 60 * 60;

/// 更新源（写死在代码里，不暴露给用户）：项目发布在 GitHub，
/// 打 tag 后由 CI 自动构建并生成 Tauri 风格的 `latest.json`。
pub const SOURCE_URL: &str =
    "https://github.com/WeedFire/baibaoxiang/releases/latest/download/latest.json";

/// 更新包验签公钥（写死在代码里，不暴露给用户）。
///
/// 发布前用 `tauri signer generate -w <目录> -p <密码>` 生成密钥对，把
/// `key.pub` 的完整内容填到下面，私钥存为 GitHub Secret
/// `TAURI_SIGNING_PRIVATE_KEY`（`tauri signer` 用它给安装包签名）。
/// 留空表示不校验签名；正式发布前务必填写真实公钥。
pub const PUBLIC_KEY: &str = "";

const ERR_404: &str = "更新源未找到（HTTP 404）：请检查地址，或该项目还没有发布正式版本";

const KEY_ENABLED: &str = "update_check_enabled";
const KEY_AUTO_INSTALL: &str = "update_auto_install";
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
        auto_install: data_service::get_setting_bool(conn, KEY_AUTO_INSTALL, false),
    }
}

pub fn save_settings(conn: &Connection, settings: &UpdateSettings) -> Result<(), String> {
    data_service::set_setting(conn, KEY_ENABLED, if settings.enabled { "1" } else { "0" })?;
    data_service::set_setting(
        conn,
        KEY_AUTO_INSTALL,
        if settings.auto_install { "1" } else { "0" },
    )
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

    let download_url = manifest.download_url();
    let optional = [
        (KEY_NOTES, manifest.notes.as_deref()),
        (KEY_URL, download_url.as_deref()),
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
/// `user_agent` 会随请求发出，GitHub 等接口要求带 User-Agent。
pub fn fetch_manifest(source: &str, user_agent: &str) -> Result<UpdateManifest, String> {
    let source = source.trim();
    if source.is_empty() {
        return Err("尚未配置更新源地址".to_string());
    }

    let text = if source.starts_with("http://") || source.starts_with("https://") {
        fetch_over_http(source, user_agent)?
    } else {
        fetch_over_file(source)?
    };
    parse_manifest(&text)
}

fn fetch_over_http(url: &str, user_agent: &str) -> Result<String, String> {
    // Windows 走系统 SChannel：必须显式指定 provider，
    // ureq 默认用 rustls，未启用该 feature 时请求 https 会直接 panic。
    let agent = update_install::build_agent(TIMEOUT);

    let response = agent
        .get(url)
        .header("User-Agent", user_agent)
        .header("Accept", "application/json")
        .call();

    let mut response = match response {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(404)) => return Err(ERR_404.to_string()),
        Err(error) => return Err(format!("请求更新源失败: {}", error)),
    };

    // 4xx/5xx 是否转成 Err 取决于 ureq 配置，这里再兜一层
    let status = response.status().as_u16();
    if status == 404 {
        return Err(ERR_404.to_string());
    }
    if !(200..300).contains(&status) {
        return Err(format!("更新源返回 HTTP {}", status));
    }

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
    source_url: &str,
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

    // GitHub 等接口要求请求带 User-Agent
    let user_agent = format!("baibaoxiang/{}", current_version);
    match fetch_manifest(source_url, &user_agent) {
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

// ---- 自动下载并安装 ----

/// 报告一个进度阶段。
fn report(progress: &mut dyn FnMut(UpdateProgress), stage: &str, downloaded: u64, total: u64, message: Option<String>) {
    progress(UpdateProgress {
        stage: stage.to_string(),
        downloaded,
        total,
        message,
    });
}

/// 自动下载并安装更新。
///
/// 流程：重新拉取清单 → 匹配本机平台资产 → 下载（带进度）→ 验签 →
/// 交给安装器（`.msi`/`.exe`）或直接原地替换（便携版）。
///
/// 安全策略：配置了公钥时，**必须**有签名且验签通过才允许安装。
pub fn install_update(
    _conn: &Connection,
    current_version: &str,
    source_url: &str,
    pubkey: &str,
    progress: &mut dyn FnMut(UpdateProgress),
) -> Result<UpdateInstallResult, String> {
    let user_agent = format!("baibaoxiang/{}", current_version);
    let platform = update_install::platform_key();

    report(progress, "preparing", 0, 0, Some("正在获取更新清单…".to_string()));
    let manifest = fetch_manifest(source_url, &user_agent)?;
    let asset = update_install::resolve_asset(&manifest, &platform).ok_or_else(|| {
        format!(
            "更新清单中没有适用于当前平台（{}）的安装包，请到发布页手动下载",
            platform
        )
    })?;

    let file_name = update_install::filename_from_url(&asset.url).unwrap_or_else(|| {
        format!("baibaoxiang-{}.bin", manifest.version.trim().trim_start_matches(['v', 'V']))
    });
    let dest = update_install::update_dir().join(&file_name);

    // 1) 下载
    report(
        progress,
        "downloading",
        0,
        0,
        Some(format!("正在下载 {}", file_name)),
    );
    let mut last_done = 0u64;
    let mut last_total = 0u64;
    update_install::download(&asset.url, &dest, &user_agent, &mut |done, total| {
        last_done = done;
        last_total = total.unwrap_or(0);
        report(progress, "downloading", done, last_total, None);
    })?;
    report(progress, "downloading", last_done, last_total, None);

    // 2) 验签（配置了公钥就强制校验）
    let pubkey = pubkey.trim();
    if !pubkey.is_empty() {
        report(progress, "verifying", last_done, last_total, Some("正在校验更新包签名…".to_string()));
        let signature = asset.signature.as_deref().ok_or_else(|| {
            "更新清单没有提供签名，出于安全考虑已拒绝自动安装（可在设置中清空公钥或改用带签名的 latest.json）"
                .to_string()
        })?;
        let bytes = std::fs::read(&dest).map_err(|e| format!("读取更新包失败: {}", e))?;
        if !update_install::verify_signature(pubkey, &bytes, signature)? {
            return Err("更新包签名校验失败，文件可能已被篡改，已拒绝安装".to_string());
        }
    }

    // 3) 安装
    report(progress, "installing", last_done, last_total, Some("正在启动安装…".to_string()));
    let current_exe = std::env::current_exe().map_err(|e| format!("无法获取当前程序路径: {}", e))?;
    let plan = update_install::installer_plan(&dest, &current_exe)?;

    let result = match plan.kind {
        update_install::InstallKind::Portable => {
            update_install::replace_portable(&current_exe, &dest)?;
            UpdateInstallResult {
                installed: true,
                file_path: dest.to_string_lossy().to_string(),
                message: format!(
                    "新版本 v{} 已就位，重启应用后生效",
                    manifest.version.trim()
                ),
                need_restart: true,
                installer_started: false,
            }
        }
        _ => {
            update_install::run_installer(&plan)?;
            UpdateInstallResult {
                installed: true,
                file_path: dest.to_string_lossy().to_string(),
                message: format!(
                    "已启动安装程序，应用即将退出并完成更新到 v{}",
                    manifest.version.trim()
                ),
                need_restart: false,
                installer_started: true,
            }
        }
    };

    report(progress, "done", last_done, last_total, Some(result.message.clone()));
    Ok(result)
}

/// 更新包下载目录（供命令行/日志使用）。
pub fn download_dir() -> PathBuf {
    update_install::update_dir()
}

fn build_result(
    status: &str,
    _settings: &UpdateSettings,
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
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_schema;
    use base64::Engine as _;

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

    /// GitHub Releases 接口返回的形状：tag_name 当版本、body 当说明、assets 当下载地址。
    #[test]
    fn parses_github_release_payload() {
        // body 里的 "## 修复" 含 `"##`，用 r###"..."### 避免提前结束原始字符串
        let json = r###"{
            "html_url": "https://github.com/WeedFire/baibaoxiang/releases/tag/v1.0.2",
            "tag_name": "v1.0.2",
            "body": "## 修复\n- 启动更快",
            "draft": false,
            "prerelease": false,
            "assets": [
                { "name": "百宝箱_1.0.2_x64_zh-CN.msi",
                  "browser_download_url": "https://github.com/WeedFire/baibaoxiang/releases/download/v1.0.2/a.msi" }
            ]
        }"###;
        let manifest = parse_manifest(json).unwrap();
        assert_eq!(manifest.version, "v1.0.2");
        assert!(manifest.notes.as_deref().unwrap().contains("启动更快"));
        // 有发布附件时优先附件，而不是 release 页面
        assert_eq!(
            manifest.download_url().as_deref(),
            Some("https://github.com/WeedFire/baibaoxiang/releases/download/v1.0.2/a.msi")
        );
        assert!(is_newer(&manifest.version, "1.0.1"));
    }

    #[test]
    fn falls_back_to_release_page_without_assets() {
        let json = r#"{ "tag_name": "v1.0.2", "html_url": "https://github.com/x/y/releases/tag/v1.0.2", "assets": [] }"#;
        let manifest = parse_manifest(json).unwrap();
        assert_eq!(
            manifest.download_url().as_deref(),
            Some("https://github.com/x/y/releases/tag/v1.0.2")
        );
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
        let manifest = fetch_manifest(&source, "baibaoxiang/test").unwrap();
        assert_eq!(manifest.version, "9.9.9");
        assert_eq!(manifest.notes.as_deref(), Some("来自网络"));
        let _ = server.join();
    }

    /// 统一保存设置：只涉及开关。
    fn enable_check(conn: &Connection, enabled: bool) {
        save_settings(
            conn,
            &UpdateSettings {
                enabled,
                auto_install: false,
            },
        )
        .unwrap();
    }

    #[test]
    fn reports_update_available_from_local_source() {
        let conn = db();
        enable_check(&conn, true);
        let (_dir, source) = manifest_file(
            r#"{ "version": "1.0.2", "notes": "修复若干问题", "url": "https://example.com/a.exe" }"#,
        );

        let result = check_update(&conn, "1.0.1", false, 1_000, &source);
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
        enable_check(&conn, true);
        let (_dir, source) = manifest_file(r#"{ "version": "1.0.1" }"#);

        let result = check_update(&conn, "1.0.1", false, 1_000, &source);
        assert!(!result.has_update);
        assert_eq!(result.status, "ok");
    }

    /// 距上次检查不到间隔时直接用缓存，连无效更新源都不会去读。
    #[test]
    fn reuses_cache_within_interval() {
        let conn = db();
        enable_check(&conn, true);
        let (_dir, source) = manifest_file(r#"{ "version": "1.0.2" }"#);
        assert!(check_update(&conn, "1.0.1", false, 1_000, &source).has_update);

        // 缓存有效期内换成不存在的源，也不应触发读取
        let bad_source = "Z:/__no_such_dir__/update.json".to_string();
        let cached = check_update(&conn, "1.0.1", false, 2_000, &bad_source);
        assert_eq!(cached.status, "ok");
        assert!(cached.from_cache);
        assert!(cached.has_update);

        // 强制检查才会真正去读（此时必然失败，但历史结果仍会带上）
        let forced = check_update(&conn, "1.0.1", true, 2_000, &bad_source);
        assert_eq!(forced.status, "error");
        assert!(forced.has_update);
        assert!(forced.message.is_some());
    }

    #[test]
    fn interval_expiry_triggers_refresh() {
        let conn = db();
        enable_check(&conn, true);
        let (_dir, source) = manifest_file(r#"{ "version": "1.0.2" }"#);
        assert!(check_update(&conn, "1.0.1", false, 1_000, &source).has_update);

        let later = 1_000 + CHECK_INTERVAL_SECS + 1;
        let refreshed = check_update(&conn, "1.0.1", false, later, &source);
        assert!(!refreshed.from_cache, "超过间隔后应重新读取更新源");
        assert_eq!(refreshed.checked_at, Some(later));
    }

    #[test]
    fn ignored_version_hides_the_update() {
        let conn = db();
        enable_check(&conn, true);
        let (_dir, source) = manifest_file(r#"{ "version": "1.0.2" }"#);

        set_ignored_version(&conn, Some("1.0.2")).unwrap();
        let result = check_update(&conn, "1.0.1", true, 1_000, &source);
        assert!(result.ignored);
        assert!(!result.has_update);

        set_ignored_version(&conn, None).unwrap();
        assert!(check_update(&conn, "1.0.1", true, 1_000, &source).has_update);
    }

    #[test]
    fn disabled_check_skips_network() {
        let conn = db();
        enable_check(&conn, false);

        let result = check_update(&conn, "1.0.1", false, 1_000, SOURCE_URL);
        assert_eq!(result.status, "disabled");
        assert!(!result.has_update);
    }

    /// 更新源与公钥写死在代码里，不再依赖用户配置。
    #[test]
    fn update_source_and_pubkey_are_hardcoded() {
        assert!(SOURCE_URL.ends_with("latest.json"), "{}", SOURCE_URL);
        assert!(SOURCE_URL.starts_with("https://"), "{}", SOURCE_URL);
        // 公钥常量存在（发布前应填真实值；为空表示不验签）
        let _ = PUBLIC_KEY;

        let conn = db();
        let settings = load_settings(&conn);
        assert!(settings.enabled, "默认应开启自动检查");
        assert!(!settings.auto_install, "默认不自动安装");

        let state = load_state(&conn, "1.0.1");
        assert_eq!(state.current_version, "1.0.1");
        assert!(state.settings.enabled);
    }

    // ---- 自动下载安装 ----

    /// 确定性密钥对（固定种子），测试不依赖随机数生成器。
    fn keypair() -> (String, ed25519_dalek::SigningKey) {
        use ed25519_dalek::SigningKey;
        let signing = SigningKey::from_bytes(&[42u8; 32]);
        let pubkey = base64::engine::general_purpose::STANDARD
            .encode(signing.verifying_key().to_bytes());
        (pubkey, signing)
    }

    fn sign(signing: &ed25519_dalek::SigningKey, data: &[u8]) -> String {
        use ed25519_dalek::Signer;
        base64::engine::general_purpose::STANDARD.encode(signing.sign(data).to_bytes())
    }

    /// 起一个一次性本地 HTTP 服务，返回端口与句柄。
    fn serve_bytes(body: Vec<u8>) -> (u16, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            }
        });
        (port, handle)
    }

    /// 构造「本机平台 → 指定包地址」的 latest.json 清单文件。
    fn platform_source(
        dir: &tempfile::TempDir,
        package_name: &str,
        package_url: &str,
        signature: Option<&str>,
    ) -> String {
        let mut asset = serde_json::json!({ "url": package_url });
        if let Some(sig) = signature {
            asset["signature"] = serde_json::Value::String(sig.to_string());
        }
        let manifest = serde_json::json!({
            "version": "9.9.9",
            "notes": "测试更新",
            "platforms": { update_install::platform_key(): asset }
        });
        let path = dir.path().join(format!("latest-{}.json", package_name));
        std::fs::write(&path, manifest.to_string()).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    fn settings_roundtrip() {
        let conn = db();
        let settings = UpdateSettings {
            enabled: false,
            auto_install: true,
        };
        save_settings(&conn, &settings).unwrap();

        let loaded = load_settings(&conn);
        assert!(!loaded.enabled);
        assert!(loaded.auto_install);

        // 默认值：自动检查开启、自动安装关闭
        let empty = db();
        assert!(load_settings(&empty).enabled);
        assert!(!load_settings(&empty).auto_install);
    }

    /// 完整链路：拉清单 → 下载 → 验签 → 由于包格式不支持而明确报错。
    #[test]
    fn install_downloads_and_verifies_before_installation() {
        let conn = db();
        let payload: Vec<u8> = (0..64u8).collect();
        let (port, server) = serve_bytes(payload.clone());
        let (pubkey, signing) = keypair();
        let dir = tempfile::tempdir().unwrap();
        let source = platform_source(
            &dir,
            "update.zip",
            &format!("http://127.0.0.1:{}/update.zip", port),
            Some(&sign(&signing, &payload)),
        );

        let mut stages: Vec<String> = Vec::new();
        let error = install_update(&conn, "1.0.0", &source, &pubkey, &mut |p| {
            stages.push(p.stage)
        })
        .unwrap_err();

        assert!(error.contains("暂不支持自动安装"), "错误应说明包格式不支持：{}", error);
        assert!(stages.contains(&"downloading".to_string()), "阶段：{:?}", stages);
        assert!(stages.contains(&"verifying".to_string()), "阶段：{:?}", stages);

        let _ = std::fs::remove_file(download_dir().join("update.zip"));
        let _ = server.join();
    }

    #[test]
    fn install_rejects_tampered_signature() {
        let conn = db();
        let payload = b"real payload".to_vec();
        let (port, server) = serve_bytes(payload.clone());
        let (pubkey, signing) = keypair();
        let dir = tempfile::tempdir().unwrap();
        // 用另一段数据的签名冒充
        let source = platform_source(
            &dir,
            "tampered.zip",
            &format!("http://127.0.0.1:{}/tampered.zip", port),
            Some(&sign(&signing, b"another payload")),
        );

        let error = install_update(&conn, "1.0.0", &source, &pubkey, &mut |_| {}).unwrap_err();
        assert!(error.contains("签名校验失败"), "{}", error);
        let _ = std::fs::remove_file(download_dir().join("tampered.zip"));
        let _ = server.join();
    }

    #[test]
    fn install_refuses_unsigned_package_when_pubkey_configured() {
        let conn = db();
        let payload = b"unsigned".to_vec();
        let (port, server) = serve_bytes(payload.clone());
        let (pubkey, _) = keypair();
        let dir = tempfile::tempdir().unwrap();
        let source = platform_source(
            &dir,
            "unsigned.zip",
            &format!("http://127.0.0.1:{}/unsigned.zip", port),
            None,
        );

        let error = install_update(&conn, "1.0.0", &source, &pubkey, &mut |_| {}).unwrap_err();
        assert!(error.contains("没有提供签名"), "{}", error);
        let _ = std::fs::remove_file(download_dir().join("unsigned.zip"));
        let _ = server.join();
    }

    #[test]
    fn install_reports_when_no_asset_for_current_platform() {
        let conn = db();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("latest.json");
        std::fs::write(
            &path,
            r#"{ "version": "9.9.9", "platforms": { "plan9-sparc": { "url": "https://example.com/x" } } }"#,
        )
        .unwrap();
        let source = path.to_string_lossy().to_string();

        let error = install_update(&conn, "1.0.0", &source, "", &mut |_| {}).unwrap_err();
        assert!(error.contains("当前平台"), "{}", error);
        assert!(error.contains(&update_install::platform_key()), "{}", error);
    }

    /// 手动验证真实更新源：`cargo test --lib -- --ignored --nocapture`
    /// 仓库还没有发布版本时应得到 404 提示，发布后应解析出版本号与下载地址。
    #[test]
    #[ignore]
    fn live_default_source() {
        let result = fetch_manifest(SOURCE_URL, "baibaoxiang/test");
        match result {
            Ok(manifest) => {
                println!("version={} url={:?}", manifest.version, manifest.download_url());
                println!("notes={:?}", manifest.notes);
            }
            Err(err) => println!("error={}", err),
        }
    }
}
