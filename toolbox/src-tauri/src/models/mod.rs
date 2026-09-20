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

/// 应用的启动方式，与数据库 `launch_kind` 列及前端“类型”下拉框一一对应
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchKind {
    /// 程序 / 可执行文件 / 批处理（默认）
    Program = 0,
    /// Python 脚本，需要解释器
    Python = 1,
    /// CMD 命令，交给 cmd.exe 执行
    Command = 2,
    /// 网页地址，交给系统默认浏览器打开
    Web = 3,
}

impl LaunchKind {
    pub fn from_i32(v: i32) -> Self {
        match v {
            1 => LaunchKind::Python,
            2 => LaunchKind::Command,
            3 => LaunchKind::Web,
            _ => LaunchKind::Program,
        }
    }

    /// 取应用的启动方式；老数据只有 `is_python_script` 时用它兜底。
    pub fn of(app: &AppItem) -> Self {
        match LaunchKind::from_i32(app.launch_kind) {
            LaunchKind::Program if app.is_python_script => LaunchKind::Python,
            other => other,
        }
    }

    pub fn is_python(&self) -> bool {
        matches!(self, LaunchKind::Python)
    }
}

impl Default for LaunchKind {
    fn default() -> Self {
        LaunchKind::Program
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
    /// 启动方式，取值见 [`LaunchKind`]；旧数据缺失时按 0（程序）处理
    #[serde(default)]
    pub launch_kind: i32,
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
    /// 启动方式，取值见 [`LaunchKind`]
    #[serde(default)]
    pub launch_kind: i32,
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
    /// 启动方式，取值见 [`LaunchKind`]
    #[serde(default)]
    pub launch_kind: i32,
    pub is_python_script: bool,
    pub python_interpreter_path: Option<String>,
    pub show_console: bool,
    pub run_as_admin: bool,
    pub allow_multiple_instances: bool,
    pub icon_path: Option<String>,
    pub sort_order: i32,
}

/// GitHub Releases 等接口里的发布附件
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateAsset {
    pub browser_download_url: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

/// Tauri `latest.json` 中单个平台的资产：下载地址 + ed25519 签名。
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdatePlatformAsset {
    /// ed25519 签名（base64 的 64 字节）。GitHub 风格清单没有这一项。
    #[serde(default, alias = "sig")]
    pub signature: Option<String>,
    /// 该平台安装包地址
    #[serde(alias = "download_url", alias = "browser_download_url")]
    pub url: String,
}

/// 更新源返回的清单（JSON），字段名尽量宽容，兼容常见的几种写法：
/// - Tauri updater 的 `latest.json`（`platforms` + `signature`，可自动下载安装）
/// - GitHub Releases API / 自建 JSON（`url` 或 `assets`，只能下载后手动安装）
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
    /// 发布附件（GitHub Releases 会带这一项）
    #[serde(default)]
    pub assets: Option<Vec<UpdateAsset>>,
    /// 发布时间（RFC3339，latest.json 会带）
    #[serde(default, alias = "pubDate", alias = "published_at")]
    pub pub_date: Option<String>,
    /// 平台资产表：键为 `windows-x86_64` 等平台串（latest.json 会带）
    #[serde(default)]
    pub platforms: Option<std::collections::BTreeMap<String, UpdatePlatformAsset>>,
}

impl UpdateManifest {
    /// 下载地址：优先第一个发布附件（GitHub 上传的安装包），其次清单里的 url。
    pub fn download_url(&self) -> Option<String> {
        let preferred = self
            .assets
            .as_ref()
            .and_then(|assets| {
                assets
                    .iter()
                    .find_map(|asset| asset.browser_download_url.clone())
            })
            .or_else(|| self.url.clone());

        preferred
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }
}

/// 更新检查设置（更新源地址与公钥写死在代码里，不再暴露给用户）。
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateSettings {
    /// 启动时自动检查
    pub enabled: bool,
    /// 发现新版本后自动下载并安装（无需手动点按钮）
    #[serde(default)]
    pub auto_install: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_install: false,
        }
    }
}

/// 解析出的、适用于本机的更新资产
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResolvedUpdateAsset {
    pub url: String,
    /// ed25519 签名（base64）；GitHub 风格清单为 None
    pub signature: Option<String>,
    /// 是否来自 `latest.json` 的平台表（true 表示按平台精确匹配）
    pub platform_specific: bool,
}

