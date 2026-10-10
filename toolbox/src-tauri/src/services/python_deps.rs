//! Python 脚本依赖自动补齐。
//!
//! 启动 Python 脚本前，扫描脚本里的 `import`，用该解释器探测哪些模块缺失，
//! 再用 `<解释器> -m pip install` 自动装上，免得用户手动装包。
//!
//! 设计取舍：应用是「spawn 后不管」的启动方式（不捕获脚本输出），所以这里选
//! **启动前检测** 而不是解析运行期报错。整个过程尽力而为——失败只提示、不阻断启动。

use crate::models::AppItem;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

/// 探测脚本：模块名列表以 JSON 形式从 `argv[1]` 传入，缺失的模块以带标记的 JSON 打印。
const PROBE_CODE: &str = r#"
import importlib.util, json, sys
missing = []
for name in json.loads(sys.argv[1]):
    try:
        if importlib.util.find_spec(name) is None:
            missing.append(name)
    except Exception:
        missing.append(name)
print("__MISSING__" + json.dumps(missing))
"#;

/// 探测结果的行标记（避免被解释器自身的告警输出干扰）。
const MARKER: &str = "__MISSING__";

// ---------------- 纯逻辑（可单测） ----------------

/// 从 Python 源码里提取 `import` 的顶层模块名（去重、保序、忽略相对导入）。
pub fn extract_imports(source: &str) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for raw in source.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("import ") {
            for part in rest.split(',') {
                if let Some(name) = module_head(part) {
                    push_unique(&mut result, name);
                }
            }
        } else if let Some(rest) = line.strip_prefix("from ") {
            let rest = rest.trim_start();
            // `from . import x` / `from .mod import y` 是相对导入，不需要安装
            if rest.starts_with('.') {
                continue;
            }
            if let Some(name) = module_head(rest) {
                push_unique(&mut result, name);
            }
        }
    }
    result
}

/// 取一个 import 片段的顶层模块名：`a.b as c` → `a`。
fn module_head(part: &str) -> Option<String> {
    let token = part.split_whitespace().next()?;
    let head = token.split('.').next().unwrap_or("");
    if is_module_name(head) {
        Some(head.to_string())
    } else {
        None
    }
}

fn is_module_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if !list.contains(&value) {
        list.push(value);
    }
}

/// 模块名 → PyPI 包名（只列出名字不一致的常见项，其余同名）。
pub fn package_for_module(module: &str) -> &str {
    match module {
        "cv2" => "opencv-python",
        "PIL" => "pillow",
        "yaml" => "pyyaml",
        "bs4" => "beautifulsoup4",
        "sklearn" => "scikit-learn",
        "skimage" => "scikit-image",
        "dateutil" => "python-dateutil",
        "dotenv" => "python-dotenv",
        "serial" => "pyserial",
        "OpenSSL" => "pyopenssl",
        "jwt" => "pyjwt",
        "attr" => "attrs",
        "pkg_resources" => "setuptools",
        "win32com" | "win32api" | "win32con" | "win32gui" | "win32file" | "pythoncom" => "pywin32",
        other => other,
    }
}

/// 解析探测输出里的缺失模块（取最后一行带标记的 JSON）。
pub fn parse_missing(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix(MARKER))
        .and_then(|json| serde_json::from_str::<Vec<String>>(json).ok())
        .unwrap_or_default()
}

// ---------------- 探测与安装（runner 可注入以便测试） ----------------

/// 用解释器探测哪些模块缺失。
pub fn find_missing_modules(
    interpreter: &Path,
    modules: &[String],
    run: &mut dyn FnMut(&Path, &[String]) -> Result<String, String>,
) -> Result<Vec<String>, String> {
    if modules.is_empty() {
        return Ok(Vec::new());
    }
    let payload =
        serde_json::to_string(modules).map_err(|e| format!("序列化模块列表失败: {}", e))?;
    let args = vec!["-c".to_string(), PROBE_CODE.to_string(), payload];
    let stdout = run(interpreter, &args)?;
    Ok(parse_missing(&stdout))
}

