//! 插件市场：拉取清单 → 下载归档 → 验签 → 解压到对应目录 →（脚本类）自动登记应用。
//!
//! 三种插件的目标目录（以「程序运行目录」= 安装目录为根）：
//! - `python_script` → `<根>/pyTools/<id>/`，并自动登记为应用（进默认分组）
//! - `program`       → `<根>/<id>/`，仅放置文件（python 运行环境的一部分）
//! - `dependency`    → `<根>/python/Lib/site-packages/`，仅放置文件
//!
//! 解压复用 Windows 自带工具（PowerShell `Expand-Archive` / `tar.exe`），
//! 不引入额外 crate，保证离线构建可用。

use crate::db;
use crate::models::{
    AddAppRequest, MarketplaceInstallResult, MarketplaceManifest, MarketplacePlugin,
    MarketplaceProgress, PluginKind,
};
use crate::services::{data_service, icon_service, update_install, update_service};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

/// 兜底清单地址：固定 tag `plugins` 的 Release 资产（写死在代码里，不暴露给用户）。
/// 正常情况优先走 [`MARKETPLACE_LATEST_API`] 自动跟随最新 Release，只有它失败才回退到这里。
pub const MARKETPLACE_URL: &str =
    "https://gitee.com/wjqnxw/baibaoxiang-plugins/releases/download/plugins/marketplace.json";

/// Gitee「最新 Release」API（仓库 `wjqnxw/baibaoxiang-plugins`，独立于 App 更新源 GitHub）。
/// Gitee 没有 `/releases/latest/download/` 别名，该接口返回最新 Release 的 `tag_name` 与 `assets`，
/// 据此定位 `marketplace.json`，从而无需把 tag 写死在代码里。
pub const MARKETPLACE_LATEST_API: &str =
    "https://gitee.com/api/v5/repos/wjqnxw/baibaoxiang-plugins/releases/latest";

/// 清单在 Release 资产中的文件名。
const MANIFEST_ASSET_NAME: &str = "marketplace.json";
const USER_AGENT: &str = "baibaoxiang/marketplace";

/// 上报一个进度阶段（封装构造，避免调用处重复样板）。
fn report(
    progress: &mut dyn FnMut(MarketplaceProgress),
    stage: &str,
    downloaded: u64,
    total: u64,
    message: Option<String>,
) {
    progress(MarketplaceProgress {
        stage: stage.to_string(),
        downloaded,
        total,
        message,
    });
}

/// 插件种类的字符串形式（`marketplace.json` 里使用的写法）。
pub fn kind_to_str(kind: PluginKind) -> &'static str {
    match kind {
        PluginKind::PythonScript => "python_script",
        PluginKind::Program => "program",
        PluginKind::Dependency => "dependency",
    }
}

/// 是否应自动登记为应用：缺省时仅 Python 脚本自动添加。
pub fn should_auto_add(plugin: &MarketplacePlugin) -> bool {
    plugin
        .auto_add
        .unwrap_or_else(|| matches!(plugin.kind, PluginKind::PythonScript))
}

/// 某类插件的目标父目录（不含 `<id>` 子目录）。
pub fn target_dir_with(base: &Path, kind: PluginKind) -> PathBuf {
    match kind {
        PluginKind::PythonScript => base.join("pyTools"),
        PluginKind::Program => base.to_path_buf(),
        PluginKind::Dependency => base.join("python").join("Lib").join("site-packages"),
    }
}

/// 某插件实际安装目录：程序包/脚本进 `<id>` 子目录，依赖包直接进 site-packages。
pub fn install_dir_for_with(base: &Path, plugin: &MarketplacePlugin) -> PathBuf {
    let target = target_dir_with(base, plugin.kind);
    match plugin.kind {
        PluginKind::Dependency => target,
        _ => target.join(&plugin.id),
    }
}

/// 拉取并解析插件市场清单（HTTP/本地文件均可，便于测试用本地文件）。
pub fn fetch_manifest(source: &str) -> Result<MarketplaceManifest, String> {
    let source = source.trim();
    if source.is_empty() {
        return Err("尚未配置插件市场源".to_string());
    }
    let text = if source.starts_with("http://") || source.starts_with("https://") {
        let agent = update_install::build_agent(std::time::Duration::from_secs(30));
        let mut resp = agent
            .get(source)
            .call()
            .map_err(|e| format!("获取插件市场清单失败: {}", e))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(format!("插件市场清单返回 HTTP {}", status));
        }
        resp.body_mut()
            .read_to_string()
            .map_err(|e| format!("读取插件市场清单失败: {}", e))?
    } else {
        // 相对路径以「程序运行目录」为根，便于放到安装目录里随包分发
        let path = crate::utils::resolve_path(source);
        std::fs::read_to_string(&path)
            .map_err(|e| format!("读取插件市场清单失败（{}）: {}", path.display(), e))?
    };
    parse_manifest(&text)
}

