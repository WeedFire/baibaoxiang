//! 自更新内核：平台匹配 → 下载（带进度）→ ed25519 验签 → 安装。
//!
//! 参考 `F:\custTools\autoUpdate`（forge-updater）的实现思路，按本项目需要重写：
//! - 清单与 Tauri updater 的 `latest.json` 完全兼容（`platforms` + `signature`）；
//! - 同时兼容 GitHub Releases API / 自建 JSON 风格（只有下载地址、无签名）；
//! - Windows 上安装包走系统安装器（`.msi` / `.exe`），便携版直接原地自替换。

use crate::models::{ResolvedUpdateAsset, UpdateManifest, UpdatePlatformAsset};
use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 下载超时：更新包可能有几十 MB，给宽松一些
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

// ---------------- 平台识别 ----------------

/// 当前平台键（Tauri 风格），如 `windows-x86_64`。
pub fn platform_key() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "amd64" => "x86_64",
        "arm64" => "aarch64",
        other => other,
    };
    format!("{}-{}", os, arch)
}

/// 把任意平台串（含常见别名）归一化成 `(os, arch)`。
///
/// 支持别名：`win`/`win32` → windows，`macos`/`osx`/`mac` → darwin，
/// `amd64`/`x64` → x86_64，`arm64` → aarch64，`i386`/`x86` → i686。
pub fn normalize_platform_key(key: &str) -> Option<(String, String)> {
    let mut parts = key.trim().splitn(2, '-');
    let os_raw = parts.next()?.trim().to_ascii_lowercase();
    let arch_raw = parts.next()?.trim().to_ascii_lowercase();

    let os = match os_raw.as_str() {
        "windows" | "win" | "win32" => "windows",
        "linux" | "gnu" | "musl" => "linux",
        "darwin" | "macos" | "osx" | "mac" => "darwin",
        _ => return None,
    };
    let arch = match arch_raw.as_str() {
        "x86_64" | "amd64" | "x64" => "x86_64",
        "aarch64" | "arm64" => "aarch64",
        "i686" | "x86" | "i386" => "i686",
        "arm" => "arm",
        _ => return None,
    };
    Some((os.to_string(), arch.to_string()))
}

/// 从平台表里挑出当前平台对应的资产：先精确匹配键，再按别名归一化匹配。
pub fn pick_platform_asset<'a>(
    platforms: &'a BTreeMap<String, UpdatePlatformAsset>,
    current: &str,
) -> Option<&'a UpdatePlatformAsset> {
    if let Some(asset) = platforms.get(current) {
        return Some(asset);
    }
    let target = normalize_platform_key(current);
    platforms
        .iter()
        .find(|(key, _)| normalize_platform_key(key) == target)
        .map(|(_, asset)| asset)
}

/// 从清单解析出适用于本机的更新资产。
///
/// 优先 `platforms` 平台表（带签名，可自动安装）；没有平台表时退化为
/// `assets`/`url` 里的下载地址（无签名，仅能下载后手动安装）。
pub fn resolve_asset(manifest: &UpdateManifest, current_platform: &str) -> Option<ResolvedUpdateAsset> {
    if let Some(platforms) = manifest.platforms.as_ref() {
        if let Some(asset) = pick_platform_asset(platforms, current_platform) {
            let url = asset.url.trim().to_string();
            if !url.is_empty() {
                return Some(ResolvedUpdateAsset {
                    url,
                    signature: asset
                        .signature
                        .as_ref()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty()),
                    platform_specific: true,
                });
            }
        }
    }
    manifest.download_url().map(|url| ResolvedUpdateAsset {
        url,
        signature: None,
        platform_specific: false,
    })
}

// ---------------- 下载 ----------------

/// 更新包存放目录：`%TEMP%/baibaoxiang-update`。
pub fn update_dir() -> PathBuf {
    std::env::temp_dir().join("baibaoxiang-update")
}

/// 从下载地址提取文件名（去掉查询串与片段）。
pub fn filename_from_url(url: &str) -> Option<String> {
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    let path_only = after_scheme.split(['?', '#']).next().unwrap_or(after_scheme);
    let segments: Vec<&str> = path_only.split('/').collect();
    let last = segments.last().copied().unwrap_or("").trim();
    if segments.len() < 2 || last.is_empty() {
        None
    } else {
        // 去掉可能的 URL 编码残留，保证是合法文件名
        Some(last.replace('%', "_"))
    }
}

