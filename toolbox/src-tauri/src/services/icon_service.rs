use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tauri::Manager;

use crate::utils::png::write_png_rgba;

/// 图标缓存目录：`<app_data_dir>/icons`
fn icon_cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法定位应用数据目录: {}", e))?;
    Ok(base.join("icons"))
}

/// 64 位 FNV-1a，用于生成缓存文件名。
fn fnv1a64(input: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in input.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

/// 生成缓存键，纳入文件大小与修改时间，避免 exe 更新后沿用旧图标。
fn cache_key(target: &Path) -> String {
    let meta = std::fs::metadata(target).ok();
    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let mtime = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let normalized = crate::utils::normalize_path(&target.to_string_lossy()).to_lowercase();
    format!("{}|{}|{}", normalized, size, mtime)
}

/// 提取文件图标并缓存为 PNG，返回可直接使用的绝对路径。
///
/// - `file_path`：目标文件（一般是 exe；也可能是 .py 脚本）
/// - `interpreter_hint`：Python 脚本时用于取解释器图标的可选解释器路径
/// 返回 `Ok(None)` 表示没有可用图标，前端应回退到占位符。
pub fn extract_icon(
    app: &AppHandle,
    file_path: &str,
    interpreter_hint: Option<&str>,
) -> Result<Option<String>, String> {
    let cache_dir = icon_cache_dir(app)?;
    extract_icon_into(&cache_dir, file_path, interpreter_hint)
}

/// 与 `extract_icon` 相同，但缓存目录由调用方指定，便于测试。
pub fn extract_icon_into(
    cache_dir: &Path,
    file_path: &str,
    interpreter_hint: Option<&str>,
) -> Result<Option<String>, String> {
    // 相对路径按「程序运行目录」解析，绝对路径原样使用
    let path = crate::utils::resolve_path(file_path);
    if !path.exists() {
        return Err(format!("文件不存在: {}", path.display()));
    }

    // 脚本本身没有图标，退化为使用解释器的图标
    let icon_source: PathBuf = if crate::utils::looks_like_python_script(file_path) {
        match interpreter_hint {
            Some(p) if !p.trim().is_empty() => {
                let hint = crate::utils::resolve_path(p);
                if !hint.exists() {
                    return Ok(None);
                }
                hint
            }
            _ => return Ok(None),
        }
    } else {
        path
    };

    std::fs::create_dir_all(cache_dir).map_err(|e| format!("无法创建图标缓存目录: {}", e))?;

    let key = cache_key(&icon_source);
    let icon_path = cache_dir.join(format!("{:016x}.png", fnv1a64(&key)));

    if icon_path.exists() {
        return Ok(Some(icon_path.to_string_lossy().to_string()));
    }

    #[cfg(target_os = "windows")]
    {
        // Shell 图标查询偶发失败，重试几次再放弃
        let mut last_error = String::new();
        for attempt in 0..3 {
            match icon_to_png(&icon_source, ICON_SIZE, &icon_path) {
                Ok(()) => return Ok(Some(icon_path.to_string_lossy().to_string())),
                Err(e) => {
                    last_error = e;
                    if attempt < 2 {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                }
            }
        }
        eprintln!("[icon] 提取失败 {}: {}", icon_source.display(), last_error);
        return Ok(None);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = write_png_rgba; // 非 Windows 平台不做图标提取
        Ok(None)
    }
}

pub const ICON_SIZE: i32 = 48;

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// 调用 Win32 取得文件关联图标，绘制到 32bpp DIB 后编码为 PNG。
#[cfg(target_os = "windows")]
fn icon_to_png(target: &Path, size: i32, output: &Path) -> Result<(), String> {
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::UI::Shell::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    unsafe {
        let wide = to_wide(&target.to_string_lossy());
        let mut sfi = SHFILEINFOW::default();
        let ret = SHGetFileInfoW(
            windows::core::PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut sfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );

        if ret == 0 || sfi.hIcon.is_invalid() {
            return Err("系统未提供该文件的图标".to_string());
        }

        let rgba = render_hicon(sfi.hIcon, size);
        let _ = DestroyIcon(sfi.hIcon);
        let (w, h, pixels) = rgba?;
        write_png_rgba(output, w, h, &pixels)
    }
}

/// 把 HICON 绘制到离屏 32bpp 位图，返回 (宽, 高, RGBA 数据)。
#[cfg(target_os = "windows")]
unsafe fn render_hicon(
    icon: windows::Win32::UI::WindowsAndMessaging::HICON,
    size: i32,
) -> Result<(u32, u32, Vec<u8>), String> {
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    let screen_dc = GetDC(None);
    if screen_dc.is_invalid() {
        return Err("GetDC 失败".to_string());
    }
    let mem_dc = CreateCompatibleDC(Some(screen_dc));
    if mem_dc.is_invalid() {
        ReleaseDC(None, screen_dc);
        return Err("CreateCompatibleDC 失败".to_string());
    }

    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: size,
        biHeight: -size, // 负值 => 自上而下，行序即 PNG 行序
        biPlanes: 1,
        biBitCount: 32,
        biCompression: 0, // BI_RGB
        biSizeImage: 0,
        biXPelsPerMeter: 0,
        biYPelsPerMeter: 0,
        biClrUsed: 0,
        biClrImportant: 0,
    };
    let info = BITMAPINFO {
        bmiHeader: header,
        bmiColors: [RGBQUAD::default(); 1],
    };

    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let bitmap = CreateDIBSection(Some(mem_dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        .map_err(|e| format!("CreateDIBSection 失败: {}", e))?;

    let old = SelectObject(mem_dc, bitmap.into());
    let stride = (size * 4) as usize;
    let result = (|| -> Result<Vec<u8>, String> {
        if bits.is_null() {
            return Err("CreateDIBSection 未返回像素缓冲区".to_string());
        }
        DrawIconEx(mem_dc, 0, 0, icon, size, size, 0, None, DI_NORMAL)
            .map_err(|e| format!("DrawIconEx 失败: {}", e))?;
        let slice = std::slice::from_raw_parts(bits as *const u8, stride * size as usize);
        // BGRA -> RGBA
        let mut rgba = Vec::with_capacity(slice.len());
        for chunk in slice.chunks_exact(4) {
            rgba.push(chunk[2]);
            rgba.push(chunk[1]);
            rgba.push(chunk[0]);
            rgba.push(chunk[3]);
        }
        Ok(rgba)
    })();

    SelectObject(mem_dc, old);
    let _ = DeleteObject(bitmap.into());
    let _ = DeleteDC(mem_dc);
    ReleaseDC(None, screen_dc);

    Ok((size as u32, size as u32, result?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_is_stable_and_distinct() {
        let p = PathBuf::from("C:/a/b.exe");
        let a = cache_key(&p);
        let b = cache_key(&p);
        assert_eq!(a, b);
        assert!(fnv1a64(&a) != fnv1a64("C:/a/c.exe|0|0"));
    }

    /// 真实图标提取依赖 Shell，串行执行以避免并发干扰。
    #[cfg(target_os = "windows")]
    fn icon_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 对系统自带 exe 做一次真实的图标提取，确认得到非空的 PNG。
    #[cfg(target_os = "windows")]
    #[test]
    fn extracts_real_icon_from_system_exe() {
        let _guard = icon_lock();
        let exe = PathBuf::from(r"C:\Windows\notepad.exe");
        if !exe.exists() {
            eprintln!("skip: 未找到 notepad.exe");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("nested").join("icon.png");

        icon_to_png(&exe, ICON_SIZE, &out).expect("应能提取图标");

        let meta = std::fs::metadata(&out).unwrap();
        assert!(meta.len() > 100, "PNG 过小，疑似空图: {} 字节", meta.len());

        let img = image::ImageReader::open(&out).unwrap().decode().unwrap();
        assert_eq!(img.width(), ICON_SIZE as u32);
        assert_eq!(img.height(), ICON_SIZE as u32);
        assert!(
            img.to_rgba8().pixels().any(|p| p[3] > 0),
            "提取出的图标不应全透明"
        );
    }

    #[test]
    fn missing_file_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let err = extract_icon_into(dir.path(), "__definitely_missing__.exe", None).unwrap_err();
        assert!(err.contains("文件不存在"), "{}", err);
    }

    #[test]
    fn python_script_without_interpreter_has_no_icon() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("main.py");
        std::fs::write(&script, b"print(1)").unwrap();
        let result = extract_icon_into(dir.path(), &script.to_string_lossy(), None).unwrap();
        assert!(result.is_none());
    }

    /// 完整流程：exe -> 缓存 PNG，且第二次调用直接命中缓存。
    #[cfg(target_os = "windows")]
    #[test]
    fn caches_icon_and_reuses_it() {
        let _guard = icon_lock();
        let exe = PathBuf::from(r"C:\Windows\notepad.exe");
        if !exe.exists() {
            eprintln!("skip: 未找到 notepad.exe");
            return;
        }
        let dir = tempfile::tempdir().unwrap();

        let first = extract_icon_into(dir.path(), &exe.to_string_lossy(), None)
            .unwrap()
            .expect("应提取到图标");
        assert!(Path::new(&first).exists());
        let mtime = std::fs::metadata(&first).unwrap().modified().unwrap();

        let second = extract_icon_into(dir.path(), &exe.to_string_lossy(), None)
            .unwrap()
            .expect("应命中缓存");
        assert_eq!(first, second);
        assert_eq!(
            std::fs::metadata(&second).unwrap().modified().unwrap(),
            mtime,
            "缓存命中时不应重写文件"
        );
    }
}