/// 解析清单 JSON（容忍 BOM）。
pub fn parse_manifest(text: &str) -> Result<MarketplaceManifest, String> {
    let text = text.trim_start_matches('\u{feff}').trim();
    if text.is_empty() {
        return Err("插件市场清单内容为空".to_string());
    }
    let manifest: MarketplaceManifest =
        serde_json::from_str(text).map_err(|e| format!("插件市场清单格式无效: {}", e))?;
    if manifest.plugins.is_empty() {
        return Err("插件市场清单中没有可用插件".to_string());
    }
    Ok(manifest)
}

// ---------------- 自动跟随最新 Release ----------------

/// Gitee Release JSON（只取需要的字段）。
#[derive(serde::Deserialize)]
struct GiteeRelease {
    #[serde(default)]
    tag_name: Option<String>,
    #[serde(default)]
    assets: Vec<GiteeAsset>,
}

#[derive(serde::Deserialize)]
struct GiteeAsset {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    browser_download_url: Option<String>,
}

/// 从 Gitee `releases/latest` 响应里找出 `marketplace.json` 资产，返回 `(下载地址, tag)`。
pub fn manifest_asset_from_release(text: &str) -> Result<(String, String), String> {
    let text = text.trim_start_matches('\u{feff}').trim();
    let release: GiteeRelease =
        serde_json::from_str(text).map_err(|e| format!("解析 Gitee Release 失败: {}", e))?;
    let tag = release.tag_name.unwrap_or_default();
    let url = release
        .assets
        .iter()
        .find(|a| a.name.as_deref() == Some(MANIFEST_ASSET_NAME))
        .and_then(|a| a.browser_download_url.clone())
        .ok_or_else(|| format!("最新 Release 中缺少 {} 资产", MANIFEST_ASSET_NAME))?;
    Ok((url, tag))
}

/// 把下载地址里的 Release tag 换成 `tag`：
/// `.../releases/download/<old-tag>/<file>` → `.../releases/download/<tag>/<file>`。
/// 这样清单里即便写着别的 tag（例如打包时的默认值），也能对齐到实际发布的最新 Release。
pub fn retag_download_url(url: &str, tag: &str) -> String {
    let tag = tag.trim();
    if tag.is_empty() {
        return url.to_string();
    }
    const MARK: &str = "/releases/download/";
    let Some(idx) = url.find(MARK) else {
        return url.to_string();
    };
    let head_end = idx + MARK.len();
    let rest = &url[head_end..];
    let Some(slash) = rest.find('/') else {
        return url.to_string();
    };
    format!("{}{}/{}", &url[..head_end], tag, &rest[slash + 1..])
}

/// 用实际 tag 重写清单里每个插件的下载地址。
pub fn retag_manifest(manifest: &mut MarketplaceManifest, tag: &str) {
    for plugin in manifest.plugins.iter_mut() {
        plugin.download_url = retag_download_url(&plugin.download_url, tag);
    }
}

/// GET 一个 URL 并返回正文（插件市场内部用）。
fn http_get_text(url: &str) -> Result<String, String> {
    let agent = update_install::build_agent(std::time::Duration::from_secs(30));
    let mut resp = agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("请求失败: {}", e))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("服务器返回 HTTP {}", status));
    }
    resp.body_mut()
        .read_to_string()
        .map_err(|e| format!("读取响应失败: {}", e))
}

/// 拉取清单（生产入口）：自动跟随 Gitee 最新 Release。
pub fn fetch_manifest_auto() -> Result<MarketplaceManifest, String> {
    fetch_manifest_auto_from(MARKETPLACE_LATEST_API, MARKETPLACE_URL)
}

/// [`fetch_manifest_auto`] 的可注入实现（URL 可替换，便于测试）。
/// 优先走「最新 Release」API 定位 `marketplace.json`，并把各插件下载地址对齐到该 Release 的 tag；
/// 任何一步失败都回退到 `fallback_url`（固定 tag 地址）。
pub fn fetch_manifest_auto_from(
    api_url: &str,
    fallback_url: &str,
) -> Result<MarketplaceManifest, String> {
    let latest = http_get_text(api_url)
        .and_then(|text| manifest_asset_from_release(&text))
        .and_then(|(url, tag)| {
            fetch_manifest(&url).map(|mut manifest| {
                retag_manifest(&mut manifest, &tag);
                manifest
            })
        });
    match latest {
        Ok(manifest) => Ok(manifest),
        Err(e) => fetch_manifest(fallback_url)
            .map_err(|fallback| format!("{}（回退固定地址亦失败：{}）", e, fallback)),
    }
}