/// 构造带原生 TLS 的 ureq 代理（与更新检查用同一套配置，避免 rustls 缺失 panic）。
pub fn build_agent(timeout: Duration) -> ureq::Agent {
    let tls_config = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::NativeTls)
        .build();
    let config = ureq::config::Config::builder()
        .timeout_global(Some(timeout))
        .tls_config(tls_config)
        .build();
    ureq::Agent::new_with_config(config)
}

/// 下载到 `dest`，每次写入回调 `on_progress(已下载, 总字节)`（总字节未知时为 `None`）。
pub fn download(
    url: &str,
    dest: &Path,
    user_agent: &str,
    on_progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<u64, String> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建下载目录失败: {}", e))?;
    }

    let agent = build_agent(DOWNLOAD_TIMEOUT);
    let mut response = agent
        .get(url)
        .header("User-Agent", user_agent)
        .call()
        .map_err(|e| format!("下载更新包失败: {}", e))?;

    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("下载更新包失败: 服务器返回 HTTP {}", status));
    }
    let total: Option<u64> = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());

    let mut reader = response.body_mut().as_reader();
    let mut file = std::fs::File::create(dest).map_err(|e| format!("创建文件失败: {}", e))?;
    let mut written: u64 = 0;
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|e| format!("读取响应失败: {}", e))?;
        if read == 0 {
            break;
        }
        file.write_all(&chunk[..read])
            .map_err(|e| format!("写入文件失败: {}", e))?;
        written += read as u64;
        on_progress(written, total);
    }
    file.flush().map_err(|e| format!("写入文件失败: {}", e))?;
    Ok(written)
}

// ---------------- 验签 ----------------

/// 用 base64 公钥校验 base64 签名（ed25519，与 Tauri updater 格式一致）。
///
/// 返回 `Ok(false)` 表示签名不匹配（数据被篡改或密钥不对）；
/// 公钥/签名格式非法则返回 `Err`。
pub fn verify_signature(pubkey_b64: &str, data: &[u8], signature_b64: &str) -> Result<bool, String> {
    let pubkey_bytes = base64::engine::general_purpose::STANDARD
        .decode(strip_bom(pubkey_b64).trim())
        .map_err(|e| format!("公钥不是合法的 base64: {}", e))?;
    if pubkey_bytes.len() != 32 {
        return Err(format!("公钥应为 32 字节，实际 {} 字节", pubkey_bytes.len()));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&pubkey_bytes);
    let verifying = VerifyingKey::from_bytes(&key).map_err(|e| format!("公钥无效: {}", e))?;

    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(strip_bom(signature_b64).trim())
        .map_err(|e| format!("签名不是合法的 base64: {}", e))?;
    if sig_bytes.len() != 64 {
        return Err(format!("签名应为 64 字节，实际 {} 字节", sig_bytes.len()));
    }
    let signature = Signature::from_slice(&sig_bytes).map_err(|e| format!("签名无效: {}", e))?;

    Ok(verifying.verify(data, &signature).is_ok())
}

fn strip_bom(text: &str) -> &str {
    text.trim_start_matches('\u{feff}')
}

// ---------------- 安装 ----------------

/// 安装方式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    /// Windows Installer 包，交给 `msiexec` 安装
    Msi,
    /// 安装程序（NSIS 等），静默参数安装
    SetupExe,
    /// 便携版：下载的就是程序自身，直接原地替换
    Portable,
}

/// 一次安装要执行的命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
    pub program: String,
    pub args: Vec<String>,
    pub kind: InstallKind,
}