/// 下载/安装进度，通过 `update://progress` 事件推给前端
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateProgress {
    /// preparing / downloading / verifying / installing / done
    pub stage: String,
    /// 已下载字节
    pub downloaded: u64,
    /// 总字节（未知时为 0）
    pub total: u64,
    /// 阶段说明（可选）
    pub message: Option<String>,
}

/// 自动下载安装的结果
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateInstallResult {
    /// 是否已成功应用（便携版自替换成功，或安装程序已启动）
    pub installed: bool,
    /// 更新包在本地的路径
    pub file_path: String,
    /// 面向用户的说明
    pub message: String,
    /// 是否需要重启应用才能生效
    pub need_restart: bool,
    /// 是否已启动外部安装程序（此时应用会自动退出以让安装程序替换文件）
    pub installer_started: bool,
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

// ---------------- 插件市场 ----------------

/// 插件种类，对应 `marketplace.json` 里的 `kind` 字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    /// Python 脚本：下载到 `<根>/pyTools/<id>/`，并自动登记为应用
    PythonScript,
    /// 程序包（python 运行环境的一部分）：下载到 `<根>/<id>/`，仅放置文件
    Program,
    /// 依赖包：下载到 `<根>/python/Lib/site-packages/`，仅放置文件
    Dependency,
}

impl PluginKind {
    /// 中文名，用于前端展示。
    #[allow(dead_code)]
    pub fn label(&self) -> &'static str {
        match self {
            PluginKind::PythonScript => "Python 脚本",
            PluginKind::Program => "程序包",
            PluginKind::Dependency => "依赖包",
        }
    }
}

/// 插件市场清单里的单个插件条目。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct MarketplacePlugin {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    pub kind: PluginKind,
    pub download_url: String,
    /// 覆盖归档字节的 ed25519/minisign 签名（base64）；留空表示不校验
    #[serde(default)]
    pub signature: Option<String>,
    /// 可选图标（PNG 地址），下载后作为应用图标
    #[serde(default)]
    pub icon_url: Option<String>,
    /// 归档内的入口文件名（用于自动添加应用）
    #[serde(default)]
    pub entry: String,
    /// 自动添加应用时的启动方式（0 程序 / 1 Python / 2 命令 / 3 网页）
    #[serde(default)]
    pub launch_kind: i32,
    /// Python 类应用的解释器相对路径（如 `python/python.exe`）
    #[serde(default)]
    pub interpreter: String,
    /// 是否自动添加为应用；缺省时仅 Python 脚本自动添加
    #[serde(default)]
    pub auto_add: Option<bool>,
    /// 可选的 Python 版本要求，仅展示用
    #[serde(default)]
    #[allow(dead_code)]
    pub python_requirement: Option<String>,
}

/// 插件市场清单（`marketplace.json`）。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct MarketplaceManifest {
    #[serde(default)]
    #[allow(dead_code)]
    pub schema: i64,
    #[serde(default)]
    #[allow(dead_code)]
    pub updated_at: Option<String>,
    pub plugins: Vec<MarketplacePlugin>,
}

/// 安装一个插件后的结果（命令返回给前端）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct MarketplaceInstallResult {
    pub installed: bool,
    /// 是否自动登记了应用
    pub added_app: bool,
    /// 自动登记的应用 id（未登记时为 None）
    pub app_id: Option<String>,
    /// 实际安装目录（绝对路径）
    pub install_dir: String,
    pub message: String,
}

/// 插件下载/安装进度（通过 `marketplace://progress` 事件推给前端）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MarketplaceProgress {
    /// preparing / downloading / verifying / installing / done
    pub stage: String,
    pub downloaded: u64,
    pub total: u64,
    pub message: Option<String>,
}

/// 前端展示用的插件视图（在清单基础上补充「已安装」状态）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct MarketplacePluginView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: String,
    /// `python_script` / `program` / `dependency`
    pub kind: String,
    pub download_url: String,
    pub icon_url: Option<String>,
    pub entry: String,
    pub launch_kind: i32,
    pub interpreter: String,
    pub auto_add: bool,
    pub installed: bool,
    pub installed_version: Option<String>,
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
