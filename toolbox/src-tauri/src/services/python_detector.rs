use crate::models::{AppItem, PythonInstallation};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// 随程序一起安装的内置 Python，由 `lib.rs` 启动时探测后写入。
/// 单元测试不会初始化它，测试环境因此仍退回系统解释器。
static BUNDLED_PYTHON: OnceLock<PathBuf> = OnceLock::new();

/// 记录内置解释器的绝对路径。
pub fn set_bundled_python(path: PathBuf) {
    let _ = BUNDLED_PYTHON.set(path);
}

/// 内置解释器的绝对路径，未探测到时返回 None。
pub fn bundled_python() -> Option<&'static Path> {
    BUNDLED_PYTHON.get().map(|path| path.as_path())
}

/// 内置解释器相对「程序运行目录」的写法（如 `python\python.exe`），
/// 用它保存配置可让应用不依赖具体安装位置；不在程序目录下时返回 None。
fn bundled_relative_path(absolute: &Path) -> Option<String> {
    let base = crate::utils::app_base_dir()?;
    let relative = absolute.strip_prefix(base).ok()?;
    Some(relative.to_string_lossy().replace('/', "\\"))
}

/// 在候选根目录下查找随程序安装的解释器。
///
/// 依次尝试 `<root>/python/python.exe` 与 `<root>/resources/python/python.exe`，
/// 兼容“安装目录即资源目录”（Windows）与“资源位于 resources 子目录”两种布局。
pub fn find_bundled_python(roots: &[PathBuf]) -> Option<PathBuf> {
    for root in roots {
        let direct = root.join("python").join("python.exe");
        if direct.is_file() {
            return Some(direct);
        }
        let nested = root.join("resources").join("python").join("python.exe");
        if nested.is_file() {
            return Some(nested);
        }
    }
    None
}

/// 检测本机所有可用的 Python 解释器，返回可直接 spawn 的绝对路径。
///
/// 排序：系统解释器（py 启动器 / 常见目录 / conda / PATH）按版本降序在前，
/// 随程序安装的内置解释器垫底作为兜底——优先使用用户自己的环境
/// （装了第三方包），内置副本只在系统没有 Python 时才被选中。
pub fn detect_python_installations() -> Vec<PythonInstallation> {
    detect_with_bundled(bundled_python())
}

/// 同上，但内置解释器路径由参数注入以便测试。
fn detect_with_bundled(bundled: Option<&Path>) -> Vec<PythonInstallation> {
    let mut found: Vec<PythonInstallation> = Vec::new();

    for p in from_py_launcher() {
        try_add(&mut found, p, "py-launcher", false);
    }
    for p in from_common_dirs() {
        try_add(&mut found, p, "programs-dir", false);
    }
    for p in from_conda() {
        try_add(&mut found, p, "conda", false);
    }
    for p in from_path() {
        try_add(&mut found, p, "path", false);
    }

    // 真实存在的优先，其次按版本号降序
    found.sort_by(|a, b| {
        let va = version_key(a.version.as_deref());
        let vb = version_key(b.version.as_deref());
        vb.cmp(&va)
    });

    // 内置解释器垫底：仅当系统没有任何解释器时才会被自动选中
    if let Some(bundled) = bundled {
        let key = path_key(bundled);
        match found.iter().position(|i| path_key(Path::new(&i.path)) == key) {
            Some(index) => {
                let mut item = found.remove(index);
                item.source = "bundled".to_string();
                item.relative_path = bundled_relative_path(bundled);
                found.push(item);
            }
            None => found.push(PythonInstallation {
                path: bundled.to_string_lossy().to_string(),
                version: version_of(bundled),
                source: "bundled".to_string(),
                is_venv: false,
                relative_path: bundled_relative_path(bundled),
            }),
        }
    }

    found
}