/// 下载 →（验签）→ 解压，返回实际安装目录。
///
/// `pubkey` 非空时强制要求插件带签名且验签通过；为空则跳过验签。
pub fn download_and_extract(
    base: &Path,
    plugin: &MarketplacePlugin,
    pubkey: &str,
    progress: &mut dyn FnMut(MarketplaceProgress),
) -> Result<PathBuf, String> {
    let temp_dir = update_install::update_dir().join("marketplace");
    std::fs::create_dir_all(&temp_dir).map_err(|e| format!("创建临时目录失败: {}", e))?;
    let file_name = update_install::filename_from_url(&plugin.download_url)
        .unwrap_or_else(|| format!("{}.zip", plugin.id));
    let archive = temp_dir.join(&file_name);

    report(progress, "downloading", 0, 0, Some("正在下载插件".to_string()));
    let written = update_install::download(
        &plugin.download_url,
        &archive,
        USER_AGENT,
        &mut |d, t| report(progress, "downloading", d, t.unwrap_or(0), None),
    )?;
    report(progress, "downloading", written, written, None);

    let pubkey = pubkey.trim();
    if !pubkey.is_empty() {
        report(
            progress,
            "verifying",
            written,
            written,
            Some("正在校验签名".to_string()),
        );
        let signature = plugin.signature.as_deref().filter(|s| !s.trim().is_empty());
        let Some(signature) = signature else {
            return Err("插件没有提供签名，出于安全考虑已拒绝安装".to_string());
        };
        let data = std::fs::read(&archive).map_err(|e| format!("读取插件文件失败: {}", e))?;
        match update_install::verify_signature(pubkey, &data, signature.trim()) {
            Ok(true) => {}
            Ok(false) => return Err("插件签名校验失败，文件可能已被篡改".to_string()),
            Err(e) => return Err(format!("插件签名校验出错: {}", e)),
        }
    }

    report(
        progress,
        "installing",
        written,
        written,
        Some("正在解压插件".to_string()),
    );
    let install_dir = install_dir_for_with(base, plugin);
    extract_archive(&archive, &install_dir)?;
    Ok(install_dir)
}

/// 完整安装一个插件：下载解压 +（脚本类）自动登记应用 + 写安装标记。
pub fn install_plugin(
    app: &AppHandle,
    base: &Path,
    plugin: &MarketplacePlugin,
    progress: &mut dyn FnMut(MarketplaceProgress),
) -> Result<MarketplaceInstallResult, String> {
    let install_dir = download_and_extract(base, plugin, update_service::PUBLIC_KEY, progress)?;

    let mut added_app = false;
    let mut app_id = None;
    if should_auto_add(plugin) {
        let conn = db::get_connection(app)?;
        let group_id = data_service::get_default_group_id(&conn)?;
        let icon = download_plugin_icon(app, plugin)?;
        let req = build_add_app_request(plugin, base, &install_dir, &group_id, icon)?;
        let created = data_service::add_app(&conn, &req)?;
        app_id = Some(created.id.clone());
        added_app = true;
    }

    write_marker(base, plugin, &install_dir.to_string_lossy(), app_id.as_deref())?;

    report(progress, "done", 0, 0, Some("安装完成".to_string()));
    Ok(MarketplaceInstallResult {
        installed: true,
        added_app,
        app_id,
        install_dir: install_dir.to_string_lossy().to_string(),
        message: "插件安装完成".to_string(),
    })
}

/// 构造自动添加应用所需的请求（路径按程序根目录转成相对写法）。
pub fn build_add_app_request(
    plugin: &MarketplacePlugin,
    base: &Path,
    install_dir: &Path,
    group_id: &str,
    icon_path: Option<String>,
) -> Result<AddAppRequest, String> {
    let entry = plugin.entry.trim();
    if entry.is_empty() {
        return Err("该插件需要自动添加应用，但未指定入口文件 entry".to_string());
    }
    let abs_entry = install_dir.join(entry);
    let rel_entry = abs_entry
        .strip_prefix(base)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| entry.to_string());

    let is_python = plugin.launch_kind == 1;
    let interpreter = if is_python && !plugin.interpreter.trim().is_empty() {
        Some(plugin.interpreter.trim().to_string())
    } else {
        None
    };

    // 不调用 refresh_app_icon：避免取到的解释器图标（蛇）覆盖；图标留空时前端回退 Python 标志
    Ok(AddAppRequest {
        group_id: group_id.to_string(),
        name: plugin.name.trim().to_string(),
        executable_path: rel_entry,
        arguments: None,
        working_directory: None,
        startup_window_style: 0,
        launch_kind: plugin.launch_kind,
        is_python_script: is_python,
        python_interpreter_path: interpreter,
        show_console: false,
        run_as_admin: false,
        allow_multiple_instances: true,
        icon_path,
    })
}

