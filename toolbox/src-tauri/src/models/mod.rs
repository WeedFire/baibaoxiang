use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// 0=Normal, 1=Maximized, 2=Minimized —— 与数据库 INTEGER 列及前端下拉框一一对应
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowStyle {
    Normal = 0,
    Maximized = 1,
    Minimized = 2,
}

impl WindowStyle {
    pub fn from_i32(v: i32) -> Self {
        match v {
            1 => WindowStyle::Maximized,
            2 => WindowStyle::Minimized,
            _ => WindowStyle::Normal,
        }
    }
}

impl Default for WindowStyle {
    fn default() -> Self {
        WindowStyle::Normal
    }
}

impl Serialize for WindowStyle {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i32(*self as i32)
    }
}

impl<'de> Deserialize<'de> for WindowStyle {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = WindowStyle;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "an integer between 0 and 2")
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<WindowStyle, E> {
                Ok(WindowStyle::from_i32(v as i32))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<WindowStyle, E> {
                Ok(WindowStyle::from_i32(v as i32))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<WindowStyle, E> {
                // 兼容早期版本序列化出的 "Normal"/"Maximized"/"Minimized"
                Ok(match v {
                    "Maximized" => WindowStyle::Maximized,
                    "Minimized" => WindowStyle::Minimized,
                    _ => WindowStyle::Normal,
                })
            }
        }
        deserializer.deserialize_any(V)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppItem {
    pub id: String,
    pub group_id: String,
    pub name: String,
    pub executable_path: String,
    pub arguments: Option<String>,
    pub working_directory: Option<String>,
    pub startup_window_style: WindowStyle,
    pub is_python_script: bool,
    pub python_interpreter_path: Option<String>,
    pub show_console: bool,
    pub run_as_admin: bool,
    pub allow_multiple_instances: bool,
    pub icon_path: Option<String>,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppGroup {
    pub id: String,
    pub name: String,
    pub sort_order: i32,
    pub is_default: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LayoutInfo {
    pub app_id: String,
    pub pos_x: f64,
    pub pos_y: f64,
}

/// 一个可被使用的 Python 解释器。`path` 一定是可直接 spawn 的绝对路径。
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PythonInstallation {
    pub path: String,
    /// 例如 "3.13.0"，检测失败时为 None
    pub version: Option<String>,
    /// 发现途径：py-launcher / registry / programs-dir / conda / path / venv / bundled
    pub source: String,
    pub is_venv: bool,
    /// 内置解释器相对「程序运行目录」的写法（如 `python\python.exe`）。
    /// 保存这种写法可以让配置与安装位置无关；其它来源为 None。
    #[serde(default)]
    pub relative_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LaunchResult {
    pub ok: bool,
    /// 面向用户的可读描述，失败时说明原因
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AddAppRequest {
    pub group_id: String,
    pub name: String,
    pub executable_path: String,
    pub arguments: Option<String>,
    pub working_directory: Option<String>,
    pub startup_window_style: i32,
    pub is_python_script: bool,
    pub python_interpreter_path: Option<String>,
    pub show_console: bool,
    pub run_as_admin: bool,
    pub allow_multiple_instances: bool,
    pub icon_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateAppRequest {
    pub id: String,
    pub group_id: String,
    pub name: String,
    pub executable_path: String,
    pub arguments: Option<String>,
    pub working_directory: Option<String>,
    pub startup_window_style: i32,
    pub is_python_script: bool,
    pub python_interpreter_path: Option<String>,
    pub show_console: bool,
    pub run_as_admin: bool,
    pub allow_multiple_instances: bool,
    pub icon_path: Option<String>,
    pub sort_order: i32,
}

/// 更新源返回的清单（JSON），字段名尽量宽容，兼容常见的几种写法。
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateManifest {
    /// 最新版本号，如 "1.0.2"（允许 "v1.0.2"）
    #[serde(alias = "latest_version", alias = "tag_name", alias = "latestVersion")]
    pub version: String,
    /// 更新说明，纯文本展示
    #[serde(default, alias = "changelog", alias = "body", alias = "description")]
    pub notes: Option<String>,
    /// 下载页/安装包地址，点击后交给系统浏览器打开
    #[serde(
        default,
        alias = "download_url",
        alias = "downloadUrl",
        alias = "link",
        alias = "html_url"
    )]
    pub url: Option<String>,
    /// 是否强制更新（强制时前端不提供“忽略/稍后”）
    #[serde(default)]
    pub mandatory: bool,
}

/// 更新检查设置
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateSettings {
    /// 启动时自动检查
    pub enabled: bool,
    /// 更新源：http(s) 地址，或本地/局域网共享路径
    pub source_url: String,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            source_url: String::new(),
        }
    }
}

/// 更新检查结果，直接返回给前端
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateCheckResult {
    /// ok / disabled / unconfigured / error
    pub status: String,
    /// 是否有可用新版本（已忽略的版本不算）
    pub has_update: bool,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub notes: Option<String>,
    pub download_url: Option<String>,
    pub mandatory: bool,
    /// 该版本是否已被用户忽略
    pub ignored: bool,
    /// 结果是否来自上次检查的缓存
    pub from_cache: bool,
    /// 上次检查时间（unix 秒）
    pub checked_at: Option<i64>,
    pub source_url: String,
    /// 出错时的可读说明
    pub message: Option<String>,
}

/// 设置页需要的完整更新状态
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateState {
    pub current_version: String,
    pub settings: UpdateSettings,
    pub ignored_version: Option<String>,
    /// 最近一次检查到的版本
    pub latest_version: Option<String>,
    pub checked_at: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_style_serializes_as_integer() {
        assert_eq!(serde_json::to_string(&WindowStyle::Normal).unwrap(), "0");
        assert_eq!(serde_json::to_string(&WindowStyle::Maximized).unwrap(), "1");
        assert_eq!(serde_json::to_string(&WindowStyle::Minimized).unwrap(), "2");
    }

    #[test]
    fn window_style_deserializes_from_multiple_forms() {
        assert_eq!(
            serde_json::from_str::<WindowStyle>("2").unwrap(),
            WindowStyle::Minimized
        );
        assert_eq!(
            serde_json::from_str::<WindowStyle>("\"Maximized\"").unwrap(),
            WindowStyle::Maximized
        );
        assert_eq!(
            serde_json::from_str::<WindowStyle>("99").unwrap(),
            WindowStyle::Normal
        );
    }
}