/// 从脚本所在目录向上查找虚拟环境。
pub fn find_venv_for_script(script_path: &str) -> Option<PythonInstallation> {
    let mut dir = Path::new(script_path).parent()?.to_path_buf();
    for _ in 0..5 {
        for name in [".venv", "venv", "env", ".env"] {
            let candidate = dir.join(name).join("Scripts").join("python.exe");
            if candidate.exists() {
                return Some(PythonInstallation {
                    path: candidate.to_string_lossy().to_string(),
                    version: version_of(&candidate),
                    source: "venv".to_string(),
                    is_venv: true,
                    relative_path: None,
                });
            }
            // POSIX 风格虚拟环境（WSL / 跨平台项目）
            let bin = dir.join(name).join("bin").join("python");
            if bin.exists() {
                return Some(PythonInstallation {
                    path: bin.to_string_lossy().to_string(),
                    version: version_of(&bin),
                    source: "venv".to_string(),
                    is_venv: true,
                    relative_path: None,
                });
            }
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// 决定实际使用的解释器：
/// 显式配置 > 脚本旁虚拟环境 > 系统检测到的解释器 > 随程序安装的内置解释器 > PATH 上的 python。
/// 脚本与解释器中的相对路径都以 `base`（「程序运行目录」，一般是安装目录）为根解析。
pub fn resolve_interpreter_with_base(app: &AppItem, base: Option<&Path>) -> Result<String, String> {
    resolve_interpreter_in(app, bundled_python(), base, &detect_python_installations())
}

/// `resolve_interpreter` 的实现：内置解释器、检测列表与相对路径的根由参数注入以便测试。
fn resolve_interpreter_in(
    app: &AppItem,
    bundled: Option<&Path>,
    base: Option<&Path>,
    detected: &[PythonInstallation],
) -> Result<String, String> {
    if let Some(ref configured) = app.python_interpreter_path {
        let configured = configured.trim();
        if !configured.is_empty() {
            // 相对路径按程序运行目录解析，绝对路径原样使用
            let resolved = crate::utils::resolve_path_with(base, configured);
            if resolved.exists() {
                return Ok(resolved.to_string_lossy().to_string());
            }
            // 裸命令名（python / py）交给 PATH 解析
            if version_of_command(configured).is_some() {
                return Ok(configured.to_string());
            }
            return Err(format!("配置的 Python 解释器不存在: {}", configured));
        }
    }

    if !app.is_python_script {
        return Ok(crate::utils::resolve_path_with(base, &app.executable_path)
            .to_string_lossy()
            .to_string());
    }

    let script = crate::utils::resolve_path_with(base, &app.executable_path)
        .to_string_lossy()
        .to_string();

    if let Some(venv) = find_venv_for_script(&script) {
        return Ok(venv.path);
    }

    // 用户自己安装的 Python（带第三方包）优先
    if let Some(first) = detected.first() {
        return Ok(first.path.clone());
    }

    // 系统没有任何 Python 时，回退到安装目录自带的解释器
    if let Some(bundled) = bundled {
        return Ok(bundled.to_string_lossy().to_string());
    }

    if version_of_command("python").is_some() {
        return Ok("python".to_string());
    }

    Err("未检测到可用的 Python 解释器，请在编辑应用时指定 python.exe 的完整路径".to_string())
}

/// 当不需要显示控制台且解释器是 `python.exe` 时，改用同目录的 `pythonw.exe`，
/// 避免弹出一闪而过的控制台窗口。
pub fn apply_console_preference(interpreter: &str, show_console: bool) -> String {
    if show_console {
        return interpreter.to_string();
    }
    let path = Path::new(interpreter);
    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return interpreter.to_string();
    };
    if file_name.eq_ignore_ascii_case("python.exe") {
        let pythonw = path.with_file_name("pythonw.exe");
        if pythonw.exists() {
            return pythonw.to_string_lossy().to_string();
        }
    }
    interpreter.to_string()
}

/// 路径比较用的归一化键：统一大小写与分隔符，避免同一解释器被重复收录。
fn path_key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase().replace('/', "\\")
}

fn try_add(list: &mut Vec<PythonInstallation>, path: PathBuf, source: &str, is_venv: bool) {
    if !path.exists() {
        return;
    }
    let key = path_key(&path);
    if list.iter().any(|i| path_key(Path::new(&i.path)) == key) {
        return;
    }
    // 排除 Microsoft Store 的占位启动器（未安装时只会打开应用商店）
    if key.contains("windowsapps") && key.ends_with("python.exe") {
        return;
    }
    let mut installation = PythonInstallation {
        path: path.to_string_lossy().to_string(),
        version: version_of(&path),
        source: source.to_string(),
        is_venv,
        relative_path: None,
    };
    if !is_venv && detect_venv(&path) {
        installation.is_venv = true;
    }
    list.push(installation);
}

fn detect_venv(python_exe: &Path) -> bool {
    python_exe
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("pyvenv.cfg").exists())
        .unwrap_or(false)
}