/// 下载插件图标（仅接受 PNG），失败返回 None（不阻断安装）。
fn download_plugin_icon(app: &AppHandle, plugin: &MarketplacePlugin) -> Result<Option<String>, String> {
    let Some(url) = plugin.icon_url.as_ref().filter(|u| !u.trim().is_empty()) else {
        return Ok(None);
    };
    let temp_dir = update_install::update_dir().join("marketplace");
    let file_name = update_install::filename_from_url(url).unwrap_or_else(|| "icon.png".to_string());
    let dest = temp_dir.join(&file_name);
    update_install::download(url, &dest, USER_AGENT, &mut |_, _| {}).ok();
    let Ok(bytes) = std::fs::read(&dest) else {
        return Ok(None);
    };
    if !bytes.starts_with(b"\x89PNG") {
        return Ok(None);
    }
    let stored = icon_service::store_icon_bytes(app, None, &bytes)?;
    Ok(Some(stored))
}

// ---------------- 安装标记（用于「已安装」状态展示） ----------------

fn marker_path(base: &Path, id: &str) -> PathBuf {
    base.join(".marketplace").join(format!("{}.json", id))
}

/// 写安装标记：记录版本、种类、安装目录、登记的应用 id。
pub fn write_marker(
    base: &Path,
    plugin: &MarketplacePlugin,
    install_dir: &str,
    app_id: Option<&str>,
) -> Result<(), String> {
    let dir = base.join(".marketplace");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建标记目录失败: {}", e))?;
    let marker = serde_json::json!({
        "id": plugin.id,
        "version": plugin.version,
        "kind": kind_to_str(plugin.kind),
        "install_dir": install_dir,
        "app_id": app_id,
    });
    let path = marker_path(base, &plugin.id);
    std::fs::write(&path, serde_json::to_string_pretty(&marker).unwrap())
        .map_err(|e| format!("写入安装标记失败: {}", e))?;
    Ok(())
}

/// 读取单个插件的安装标记。
pub fn read_marker(base: &Path, id: &str) -> Option<PluginMarker> {
    let path = marker_path(base, id);
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(PluginMarker {
        id: id.to_string(),
        version: v.get("version").and_then(|x| x.as_str()).map(|s| s.to_string()),
        kind: v
            .get("kind")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        install_dir: v
            .get("install_dir")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        app_id: v.get("app_id").and_then(|x| x.as_str()).map(|s| s.to_string()),
    })
}

/// 读取全部安装标记（命令构建插件视图时用）。
pub fn read_all_markers(base: &Path) -> HashMap<String, PluginMarker> {
    let mut map = HashMap::new();
    let dir = base.join(".marketplace");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return map;
    };
    for entry in entries.flatten() {
        if entry.path().extension().and_then(|x| x.to_str()) == Some("json") {
            if let Some(name) = entry.path().file_stem().and_then(|x| x.to_str()) {
                if let Some(info) = read_marker(base, name) {
                    map.insert(name.to_string(), info);
                }
            }
        }
    }
    map
}

/// 单个插件的安装标记。
#[derive(Debug, Clone, Serialize)]
pub struct PluginMarker {
    pub id: String,
    pub version: Option<String>,
    pub kind: String,
    pub install_dir: String,
    pub app_id: Option<String>,
}

// ---------------- 归档解压（Windows 内置工具） ----------------

/// 把归档解压到 `dest`，若归档内只有单个顶层目录则去掉该层（避免双重嵌套）。
pub fn extract_archive(archive: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("创建解压目录失败: {}", e))?;
    let name = archive.to_string_lossy().to_ascii_lowercase();
    let is_tar_gz = name.ends_with(".tar.gz") || name.ends_with(".tgz");
    let is_zip = name.ends_with(".zip") || name.ends_with(".whl");

    if is_zip {
        extract_zip(archive, dest)?;
    } else if is_tar_gz {
        extract_targz(archive, dest)?;
    } else {
        return Err(format!("不支持的归档格式: {}", archive.display()));
    }

    flatten_single_top_dir(dest)?;
    Ok(())
}