/// 补齐脚本依赖：扫描 import → 探测缺失 → pip 安装；返回实际安装的包名。
pub fn ensure_script_dependencies_with(
    interpreter: &Path,
    script: &Path,
    run: &mut dyn FnMut(&Path, &[String]) -> Result<String, String>,
    notify: &mut dyn FnMut(&str),
) -> Result<Vec<String>, String> {
    let source = std::fs::read_to_string(script)
        .map_err(|e| format!("读取脚本失败（{}）: {}", script.display(), e))?;
    let modules = extract_imports(&source);
    if modules.is_empty() {
        return Ok(Vec::new());
    }

    let missing = find_missing_modules(interpreter, &modules, run)?;
    if missing.is_empty() {
        return Ok(Vec::new());
    }

    let mut packages: Vec<String> = Vec::new();
    for module in &missing {
        push_unique(&mut packages, package_for_module(module).to_string());
    }
    notify(&format!(
        "检测到缺少的依赖，正在自动安装：{}（使用 {}）",
        packages.join("、"),
        interpreter.display()
    ));

    let mut args = vec!["-m".to_string(), "pip".to_string(), "install".to_string()];
    args.extend(packages.iter().cloned());
    let output = run(interpreter, &args).map_err(|e| format!("自动安装依赖失败: {}", e))?;

    // 明确汇报安装位置：pip 装进的是「该解释器自己的 site-packages」
    if let Some(site) = site_packages_of(interpreter, run) {
        notify(&format!("依赖已安装到：{}", site));
    }
    if output.contains("user installation") {
        notify("注意：该 Python 的 site-packages 不可写，pip 已改装到用户目录（同一解释器仍可正常 import）");
    }
    Ok(packages)
}

/// 询问解释器「包会装到哪」的一次性代码（`sysconfig` 的 `purelib`）。
const SITE_CODE: &str = r#"
import sysconfig
print("__SITE__" + sysconfig.get_paths()["purelib"])
"#;
const SITE_MARKER: &str = "__SITE__";

/// 解析解释器上报的安装目录。
pub fn parse_site(output: &str) -> Option<String> {
    output
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix(SITE_MARKER))
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty())
}

/// 查询解释器的 site-packages 目录（用于向用户说明「依赖装到哪了」）。
pub fn site_packages_of(
    interpreter: &Path,
    run: &mut dyn FnMut(&Path, &[String]) -> Result<String, String>,
) -> Option<String> {
    let args = vec!["-c".to_string(), SITE_CODE.to_string()];
    run(interpreter, &args).ok().and_then(|out| parse_site(&out))
}

// ---------------- 真实执行 ----------------

