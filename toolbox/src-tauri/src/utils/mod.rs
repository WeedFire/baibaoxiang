pub mod args;
pub mod png;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 「程序运行目录」（安装目录 / exe 所在目录），作为相对路径的根。
/// 由 `lib.rs` 启动时登记；未登记时（如单元测试）相对路径保持原样。
static APP_BASE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 登记程序自身所在的目录。
pub fn set_app_base_dir(dir: PathBuf) {
    let _ = APP_BASE_DIR.set(dir);
}

/// 程序自身所在的目录，未登记时为 None。
pub fn app_base_dir() -> Option<&'static Path> {
    APP_BASE_DIR.get().map(|dir| dir.as_path())
}

/// 去掉首尾空白与成对引号：用户常直接从资源管理器复制带引号的路径。
fn clean_input(input: &str) -> String {
    input.trim().trim_matches('"').trim().to_string()
}

/// 以 `base` 为根解析相对路径：
/// - 绝对路径原样返回；
/// - `base` 未提供（或为空）时原样返回；
/// - 相对路径优先按 `base` 拼接；只有当程序目录下不存在、而原路径在当前工作目录下确实存在时，
///   才保留原路径，兼容历史数据。
pub fn resolve_path_with(base: Option<&Path>, input: &str) -> PathBuf {
    let raw = clean_input(input);
    let candidate = Path::new(&raw);
    if candidate.is_absolute() {
        return candidate.to_path_buf();
    }
    let Some(base) = base else {
        return candidate.to_path_buf();
    };
    let joined = base.join(candidate);
    if joined.exists() || !candidate.exists() {
        joined
    } else {
        candidate.to_path_buf()
    }
}

/// 以「程序运行目录」为根解析相对路径。
pub fn resolve_path(input: &str) -> PathBuf {
    resolve_path_with(app_base_dir(), input)
}

/// 判断输入是否是相对路径（供界面提示使用）。
pub fn is_relative_path(input: &str) -> bool {
    let raw = clean_input(input);
    !raw.is_empty() && !Path::new(&raw).is_absolute()
}

/// 归一化路径分隔符为正斜杠，便于前端与比较使用。
pub fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

/// 判断路径是否指向 Python 脚本。
pub fn looks_like_python_script(path: &str) -> bool {
    matches!(
        Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("py") | Some("pyw") | Some("pyc")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_backslashes() {
        assert_eq!(normalize_path(r"C:\a\b"), "C:/a/b");
    }

    #[test]
    fn detects_python_extensions() {
        assert!(looks_like_python_script(r"D:\x\main.py"));
        assert!(looks_like_python_script(r"D:\x\main.PYW"));
        assert!(!looks_like_python_script(r"D:\x\main.exe"));
        assert!(!looks_like_python_script(r"D:\x\noext"));
    }

    #[test]
    fn resolves_relative_path_against_base() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("pyTools").join("main.py");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&script, b"").unwrap();

        assert_eq!(resolve_path_with(Some(dir.path()), "pyTools/main.py"), script);
        assert_eq!(resolve_path(&script.to_string_lossy()), script);
    }

    #[test]
    fn absolute_path_is_returned_as_is() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("a.exe");
        assert_eq!(
            resolve_path_with(Some(Path::new("C:/__unrelated__")), &exe.to_string_lossy()),
            exe
        );
    }

    #[test]
    fn unresolved_relative_path_still_anchors_to_base() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_path_with(Some(dir.path()), "missing/thing.py"),
            dir.path().join("missing").join("thing.py")
        );
    }

    #[test]
    fn strips_quotes_and_handles_missing_base() {
        assert_eq!(
            resolve_path_with(None, "  \"a/b.py\"  "),
            PathBuf::from("a/b.py")
        );
    }

    #[test]
    fn detects_relative_inputs() {
        assert!(is_relative_path("pyTools/main.py"));
        assert!(!is_relative_path("   "));
        #[cfg(target_os = "windows")]
        assert!(!is_relative_path(r"C:\tools\main.py"));
    }
}