/// 若解压目录下恰好只有一个顶层目录，则把其内容提到上层、删除该空目录。
fn flatten_single_top_dir(dir: &Path) -> Result<(), String> {
    let mut entries: Vec<PathBuf> = Vec::new();
    for e in std::fs::read_dir(dir).map_err(|e| format!("读取解压目录失败: {}", e))?.flatten() {
        entries.push(e.path());
    }
    if entries.len() == 1 && entries[0].is_dir() {
        let top = &entries[0];
        for item in std::fs::read_dir(top)
            .map_err(|e| format!("读取顶层目录失败: {}", e))?
            .flatten()
        {
            let from = item.path();
            let to = dir.join(item.file_name());
            std::fs::rename(&from, &to).map_err(|e| format!("移动文件失败: {}", e))?;
        }
        let _ = std::fs::remove_dir(top);
    }
    Ok(())
}

/// 用 `zip` crate 解压 zip / whl（比 PowerShell Expand-Archive 更稳，跨格式）。
fn extract_zip(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| format!("打开归档失败: {}", e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("归档无法识别: {}", e))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取归档项失败: {}", e))?;
        let Some(name) = entry.enclosed_name() else {
            continue; // 跳过不安全路径（含 `..` 或绝对路径）
        };
        let out_path = dest.join(&name);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| format!("创建目录失败: {}", e))?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
            }
            let mut out =
                std::fs::File::create(&out_path).map_err(|e| format!("写出文件失败: {}", e))?;
            std::io::copy(&mut entry, &mut out).map_err(|e| format!("写出文件失败: {}", e))?;
        }
    }
    Ok(())
}

/// 用 Windows 自带 `tar.exe` 解压 .tar.gz / .tgz。
fn extract_targz(archive: &Path, dest: &Path) -> Result<(), String> {
    let status = std::process::Command::new("tar.exe")
        .args([
            "-xzf",
            &archive.to_string_lossy(),
            "-C",
            &dest.to_string_lossy(),
        ])
        .status()
        .map_err(|e| format!("启动 tar 失败: {}", e))?;
    if !status.success() {
        return Err(format!("解压 tar.gz 失败（退出码 {:?}）", status.code()));
    }
    Ok(())
}