/// 真实执行 Python（隐藏控制台）。
pub fn run_python(interpreter: &Path, args: &[String]) -> Result<String, String> {
    let mut cmd = Command::new(interpreter);
    cmd.args(args);
    // 清掉继承来的 PYTHONPATH：否则探测会用别的环境（如 IDE/终端注入的系统 site-packages）
    // 里的包，判定"不缺依赖"从而不安装，掩盖程序自带 Python 的真实缺失情况。
    cmd.env_remove("PYTHONPATH");
    // 强制 UTF-8 输出：中文 Windows 上 Python 走管道默认按 GBK 输出，按 UTF-8 解码会乱码。
    crate::utils::force_python_utf8(&mut cmd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd
        .output()
        .map_err(|e| format!("启动 Python 失败（{}）: {}", interpreter.display(), e))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        // 「No module named pip」通常是内置 Python 缺 pip（打包/安装不全），给出明确指引。
        if detail.to_lowercase().contains("no module named pip") {
            return Err(format!(
                "内置 Python 缺少 pip：请在「插件市场」重新安装「Python 运行环境」程序包（需包含 Lib/site-packages/pip）。详情: {}",
                tail_chars(detail, 200)
            ));
        }
        return Err(format!("Python 执行失败: {}", tail_chars(detail, 400)));
    }
    // stderr 也带上：pip 的「改装到用户目录」等警告都在 stderr，调用方要据此提示用户
    Ok(format!(
        "{}{}",
        stdout,
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn tail_chars(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        text.to_string()
    } else {
        format!("…{}", chars[chars.len() - max..].iter().collect::<String>())
    }
}

/// 为应用的 Python 脚本补齐依赖（启动前调用）。
///
/// 同一（解释器 + 脚本 + 修改时间）在一次会话内只检测一次；脚本改动后会重新检测。
pub fn ensure_dependencies_for_app(
    app: &AppItem,
    base: Option<&Path>,
    notify: &mut dyn FnMut(&str),
) -> Result<Vec<String>, String> {
    let interpreter = crate::services::python_detector::resolve_interpreter_with_base(app, base)?;
    let script = crate::utils::resolve_path_with(base, app.executable_path.trim());
    if !script.is_file() {
        return Err(format!("Python 脚本不存在: {}", script.display()));
    }

    let interpreter_path = PathBuf::from(&interpreter);
    // 检测过的脚本（同一解释器 + 同一版本）不再重复探测：后续启动直接放行，快速启动
    if is_checked(base, &interpreter_path, &script) {
        return Ok(Vec::new());
    }

    notify(&format!(
        "首次运行：正在检查 Python 依赖（{}）…",
        interpreter
    ));
    let installed =
        ensure_script_dependencies_with(&interpreter_path, &script, &mut run_python, notify)?;
    if installed.is_empty() {
        notify("依赖检查完成，后续启动将直接运行");
    }
    mark_checked(base, &interpreter_path, &script);
    Ok(installed)
}

/// 会话级缓存的键：解释器 + 脚本 + 脚本修改时间。
pub fn cache_key(interpreter: &Path, script: &Path) -> String {
    let mtime = std::fs::metadata(script)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}|{}|{}", interpreter.display(), script.display(), mtime)
}

fn checked_cache() -> &'static Mutex<HashSet<String>> {
    static CACHE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashSet::new()))
}

fn is_checked(base: Option<&Path>, interpreter: &Path, script: &Path) -> bool {
    let key = cache_key(interpreter, script);
    if checked_cache()
        .lock()
        .map(|cache| cache.contains(&key))
        .unwrap_or(false)
    {
        return true;
    }
    // 之前运行过（持久化在程序根目录）也直接放行：后续启动要快
    if let Some(base) = base {
        if load_disk_cache(base).contains(&key) {
            if let Ok(mut cache) = checked_cache().lock() {
                cache.insert(key);
            }
            return true;
        }
    }
    false
}

fn mark_checked(base: Option<&Path>, interpreter: &Path, script: &Path) {
    let key = cache_key(interpreter, script);
    if let Ok(mut cache) = checked_cache().lock() {
        cache.insert(key.clone());
    }
    if let Some(base) = base {
        let mut keys = load_disk_cache(base);
        if keys.insert(key) {
            save_disk_cache(base, &keys);
        }
    }
}

/// 依赖检测缓存文件（放在程序根目录，随程序走；删掉它会触发一次重新检测）。
const CACHE_FILE: &str = ".python-deps.json";