/// `py -0p` 会列出 Python 启动器登记的所有解释器及其路径。
fn from_py_launcher() -> Vec<PathBuf> {
    let output = match Command::new("py").args(["-0p"]).output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    if !output.status.success() && output.stdout.is_empty() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut result = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(start) = line.find(":\\") else {
            continue;
        };
        if start == 0 {
            continue;
        }
        // 路径从盘符开始，往后遇到两个连续空格即结束（允许路径中含单个空格）
        let rest = &line[start - 1..];
        let end = rest
            .find("  ")
            .unwrap_or(rest.len());
        let candidate = rest[..end].trim();
        let candidate = candidate.trim_end_matches(['\u{e9c}', '*']).trim();
        if candidate.to_lowercase().ends_with("python.exe") {
            result.push(PathBuf::from(candidate));
        }
    }
    result
}

fn from_common_dirs() -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut roots: Vec<PathBuf> = Vec::new();

    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        roots.push(PathBuf::from(local).join("Programs").join("Python"));
    }
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(dir) = std::env::var(var) {
            roots.push(PathBuf::from(dir));
        }
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        roots.push(PathBuf::from(home).join("AppData").join("Local").join("Programs").join("Python"));
    }
    roots.push(PathBuf::from(r"C:\"));

    for root in roots {
        if let Ok(entries) = std::fs::read_dir(&root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if !entry.path().is_dir() {
                    continue;
                }
                // PythonXY / Python3XY / PythonXY-32
                let is_python_dir = name.starts_with("python") && name[6..].chars().any(|c| c.is_ascii_digit());
                if !is_python_dir {
                    continue;
                }
                let exe = entry.path().join("python.exe");
                if exe.exists() {
                    result.push(exe);
                }
            }
        }
    }
    result
}

fn from_conda() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(home) = std::env::var("USERPROFILE") {
        roots.push(PathBuf::from(&home).join("anaconda3"));
        roots.push(PathBuf::from(&home).join("miniconda3"));
        roots.push(PathBuf::from(&home).join("miniforge3"));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        roots.push(local.join("anaconda3"));
        roots.push(local.join("miniconda3"));
    }
    if let Ok(pd) = std::env::var("ProgramData") {
        let pd = PathBuf::from(pd);
        roots.push(pd.join("anaconda3"));
        roots.push(pd.join("miniconda3"));
    }
    roots
        .into_iter()
        .map(|r| r.join("python.exe"))
        .filter(|p| p.exists())
        .collect()
}

fn from_path() -> Vec<PathBuf> {
    let mut result = Vec::new();
    for cmd in ["python", "python3"] {
        if let Ok(output) = Command::new("where").arg(cmd).output() {
            if output.status.success() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    let line = line.trim();
                    if !line.is_empty() {
                        result.push(PathBuf::from(line));
                    }
                }
            }
        }
    }
    result
}

/// 执行 `<interp> --version` 解析版本号。
pub fn version_of(interpreter: &Path) -> Option<String> {
    let output = Command::new(interpreter)
        .arg("--version")
        .output()
        .ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    parse_version(&text)
}

pub fn version_of_command(command: &str) -> Option<String> {
    let output = Command::new(command).arg("--version").output().ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    parse_version(&text)
}

/// 从 "Python 3.13.0" / "Python 3.9.13 :: ..." 中取出版本号。
pub fn parse_version(text: &str) -> Option<String> {
    let text = text.trim();
    let rest = text.strip_prefix("Python").unwrap_or(text).trim();
    let token = rest.split_whitespace().next()?;
    if token.chars().next().map(|c| c.is_ascii_digit()) != Some(true) {
        return None;
    }
    let version: String = token
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if version.is_empty() {
        None
    } else {
        Some(version)
    }
}