/// 根据更新包与当前可执行文件决定安装方式。
///
/// - `.msi` → `msiexec /i <包> /qb /norestart`
/// - `.exe` 且文件名与当前程序相同 → 便携版自替换
/// - 其它 `.exe` → 视作安装程序，加 `/S` 静默安装
pub fn installer_plan(package: &Path, current_exe: &Path) -> Result<InstallPlan, String> {
    let ext = package
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let package_str = package.to_string_lossy().to_string();

    match ext.as_str() {
        "msi" => Ok(InstallPlan {
            program: "msiexec".to_string(),
            args: vec![
                "/i".to_string(),
                package_str,
                "/qb".to_string(),
                "/norestart".to_string(),
            ],
            kind: InstallKind::Msi,
        }),
        "exe" => {
            let same_name = package
                .file_name()
                .zip(current_exe.file_name())
                .map(|(a, b)| a.eq_ignore_ascii_case(b))
                .unwrap_or(false);
            if same_name {
                Ok(InstallPlan {
                    program: package_str,
                    args: Vec::new(),
                    kind: InstallKind::Portable,
                })
            } else {
                Ok(InstallPlan {
                    program: package_str,
                    args: vec!["/S".to_string()],
                    kind: InstallKind::SetupExe,
                })
            }
        }
        other => {
            let label = if other.is_empty() {
                "未知格式".to_string()
            } else {
                format!(".{}", other)
            };
            Err(format!(
                "暂不支持自动安装{}格式的更新包，请手动安装：{}",
                label, package_str
            ))
        }
    }
}

/// 启动安装程序（不等待其结束：安装过程通常会替换本程序，需要本进程先退出）。
pub fn run_installer(plan: &InstallPlan) -> Result<(), String> {
    let mut command = std::process::Command::new(&plan.program);
    command.args(&plan.args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // 让安装程序独立于本进程，避免本程序退出时被一并结束
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    command
        .spawn()
        .map_err(|e| format!("启动安装程序失败（{}）: {}", plan.program, e))?;
    Ok(())
}

/// 便携版自替换：把下载到的新程序替换掉当前可执行文件。
///
/// Windows 上正在运行的 exe 无法直接覆盖，先备份旧文件；若仍失败则预约
/// 下次重启时替换（`MoveFileExW` + `MOVEFILE_DELAY_UNTIL_REBOOT`）。
pub fn replace_portable(target: &Path, new_exe: &Path) -> Result<(), String> {
    if target.exists() {
        let backup = backup_path(target);
        let _ = std::fs::remove_file(&backup);
        if std::fs::rename(target, &backup).is_err() {
            let _ = std::fs::copy(target, &backup);
        }
    }

    match std::fs::copy(new_exe, target) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            schedule_replace_on_reboot(target, new_exe)
        }
        Err(error) => Err(format!("替换程序文件失败: {}", error)),
    }
}

/// 备份路径：`app.exe` → `app.exe.bak`
pub fn backup_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_os_string();
    name.push(".bak");
    PathBuf::from(name)
}

#[cfg(windows)]
fn schedule_replace_on_reboot(target: &Path, new_exe: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};

    let wide = |path: &Path| -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let from = wide(new_exe);
    let to = wide(target);

    let ok = unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_DELAY_UNTIL_REBOOT,
        )
    };
    ok.map_err(|e| format!("无法替换正在运行的程序文件（{}），且预约重启替换失败", e))
}