/// 起一个一次性本地 HTTP 服务，返回端口。测试用。
#[cfg(test)]
fn serve_bytes(body: Vec<u8>) -> (u16, std::thread::JoinHandle<()>) {
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

/// 顺序返回多个 JSON 响应体的本地 HTTP 服务；端口先给出，便于把端口写进响应内容（测试用）。
#[cfg(test)]
fn serve_json_sequence(build: impl FnOnce(u16) -> Vec<String>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let bodies = build(port);
    std::thread::spawn(move || {
        for body in bodies {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(body.as_bytes());
                let _ = stream.flush();
            }
        }
    });
    port
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_dir() -> PathBuf {
        // keep() 不自动清理，保证解压目标在整个测试期间可写
        tempfile::tempdir().unwrap().keep()
    }

    fn script_plugin() -> MarketplacePlugin {
        serde_json::from_str(
            r#"{"id":"file_janitor","name":"文件清理助手","kind":"python_script",
               "download_url":"http://127.0.0.1/x.zip","entry":"main.py",
               "launch_kind":1,"interpreter":"python/python.exe"}"#,
        )
        .unwrap()
    }

    fn dependency_plugin() -> MarketplacePlugin {
        serde_json::from_str(
            r#"{"id":"requests_dep","name":"requests","kind":"dependency",
               "download_url":"http://127.0.0.1/x.zip"}"#,
        )
        .unwrap()
    }

    #[test]
    fn kind_serializes_to_snake_case() {
        let json = r#"{"id":"a","name":"A","kind":"dependency","download_url":"http://x/y.zip"}"#;
        let p: MarketplacePlugin = serde_json::from_str(json).unwrap();
        assert_eq!(p.kind, PluginKind::Dependency);
        assert_eq!(kind_to_str(PluginKind::Dependency), "dependency");
        assert_eq!(kind_to_str(PluginKind::PythonScript), "python_script");
    }

    #[test]
    fn targets_dirs_by_kind() {
        let base = base_dir();
        assert_eq!(
            target_dir_with(&base, PluginKind::PythonScript),
            base.join("pyTools")
        );
        assert_eq!(target_dir_with(&base, PluginKind::Program), base);
        assert_eq!(
            target_dir_with(&base, PluginKind::Dependency),
            base.join("python").join("Lib").join("site-packages")
        );
    }

    #[test]
    fn install_dir_nests_id_except_dependency() {
        let base = base_dir();
        let p = script_plugin();
        assert_eq!(
            install_dir_for_with(&base, &p),
            base.join("pyTools").join("file_janitor")
        );
        let d = dependency_plugin();
        assert_eq!(
            install_dir_for_with(&base, &d),
            base.join("python").join("Lib").join("site-packages")
        );
    }

    #[test]
    fn auto_add_defaults_only_for_python_script() {
        assert!(should_auto_add(&script_plugin()));
        assert!(!should_auto_add(&dependency_plugin()));
        // 显式覆盖：依赖包也可要求自动添加
        let mut d = dependency_plugin();
        d.auto_add = Some(true);
        assert!(should_auto_add(&d));
    }

    #[test]
    fn parses_manifest_and_rejects_empty() {
        let text = r#"{"plugins":[
            {"id":"a","name":"A","kind":"python_script","download_url":"http://x/a.zip","entry":"a.py"},
            {"id":"b","name":"B","kind":"dependency","download_url":"http://x/b.zip"}
        ]}"#;
        let m = parse_manifest(text).unwrap();
        assert_eq!(m.plugins.len(), 2);
        assert!(parse_manifest("").is_err());
        assert!(parse_manifest("[]").is_err());
    }

    #[test]
    fn retags_download_url_to_actual_release_tag() {
        let url = "https://gitee.com/wjqnxw/baibaoxiang-plugins/releases/download/plugins/a_1.0.0.zip";
        assert_eq!(
            retag_download_url(url, "v1.0.0"),
            "https://gitee.com/wjqnxw/baibaoxiang-plugins/releases/download/v1.0.0/a_1.0.0.zip"
        );
        // 不含 Release 标记、或 tag 为空时原样返回
        assert_eq!(retag_download_url("http://x/a.zip", "v1"), "http://x/a.zip");
        assert_eq!(retag_download_url(url, "  "), url);
    }

    #[test]
    fn finds_manifest_asset_in_latest_release() {
        let text = r#"{
            "tag_name": "v1.0.0",
            "assets": [
                {"name": "a_1.0.0.zip", "browser_download_url": "https://gitee.com/r/releases/download/v1.0.0/a_1.0.0.zip"},
                {"name": "marketplace.json", "browser_download_url": "https://gitee.com/r/releases/download/v1.0.0/marketplace.json"}
            ]
        }"#;
        let (url, tag) = manifest_asset_from_release(text).unwrap();
        assert_eq!(tag, "v1.0.0");
        assert!(url.ends_with("/v1.0.0/marketplace.json"));
        // 缺少清单资产要报错
        assert!(manifest_asset_from_release(r#"{"tag_name":"v1","assets":[]}"#).is_err());
    }

    #[test]
    fn auto_fetch_follows_latest_release_and_aligns_tag() {
        let manifest = r#"{"plugins":[{"id":"a","name":"A","kind":"python_script","entry":"a.py",
            "download_url":"https://gitee.com/wjqnxw/baibaoxiang-plugins/releases/download/plugins/a_1.0.0.zip"}]}"#
            .to_string();
        let port = serve_json_sequence(|port| {
            vec![
                format!(
                    r#"{{"tag_name":"v1.0.0","assets":[
                    {{"name":"a_1.0.0.zip","browser_download_url":"http://127.0.0.1:{port}/a.zip"}},
                    {{"name":"marketplace.json","browser_download_url":"http://127.0.0.1:{port}/mp.json"}}]}}"#,
                    port = port
                ),
                manifest.clone(),
            ]
        });
        let api = format!("http://127.0.0.1:{}/latest", port);
        let fallback = format!("http://127.0.0.1:{}/fallback.json", port);
        let m = fetch_manifest_auto_from(&api, &fallback).unwrap();
        assert_eq!(m.plugins.len(), 1);
        // 清单里写的是 `plugins`，应被对齐为最新 Release 的 `v1.0.0`
        assert_eq!(
            m.plugins[0].download_url,
            "https://gitee.com/wjqnxw/baibaoxiang-plugins/releases/download/v1.0.0/a_1.0.0.zip"
        );
    }

    #[test]
    fn builds_add_request_with_relative_path_and_interpreter() {
        let base = base_dir();
        let install_dir = base.join("pyTools").join("file_janitor");
        std::fs::create_dir_all(&install_dir).unwrap();
        let req =
            build_add_app_request(&script_plugin(), &base, &install_dir, "default", None).unwrap();
        assert_eq!(req.group_id, "default");
        assert_eq!(req.name, "文件清理助手");
        assert_eq!(req.executable_path, "pyTools/file_janitor/main.py");
        assert_eq!(req.launch_kind, 1);
        assert!(req.is_python_script);
        assert_eq!(req.python_interpreter_path.as_deref(), Some("python/python.exe"));
        assert_eq!(req.icon_path, None);
    }

    #[test]
    fn build_add_request_rejects_missing_entry() {
        let base = base_dir();
        let install_dir = base.join("pyTools").join("x");
        let mut p = script_plugin();
        p.entry = String::new();
        let err = build_add_app_request(&p, &base, &install_dir, "default", None).unwrap_err();
        assert!(err.contains("entry"), "{}", err);
    }

    // ---- 解压（依赖 Windows 内置工具）----

    /// 纯 Rust 写出的「存储型」zip（无压缩），用于构造测试归档，避免依赖外部压缩工具。
    fn write_zip(path: &Path, files: &[(&str, &str)]) {
        let mut out: Vec<u8> = Vec::new();
        let mut central: Vec<u8> = Vec::new();
        let mut offset: u32 = 0;
        for (name, data) in files {
            let name_bytes = name.as_bytes();
            let crc = crc32(data.as_bytes());
            let size = data.len() as u32;

            let local_header = build_local_header(name_bytes, crc, size);
            out.extend_from_slice(&local_header);
            out.extend_from_slice(name_bytes);
            out.extend_from_slice(data.as_bytes());

            let local_offset = offset;
            let cd_header = build_central_header(name_bytes, crc, size, local_offset);
            central.extend_from_slice(&cd_header);
            central.extend_from_slice(name_bytes);

            offset += local_header.len() as u32 + name_bytes.len() as u32 + size;
        }
        let cd_offset = offset;
        let cd_size = central.len() as u32;
        let n = files.len() as u16;

        out.extend_from_slice(&central);
        // End of central directory record
        out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&[0, 0]);

        std::fs::write(path, &out).unwrap();
    }

    fn build_local_header(name: &[u8], crc: u32, size: u32) -> Vec<u8> {
        let mut h = Vec::new();
        h.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]); // local file header sig
        h.extend_from_slice(&[20, 0]); // version needed
        h.extend_from_slice(&[0, 0]); // flags
        h.extend_from_slice(&[0, 0]); // method = store
        h.extend_from_slice(&[0, 0, 0, 0]); // mod time/date
        h.extend_from_slice(&crc.to_le_bytes());
        h.extend_from_slice(&size.to_le_bytes()); // compressed
        h.extend_from_slice(&size.to_le_bytes()); // uncompressed
        h.extend_from_slice(&(name.len() as u16).to_le_bytes());
        h.extend_from_slice(&[0, 0]); // extra len
        h
    }

    fn build_central_header(name: &[u8], crc: u32, size: u32, local_offset: u32) -> Vec<u8> {
        let mut h = Vec::new();
        h.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]); // central dir sig
        h.extend_from_slice(&[20, 0]); // version made by
        h.extend_from_slice(&[20, 0]); // version needed
        h.extend_from_slice(&[0, 0]); // flags
        h.extend_from_slice(&[0, 0]); // method = store
        h.extend_from_slice(&[0, 0, 0, 0]); // mod time/date
        h.extend_from_slice(&crc.to_le_bytes());
        h.extend_from_slice(&size.to_le_bytes());
        h.extend_from_slice(&size.to_le_bytes());
        h.extend_from_slice(&(name.len() as u16).to_le_bytes());
        h.extend_from_slice(&[0, 0]); // extra len
        h.extend_from_slice(&[0, 0]); // comment len
        h.extend_from_slice(&[0, 0]); // disk number
        h.extend_from_slice(&[0, 0]); // internal attrs
        h.extend_from_slice(&[0, 0, 0, 0]); // external attrs
        h.extend_from_slice(&local_offset.to_le_bytes());
        h
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc: u32 = 0xFFFF_FFFF;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    #[cfg(target_os = "windows")]
    fn make_zip(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
        let out = tempfile::tempdir().unwrap();
        let zip = out.path().join("archive.zip");
        write_zip(&zip, files);
        (out, zip)
    }

    #[cfg(target_os = "windows")]
    fn make_targz(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
        let content = tempfile::tempdir().unwrap();
        for (rel, data) in files {
            let p = content.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, data).unwrap();
        }
        let out = tempfile::tempdir().unwrap();
        let tgz = out.path().join("archive.tar.gz");
        let status = std::process::Command::new("tar.exe")
            .args([
                "-czf",
                &tgz.to_string_lossy(),
                "-C",
                &content.path().to_string_lossy(),
                ".",
            ])
            .status()
            .unwrap();
        assert!(status.success(), "创建 tar.gz 失败");
        (out, tgz)
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn extracts_zip_and_flattens_single_top_dir() {
        let (_c, zip) = make_zip(&[("top/main.py", "print(1)"), ("top/README.md", "hi")]);
        let dest = tempfile::tempdir().unwrap();
        extract_archive(&zip, dest.path()).unwrap();
        assert!(dest.path().join("main.py").exists(), "单层顶层目录应被剥离");
        assert!(dest.path().join("README.md").exists());
        assert!(!dest.path().join("top").exists(), "不应残留顶层目录");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn extracts_zip_without_flatten_when_multiple_tops() {
        let (_c, zip) = make_zip(&[("a.txt", "1"), ("b.txt", "2")]);
        let dest = tempfile::tempdir().unwrap();
        extract_archive(&zip, dest.path()).unwrap();
        assert!(dest.path().join("a.txt").exists());
        assert!(dest.path().join("b.txt").exists());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn extracts_targz_and_flattens_single_top_dir() {
        let (_c, tgz) = make_targz(&[("top/main.py", "print(1)"), ("top/README.md", "hi")]);
        let dest = tempfile::tempdir().unwrap();
        extract_archive(&tgz, dest.path()).unwrap();
        assert!(dest.path().join("main.py").exists());
        assert!(!dest.path().join("top").exists());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn rejects_unknown_archive_format() {
        let dest = tempfile::tempdir().unwrap();
        let bad = dest.path().join("x.xyz");
        std::fs::write(&bad, b"data").unwrap();
        assert!(extract_archive(&bad, &dest.path().join("out")).is_err());
    }

    // ---- 下载 + 解压 管线 ----
    #[cfg(target_os = "windows")]
    #[test]
    fn downloads_and_extracts_dependency_into_site_packages() {
        let (_c, zip) = make_zip(&[("top/pkg/__init__.py", "x"), ("top/pkg/util.py", "y")]);
        let payload = std::fs::read(&zip).unwrap();
        let (port, server) = serve_bytes(payload);
        let base = base_dir();
        let mut p = dependency_plugin();
        p.download_url = format!("http://127.0.0.1:{}/requests.zip", port);

        let install_dir =
            download_and_extract(&base, &p, "", &mut |_| {}).unwrap();

        // 依赖包直接解压进 site-packages，且单层顶层目录被剥离
        assert_eq!(
            install_dir,
            base.join("python").join("Lib").join("site-packages")
        );
        assert!(
            install_dir.join("pkg").join("__init__.py").exists(),
            "依赖包内容应落在 site-packages/pkg"
        );
        let _ = server.join();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn download_rejects_unsigned_plugin_when_pubkey_configured() {
        let (_c, zip) = make_zip(&[("main.py", "1")]);
        let payload = std::fs::read(&zip).unwrap();
        let (port, server) = serve_bytes(payload);
        let base = base_dir();
        let mut p = dependency_plugin();
        p.download_url = format!("http://127.0.0.1:{}/x.zip", port);
        // 未提供签名，但公钥非空 → 拒绝
        let err = download_and_extract(&base, &p, update_service::PUBLIC_KEY, &mut |_| {})
            .unwrap_err();
        assert!(err.contains("签名"), "{}", err);
        let _ = server.join();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn download_rejects_tampered_signature() {
        let (_c, zip) = make_zip(&[("main.py", "1")]);
        let payload = std::fs::read(&zip).unwrap();
        let (port, server) = serve_bytes(payload);
        let base = base_dir();
        let mut p = dependency_plugin();
        p.download_url = format!("http://127.0.0.1:{}/x.zip", port);
        p.signature = Some("bogus-signature".to_string());
        let err = download_and_extract(&base, &p, update_service::PUBLIC_KEY, &mut |_| {})
            .unwrap_err();
        assert!(err.contains("签名"), "{}", err);
        let _ = server.join();
    }

    // ---- 安装标记 ----
    #[test]
    fn marker_roundtrip_records_installed_state() {
        let base = base_dir();
        let p = dependency_plugin();
        write_marker(&base, &p, "C:/install", Some("app-123")).unwrap();
        let marker = read_marker(&base, "requests_dep").unwrap();
        assert_eq!(marker.version, Some("".to_string()));
        assert_eq!(marker.kind, "dependency");
        assert_eq!(marker.install_dir, "C:/install");
        assert_eq!(marker.app_id.as_deref(), Some("app-123"));

        let all = read_all_markers(&base);
        assert!(all.contains_key("requests_dep"));
    }
}