/// 把 "3.13.0" 编码成可比较的整数，便于降序排序。
fn version_key(version: Option<&str>) -> (u32, u32, u32) {
    let Some(v) = version else {
        return (0, 0, 0);
    };
    let mut parts = v.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        assert_eq!(parse_version("Python 3.13.0").as_deref(), Some("3.13.0"));
        assert_eq!(parse_version("Python 3.9.13 :: ::").as_deref(), Some("3.9.13"));
        assert_eq!(parse_version("Python 2.7.18").as_deref(), Some("2.7.18"));
        assert_eq!(parse_version("").as_deref(), None);
    }

    #[test]
    fn version_keys_are_ordered() {
        assert!(version_key(Some("3.13.0")) > version_key(Some("3.9.13")));
        assert!(version_key(Some("3.9.13")) > version_key(Some("2.7.18")));
        assert_eq!(version_key(None), (0, 0, 0));
    }

    #[test]
    fn console_preference_prefers_pythonw() {
        let dir = tempfile::tempdir().unwrap();
        let py = dir.path().join("python.exe");
        let pyw = dir.path().join("pythonw.exe");
        std::fs::write(&py, b"").unwrap();
        std::fs::write(&pyw, b"").unwrap();

        let hidden = apply_console_preference(&py.to_string_lossy(), false);
        assert!(hidden.to_lowercase().ends_with("pythonw.exe"));

        let shown = apply_console_preference(&py.to_string_lossy(), true);
        assert!(shown.to_lowercase().ends_with("python.exe"));
    }

    #[test]
    fn console_preference_keeps_interpreter_when_pythonw_missing() {
        let dir = tempfile::tempdir().unwrap();
        let py = dir.path().join("my-python.exe");
        std::fs::write(&py, b"").unwrap();
        let out = apply_console_preference(&py.to_string_lossy(), false);
        assert!(out.to_lowercase().ends_with("my-python.exe"));
    }

    #[test]
    fn detects_at_least_one_real_python_on_dev_machine() {
        // 开发环境装有 Python；CI 若无 Python 则跳过
        let found = detect_python_installations();
        if found.is_empty() {
            eprintln!("skip: 本机未安装 Python");
            return;
        }
        assert!(
            Path::new(&found[0].path).exists(),
            "返回的解释器路径必须真实存在: {}",
            found[0].path
        );
    }

    #[test]
    fn resolve_interpreter_rejects_bad_config() {
        let app = AppItem {
            id: "1".into(),
            group_id: "default".into(),
            name: "x".into(),
            executable_path: "x.py".into(),
            arguments: None,
            working_directory: None,
            startup_window_style: crate::models::WindowStyle::Normal,
            launch_kind: 1,
            is_python_script: true,
            python_interpreter_path: Some("C:/__no_such_python__/python.exe".into()),
            show_console: false,
            run_as_admin: false,
            allow_multiple_instances: true,
            icon_path: None,
            sort_order: 0,
            created_at: String::new(),
            updated_at: String::new(),
        };
        assert!(resolve_interpreter_with_base(&app, None).is_err());
    }

    #[test]
    fn resolve_interpreter_accepts_existing_config() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("python.exe");
        std::fs::write(&fake, b"").unwrap();
        let app = AppItem {
            id: "1".into(),
            group_id: "default".into(),
            name: "x".into(),
            executable_path: "x.py".into(),
            arguments: None,
            working_directory: None,
            startup_window_style: crate::models::WindowStyle::Normal,
            launch_kind: 1,
            is_python_script: true,
            python_interpreter_path: Some(fake.to_string_lossy().to_string()),
            show_console: false,
            run_as_admin: false,
            allow_multiple_instances: true,
            icon_path: None,
            sort_order: 0,
            created_at: String::new(),
            updated_at: String::new(),
        };
        assert_eq!(
            resolve_interpreter_with_base(&app, None).unwrap(),
            fake.to_string_lossy()
        );
    }

    fn python_script_app(script: &Path) -> AppItem {
        AppItem {
            id: "1".into(),
            group_id: "default".into(),
            name: "x".into(),
            executable_path: script.to_string_lossy().to_string(),
            arguments: None,
            working_directory: None,
            startup_window_style: crate::models::WindowStyle::Normal,
            launch_kind: 1,
            is_python_script: true,
            python_interpreter_path: None,
            show_console: false,
            run_as_admin: false,
            allow_multiple_instances: true,
            icon_path: None,
            sort_order: 0,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn finds_bundled_python_in_install_root() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("python").join("python.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();

        assert_eq!(
            find_bundled_python(&[dir.path().to_path_buf()]).as_deref(),
            Some(exe.as_path())
        );
    }

    #[test]
    fn finds_bundled_python_under_resources() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir
            .path()
            .join("resources")
            .join("python")
            .join("python.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();

        assert_eq!(
            find_bundled_python(&[dir.path().to_path_buf()]).as_deref(),
            Some(exe.as_path())
        );
    }

    #[test]
    fn missing_bundled_python_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_bundled_python(&[dir.path().to_path_buf()]).is_none());
    }

    /// 系统安装的 Python（用户自己的环境）优先于内置解释器。
    #[test]
    fn system_python_preferred_over_bundled() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("job.py");
        std::fs::write(&script, b"print(1)").unwrap();
        let bundled = dir.path().join("python").join("python.exe");
        std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        std::fs::write(&bundled, b"").unwrap();

        let system = PythonInstallation {
            path: "C:/Python313/python.exe".to_string(),
            version: Some("3.13.0".to_string()),
            source: "py-launcher".to_string(),
            is_venv: false,
            relative_path: None,
        };
        let resolved = resolve_interpreter_in(
            &python_script_app(&script),
            Some(bundled.as_path()),
            None,
            &[system.clone()],
        )
        .unwrap();
        assert_eq!(resolved, system.path);
    }

    /// 系统没有任何 Python 时，回退到安装目录自带的解释器。
    #[test]
    fn bundled_python_used_when_no_system_python() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("job.py");
        std::fs::write(&script, b"print(1)").unwrap();
        let bundled = dir.path().join("python").join("python.exe");
        std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        std::fs::write(&bundled, b"").unwrap();

        let resolved = resolve_interpreter_in(
            &python_script_app(&script),
            Some(bundled.as_path()),
            None,
            &[],
        )
        .unwrap();
        assert_eq!(resolved, bundled.to_string_lossy());
    }

    /// 检测列表中内置解释器排在系统解释器之后（下拉框里作兜底选项）。
    #[test]
    fn detect_puts_bundled_python_last() {
        let dir = tempfile::tempdir().unwrap();
        let bundled = dir.path().join("python").join("python.exe");
        std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        std::fs::write(&bundled, b"").unwrap();
        let system = dir.path().join("system-python.exe");
        std::fs::write(&system, b"").unwrap();

        let found = detect_with_bundled(Some(bundled.as_path()));
        // 本机装有系统 Python 时，列表首位应是系统解释器，内置在最后
        if found.len() > 1 {
            assert_eq!(
                found.last().unwrap().path,
                bundled.to_string_lossy(),
                "内置解释器应垫底: {:?}",
                found.iter().map(|i| i.path.clone()).collect::<Vec<_>>()
            );
            assert_ne!(found[0].path, bundled.to_string_lossy());
        } else {
            // CI 无 Python 时列表只有内置
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].source, "bundled");
        }
        let _ = system;
    }

    #[test]
    fn relative_script_and_interpreter_are_resolved_against_base() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("pyTools").join("job.py");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&script, b"print(1)").unwrap();
        let interp = dir.path().join("python").join("python.exe");
        std::fs::create_dir_all(interp.parent().unwrap()).unwrap();
        std::fs::write(&interp, b"").unwrap();

        let mut app = python_script_app(&script);
        app.executable_path = "pyTools/job.py".into();
        app.python_interpreter_path = Some("python/python.exe".into());

        let resolved = resolve_interpreter_with_base(&app, Some(dir.path())).unwrap();
        assert_eq!(Path::new(&resolved), interp);
    }

    #[test]
    fn explicit_config_still_wins_over_bundled_python() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("job.py");
        std::fs::write(&script, b"print(1)").unwrap();
        let configured = dir.path().join("my-python.exe");
        std::fs::write(&configured, b"").unwrap();
        let bundled = dir.path().join("python").join("python.exe");
        std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        std::fs::write(&bundled, b"").unwrap();

        let mut app = python_script_app(&script);
        app.python_interpreter_path = Some(configured.to_string_lossy().to_string());

        let resolved =
            resolve_interpreter_in(&app, Some(bundled.as_path()), None, &[]).unwrap();
        assert_eq!(resolved, configured.to_string_lossy());
    }
}