fn load_disk_cache(base: &Path) -> HashSet<String> {
    std::fs::read_to_string(base.join(CACHE_FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
        .map(|list| list.into_iter().collect())
        .unwrap_or_default()
}

fn save_disk_cache(base: &Path, keys: &HashSet<String>) {
    let list: Vec<&String> = keys.iter().collect();
    if let Ok(text) = serde_json::to_string_pretty(&list) {
        let _ = std::fs::write(base.join(CACHE_FILE), text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_script(source: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.py");
        std::fs::write(&path, source).unwrap();
        (dir, path)
    }

    #[test]
    fn extracts_top_level_imports() {
        let imports = extract_imports(
            "import os\nimport sys, json\nimport numpy as np\nfrom pandas.core import frame\n",
        );
        assert_eq!(imports, vec!["os", "sys", "json", "numpy", "pandas"]);
    }

    #[test]
    fn ignores_relative_imports_and_plain_text() {
        assert!(extract_imports("from . import x\nfrom .mod import y\n").is_empty());
        assert!(extract_imports("x = 1\nprint('import os')\n").is_empty());
    }

    #[test]
    fn maps_modules_to_pypi_packages() {
        assert_eq!(package_for_module("cv2"), "opencv-python");
        assert_eq!(package_for_module("PIL"), "pillow");
        assert_eq!(package_for_module("yaml"), "pyyaml");
        assert_eq!(package_for_module("bs4"), "beautifulsoup4");
        assert_eq!(package_for_module("win32com"), "pywin32");
        assert_eq!(package_for_module("requests"), "requests");
    }

    #[test]
    fn parses_probe_output() {
        assert_eq!(
            parse_missing("warning\n__MISSING__[\"numpy\",\"pandas\"]\n"),
            vec!["numpy", "pandas"]
        );
        assert!(parse_missing("no marker here").is_empty());
    }

    #[test]
    fn installs_missing_packages_via_pip() {
        let (_dir, script) = write_script("import numpy\nimport cv2\n");
        let interpreter = Path::new("C:/py/python.exe");
        let mut calls: Vec<Vec<String>> = Vec::new();
        let mut notices: Vec<String> = Vec::new();

        let mut run = |_py: &Path, args: &[String]| -> Result<String, String> {
            calls.push(args.to_vec());
            if args[0] != "-c" {
                Ok("Successfully installed numpy opencv-python".to_string())
            } else if args[1].contains("sysconfig") {
                // 安装后询问「装到哪了」
                Ok("__SITE__C:/py/Lib/site-packages\n".to_string())
            } else {
                Ok("__MISSING__[\"numpy\",\"cv2\"]\n".to_string())
            }
        };
        let installed = ensure_script_dependencies_with(
            interpreter,
            &script,
            &mut run,
            &mut |msg| notices.push(msg.to_string()),
        )
        .unwrap();

        assert_eq!(installed, vec!["numpy", "opencv-python"]);
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0][0], "-c");
        assert_eq!(calls[0].len(), 3);
        assert_eq!(
            calls[1],
            vec!["-m", "pip", "install", "numpy", "opencv-python"]
        );
        assert_eq!(calls[2][0], "-c");
        assert!(notices.iter().any(|n| n.contains("正在自动安装")));
    }

    #[test]
    fn skips_pip_when_nothing_is_missing() {
        let (_dir, script) = write_script("import os\n");
        let mut calls = 0;
        let mut run = |_py: &Path, _args: &[String]| -> Result<String, String> {
            calls += 1;
            Ok("__MISSING__[]\n".to_string())
        };
        let installed = ensure_script_dependencies_with(
            Path::new("C:/py/python.exe"),
            &script,
            &mut run,
            &mut |_| {},
        )
        .unwrap();

        assert!(installed.is_empty());
        assert_eq!(calls, 1, "没有缺失时不应调用 pip");
    }

    #[test]
    fn skips_probe_when_script_has_no_imports() {
        let (_dir, script) = write_script("print('hi')\n");
        let mut calls = 0;
        let mut run = |_py: &Path, _args: &[String]| -> Result<String, String> {
            calls += 1;
            Ok(String::new())
        };
        ensure_script_dependencies_with(
            Path::new("C:/py/python.exe"),
            &script,
            &mut run,
            &mut |_| {},
        )
        .unwrap();

        assert_eq!(calls, 0, "没有 import 时不应启动探测进程");
    }

    /// 端到端：用本机真实 Python 跑一次探测——标准库应存在、假模块应被判为缺失。
    /// 只探测，不会安装任何东西。
    #[test]
    fn real_probe_detects_missing_module() {
        let Some(py) = crate::services::python_detector::detect_python_installations()
            .into_iter()
            .next()
        else {
            eprintln!("skip: 本机未安装 Python");
            return;
        };

        let modules = vec!["sys".to_string(), "__bitmind_not_a_real_module__".to_string()];
        let missing =
            find_missing_modules(Path::new(&py.path), &modules, &mut run_python).unwrap();

        assert_eq!(missing, vec!["__bitmind_not_a_real_module__".to_string()]);
    }

    /// 回归：探测不能被启动器继承的 `PYTHONPATH` 影响。
    ///
    /// 否则脚本会用别的环境（IDE / 终端注入的 site-packages）里的包，导致探测判定
    /// 「不缺依赖」而永不安装，掩盖程序自带 Python 真实缺包的情况。
    #[test]
    fn probe_ignores_inherited_pythonpath() {
        let Some(py) = crate::services::python_detector::detect_python_installations()
            .into_iter()
            .next()
        else {
            eprintln!("skip: 本机未安装 Python");
            return;
        };

        // 造一个只存在于 PYTHONPATH 上的假模块
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("zz_bitmind_env_probe.py"), "").unwrap();

        let previous = std::env::var("PYTHONPATH").ok();
        std::env::set_var("PYTHONPATH", dir.path());
        let missing = find_missing_modules(
            Path::new(&py.path),
            &["zz_bitmind_env_probe".to_string()],
            &mut run_python,
        );
        match previous {
            Some(value) => std::env::set_var("PYTHONPATH", value),
            None => std::env::remove_var("PYTHONPATH"),
        }

        assert_eq!(
            missing.unwrap(),
            vec!["zz_bitmind_env_probe".to_string()],
            "探测应忽略继承的 PYTHONPATH"
        );
    }

    #[test]
    fn reports_pip_failure() {
        let (_dir, script) = write_script("import numpy\n");
        let mut run = |_py: &Path, args: &[String]| -> Result<String, String> {
            if args[0] == "-c" {
                Ok("__MISSING__[\"numpy\"]\n".to_string())
            } else {
                Err("pip 安装失败: 网络不可用".to_string())
            }
        };
        let err = ensure_script_dependencies_with(
            Path::new("C:/py/python.exe"),
            &script,
            &mut run,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(err.contains("自动安装依赖失败"), "{}", err);
    }

    #[test]
    fn parses_site_packages_path() {
        assert_eq!(
            parse_site("noise\n__SITE__C:/py/Lib/site-packages\n").as_deref(),
            Some("C:/py/Lib/site-packages")
        );
        assert_eq!(parse_site("nothing here"), None);
    }

    #[test]
    fn tells_where_packages_were_installed() {
        let (_dir, script) = write_script("import numpy\n");
        let mut notices: Vec<String> = Vec::new();
        let mut run = |_py: &Path, args: &[String]| -> Result<String, String> {
            if args[0] != "-c" {
                Ok("Successfully installed numpy".to_string())
            } else if args[1].contains("sysconfig") {
                Ok("__SITE__C:/py/Lib/site-packages\n".to_string())
            } else {
                Ok("__MISSING__[\"numpy\"]\n".to_string())
            }
        };
        ensure_script_dependencies_with(
            Path::new("C:/py/python.exe"),
            &script,
            &mut run,
            &mut |m| notices.push(m.to_string()),
        )
        .unwrap();

        assert!(
            notices
                .iter()
                .any(|n| n.contains("已安装到：C:/py/Lib/site-packages")),
            "应告诉用户装到了哪里：{:?}",
            notices
        );
    }

    #[test]
    fn remembers_checked_scripts_on_disk_so_next_launch_is_fast() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let script = base.join("main.py");
        std::fs::write(&script, "import os\n").unwrap();
        let interpreter = Path::new("C:/py/python.exe");

        assert!(!is_checked(Some(base), interpreter, &script));
        mark_checked(Some(base), interpreter, &script);

        assert!(
            base.join(CACHE_FILE).is_file(),
            "应写入持久化缓存，后续启动直接放行"
        );
        assert!(is_checked(Some(base), interpreter, &script));
    }
}