#[cfg(not(windows))]
fn schedule_replace_on_reboot(_target: &Path, _new_exe: &Path) -> Result<(), String> {
    Err("无法替换正在运行的可执行文件".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{UpdateAsset, UpdateManifest};

    const SAMPLE_LATEST_JSON: &str = r#"{
        "version": "1.2.3",
        "notes": "修复若干问题",
        "pub_date": "2026-09-01T10:00:00Z",
        "platforms": {
            "windows-x86_64": { "signature": "SIG_WIN", "url": "https://example.com/百宝箱_1.2.3_x64-setup.exe" },
            "linux-x86_64":   { "signature": "SIG_LIN", "url": "https://example.com/app.AppImage" },
            "darwin-aarch64": { "signature": "SIG_MAC", "url": "https://example.com/app.app.tar.gz" }
        }
    }"#;

    fn parse(json: &str) -> UpdateManifest {
        serde_json::from_str(json).expect("清单应能解析")
    }

    #[test]
    fn 当前平台键是规范格式() {
        let key = platform_key();
        assert!(key.contains('-'), "平台键应形如 os-arch，实际 {}", key);
        let (os, arch) = normalize_platform_key(&key).expect("自身平台键必须能归一化");
        assert!(matches!(os.as_str(), "windows" | "linux" | "darwin"));
        assert!(!arch.is_empty());
    }

    #[test]
    fn 平台别名归一化() {
        assert_eq!(
            normalize_platform_key("win32-amd64"),
            Some(("windows".into(), "x86_64".into()))
        );
        assert_eq!(
            normalize_platform_key("macos-arm64"),
            Some(("darwin".into(), "aarch64".into()))
        );
        assert_eq!(normalize_platform_key("windows"), None);
        assert_eq!(normalize_platform_key("freebsd-x86_64"), None);
    }

    #[test]
    fn 解析_latest_json_并选中本机资产() {
        let manifest = parse(SAMPLE_LATEST_JSON);
        assert_eq!(manifest.version, "1.2.3");
        assert_eq!(manifest.pub_date.as_deref(), Some("2026-09-01T10:00:00Z"));

        let asset = resolve_asset(&manifest, "windows-x86_64").unwrap();
        assert!(asset.platform_specific);
        assert_eq!(asset.signature.as_deref(), Some("SIG_WIN"));
        assert!(asset.url.ends_with("百宝箱_1.2.3_x64-setup.exe"));

        assert!(resolve_asset(&manifest, "windows-aarch64").is_none());
    }

    #[test]
    fn 平台别名键也能命中() {
        let manifest = parse(SAMPLE_LATEST_JSON);
        let asset = resolve_asset(&manifest, "win32-x64").unwrap();
        assert_eq!(asset.signature.as_deref(), Some("SIG_WIN"));
    }

    #[test]
    fn 无平台表时退化为通用下载地址() {
        let manifest = parse(
            r#"{ "tag_name": "v1.0.2", "url": "https://example.com/a.exe", "assets": [] }"#,
        );
        let asset = resolve_asset(&manifest, "windows-x86_64").unwrap();
        assert!(!asset.platform_specific);
        assert!(asset.signature.is_none());
        assert_eq!(asset.url, "https://example.com/a.exe");
    }

    #[test]
    fn github_附件优先于通用地址() {
        let manifest = parse(
            r#"{ "tag_name": "v1.0.2", "html_url": "https://github.com/x/y/releases/tag/v1.0.2",
                "assets": [{ "name": "a.msi", "browser_download_url": "https://example.com/a.msi" }] }"#,
        );
        let asset = resolve_asset(&manifest, "windows-x86_64").unwrap();
        assert_eq!(asset.url, "https://example.com/a.msi");
    }

    #[test]
    fn 从_url_提取文件名() {
        assert_eq!(
            filename_from_url("https://example.com/releases/百宝箱_1.2.3_x64-setup.exe"),
            Some("百宝箱_1.2.3_x64-setup.exe".to_string())
        );
        assert_eq!(
            filename_from_url("https://example.com/app.exe?token=abc#frag"),
            Some("app.exe".to_string())
        );
        assert_eq!(filename_from_url("https://example.com/"), None);
    }

    #[test]
    fn 安装计划_按扩展名分派() {
        let current = Path::new("C:/app/百宝箱.exe");

        let msi = installer_plan(Path::new("C:/tmp/百宝箱_1.2.3_x64.msi"), current).unwrap();
        assert_eq!(msi.kind, InstallKind::Msi);
        assert_eq!(msi.program, "msiexec");
        assert!(msi.args.contains(&"/qb".to_string()));

        let setup = installer_plan(Path::new("C:/tmp/百宝箱_1.2.3_x64-setup.exe"), current).unwrap();
        assert_eq!(setup.kind, InstallKind::SetupExe);
        assert_eq!(setup.args, vec!["/S".to_string()]);

        // 文件名与当前程序一致 → 便携版自替换
        let portable = installer_plan(Path::new("D:/dl/百宝箱.exe"), current).unwrap();
        assert_eq!(portable.kind, InstallKind::Portable);

        assert!(installer_plan(Path::new("C:/tmp/update.zip"), current).is_err());
    }

    #[test]
    fn 备份路径追加_bak() {
        assert_eq!(
            backup_path(Path::new("C:/app/百宝箱.exe")),
            PathBuf::from("C:/app/百宝箱.exe.bak")
        );
    }

    /// 便携版自替换：新文件覆盖目标，旧文件留作 .bak（不依赖正在运行的进程）。
    #[test]
    fn 便携版自替换_覆盖并保留备份() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("百宝箱.exe");
        let new_exe = dir.path().join("download").join("百宝箱.exe");
        std::fs::create_dir_all(new_exe.parent().unwrap()).unwrap();
        std::fs::write(&target, b"old version").unwrap();
        std::fs::write(&new_exe, b"new version").unwrap();

        replace_portable(&target, &new_exe).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"new version");
        assert_eq!(std::fs::read(backup_path(&target)).unwrap(), b"old version");
    }

    #[test]
    fn 验签_自签可验证_被篡改则失败() {
        use ed25519_dalek::{Signer, SigningKey};

        // 固定种子 → 确定性密钥对，测试不依赖随机数生成器
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let pubkey_b64 =
            base64::engine::general_purpose::STANDARD.encode(signing.verifying_key().to_bytes());

        let data = b"baibaoxiang update payload".to_vec();
        let sig_b64 =
            base64::engine::general_purpose::STANDARD.encode(signing.sign(&data).to_bytes());

        assert!(verify_signature(&pubkey_b64, &data, &sig_b64).unwrap());
        // 数据被篡改 → 校验失败（返回 false 而不是报错）
        assert!(!verify_signature(&pubkey_b64, b"tampered payload", &sig_b64).unwrap());
        // 换一把公钥 → 校验失败
        let other = SigningKey::from_bytes(&[9u8; 32]);
        let other_pub =
            base64::engine::general_purpose::STANDARD.encode(other.verifying_key().to_bytes());
        assert!(!verify_signature(&other_pub, &data, &sig_b64).unwrap());
    }

    #[test]
    fn 验签_非法输入返回错误() {
        let short_key = base64::engine::general_purpose::STANDARD.encode(b"tooshort");
        let dummy_sig = base64::engine::general_purpose::STANDARD.encode([0u8; 64]);
        assert!(verify_signature(&short_key, b"x", &dummy_sig).is_err());
        assert!(verify_signature("not base64!!", b"x", &dummy_sig).is_err());
        // 签名长度不对 → 报错
        let good_key = base64::engine::general_purpose::STANDARD.encode([1u8; 32]);
        let short_sig = base64::engine::general_purpose::STANDARD.encode(b"short");
        assert!(verify_signature(&good_key, b"x", &short_sig).is_err());
    }

    /// 起一个一次性本地 HTTP 服务，验证下载 + 进度回调确实按字节推进。
    #[test]
    fn 下载可从本地_http_取得完整内容() {
        use std::io::Write as _;
        use std::net::TcpListener;

        let payload = vec![7u8; 300 * 1024];
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = payload.clone();
        let server = std::thread::spawn(move || {
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

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("pkg.bin");
        let mut calls: Vec<(u64, Option<u64>)> = Vec::new();
        let url = format!("http://127.0.0.1:{}/pkg.bin", port);
        let written = download(&url, &dest, "baibaoxiang/test", &mut |done, total| {
            calls.push((done, total))
        })
        .unwrap();

        assert_eq!(written as usize, payload.len());
        assert_eq!(std::fs::read(&dest).unwrap(), payload);
        assert!(!calls.is_empty(), "应至少回调一次进度");
        assert_eq!(calls.last().unwrap().0 as usize, payload.len());
        assert_eq!(calls.last().unwrap().1, Some(payload.len() as u64));
        let _ = server.join();
    }

    #[test]
    fn 下载遇到_404_报错() {
        use std::io::Write as _;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 1024];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                let _ = stream.flush();
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("missing.bin");
        let url = format!("http://127.0.0.1:{}/missing.bin", port);
        let error = download(&url, &dest, "baibaoxiang/test", &mut |_, _| {}).unwrap_err();
        assert!(error.contains("404"), "错误信息应包含状态码：{}", error);
        let _ = server.join();
    }

    #[test]
    fn update_manifest_兼容旧字段() {
        let manifest = parse(r#"{ "version": "1.0.2", "url": "https://example.com/a.exe" }"#);
        assert!(manifest.platforms.is_none());
        assert_eq!(manifest.download_url().as_deref(), Some("https://example.com/a.exe"));
        let _ = UpdateAsset {
            browser_download_url: None,
            name: None,
        };
    }
}
