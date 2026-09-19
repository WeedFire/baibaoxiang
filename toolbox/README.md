# 百宝箱（Toolbox）

Windows 桌面应用与 **Python 脚本**快速启动管理工具。基于 Tauri 2 + React 19 构建，支持应用分组、网格/自由布局、图标提取、单实例控制、配置导入导出、自动检查更新与一键下载安装。

## 功能特性

- **应用启动**：以普通权限或管理员权限（`runas`）启动 `.exe` / `.lnk` / `.bat` / `.cmd` / `.ps1` 等程序，可指定启动窗口样式（正常 / 最大化 / 最小化）。
- **Python 脚本支持**：
  - 自动检测本机 Python 解释器（`py -0p`、常见安装目录、conda/miniforge、`PATH`），并自动发现脚本所在目录的虚拟环境（`.venv` / `venv` / `env`）。
  - 不显示控制台时自动改用同目录 `pythonw.exe`；显示控制台时用 `cmd /k` 包裹，运行结束保留输出窗口，方便查看 `print` 与报错。
  - 隐藏控制台时通过 `CREATE_NO_WINDOW` 静默执行。
- **图标提取**：调用 Windows Shell（`SHGetFileInfoW`）提取真实文件图标，缓存为 PNG 到应用数据目录，前端通过 asset 协议加载。
- **分组管理**：多分组、拖拽排序、内联重命名与删除。
- **布局模式**：自动网格布局，以及拖拽自由定位的手动布局（位置持久化）。
- **最近 / 常用**：仪表盘展示最近使用与常用应用。
- **单实例控制**：取消「允许多实例」时，目标程序已在运行则不再重复启动。
- **配置导入导出**：将全部分组与应用导出为 JSON 文件，或从 JSON 文件导入（事务写入，缺分组时回退默认分组）。
- **自动更新**（参考 [`F:\custTools\autoUpdate`](F:/custTools/autoUpdate) 的 forge-updater）：
  - **检查更新**：启动后自动检查（间隔 6 小时内复用缓存），也可在设置页手动检查；更新源支持 `http(s)`、本地路径与局域网共享。
  - **下载并安装**：提示条/设置页一键完成「下载 → ed25519 验签 → 安装」，带实时进度；设置为“自动下载并安装”后无需再确认。
  - **清单兼容**：Tauri updater 的 `latest.json`（`platforms` 按平台分发 + `signature` 验签）优先；也兼容 GitHub Releases API / 自建 JSON（只提供下载地址，此时仅「手动下载」）。
  - **安装方式**：`.msi` 交由 `msiexec`、`.exe` 以 `/S` 静默安装（安装程序启动后应用自动退出）；便携版（下载文件与当前程序同名）直接原地自替换，重启生效。
  - **安全**：设置里填写「更新包公钥」后，仅安装验签通过的包；清单缺少签名会直接拒绝安装。

## 技术栈

| 层 | 技术 |
| --- | --- |
| 前端 | React 19 + TypeScript + Vite 8 + Zustand |
| 桌面壳 | Tauri 2（`@tauri-apps/api`、`@tauri-apps/plugin-dialog`） |
| 后端 | Rust + rusqlite（SQLite，bundled）+ windows-rs + image |
| 校验 | Rust 测试（真实启动 Python 脚本、真实图标提取）、oxlint |

## 目录结构

```
toolbox/
├─ src/                      # 前端（React + TS）
│  ├─ lib/tauri.ts           # 类型化 Tauri 命令调用层
│  ├─ store/appStore.ts      # Zustand 全局状态
│  ├─ components/            # AppDialog / AppIcon / GroupTabs / LayoutCanvas / Toolbar / ContextMenu
│  └─ pages/                # Dashboard / Settings
└─ src-tauri/               # Rust 后端
   ├─ src/
   │  ├─ commands/          # Tauri 命令（app / group / system / layout）
   │  ├─ services/          # data_service / process_launcher / python_detector / icon_service / layout_engine
   │  ├─ db/mod.rs          # SQLite 初始化与 schema
   │  ├─ models/mod.rs      # 数据模型（与前端类型对应）
   │  └─ utils/             # 参数解析 args.rs / PNG 编码 png.rs / 路径辅助
   └─ tauri.conf.json
```

## 开发环境要求

- Node.js（建议 18+）
- Rust 工具链（stable）与 Tauri 2 的系统依赖（Windows 10/11 原生支持）
- 本机安装 Python（可选，仅在使用 Python 脚本功能时需要）

## 常用命令

```bash
# 安装依赖
npm install

# 开发模式（同时启动 Vite 与 Tauri 窗口）
npm run tauri dev

# 仅构建前端产物
npm run build

# 代码校验
npm run lint

# 打包为 Windows 安装程序 / 可执行文件
npm run tauri build
```

Rust 相关（在 `src-tauri` 目录下）：

```bash
# 编译检查
cargo check
# 运行全部测试（含真实启动 Python 脚本、真实图标提取）
cargo test
# 构建
cargo build
```

## 数据与配置位置

- 数据库与图标缓存：`<用户目录>/AppData/Roaming/com.toolbox.app/`
  - `toolbox.db`：全部分组、应用、布局与启动历史（更新设置与检查缓存都在 `user_settings` 表）
  - `icons/`：提取并缓存的应用图标 PNG
- 更新包下载目录：`<临时目录>/baibaoxiang-update/`

## 发布更新（客户端侧要准备什么）

1. 生成 ed25519 密钥对并妥善保存私钥（参考 forge-updater 的 `forge-up keygen`，私钥只放 CI Secret）。
2. 构建安装包后对每个产物签名，生成 Tauri 兼容的 `latest.json` 并随 Release 一起上传：

```json
{
  "version": "1.0.2",
  "notes": "本次更新内容",
  "platforms": {
    "windows-x86_64": {
      "signature": "<ed25519 签名 base64>",
      "url": "https://github.com/<owner>/<repo>/releases/download/v1.0.2/百宝箱_1.0.2_x64-setup.exe"
    }
  }
}
```

3. 把 `latest.json` 地址填到「设置 → 版本更新 → 更新源地址」，公钥填到「更新包公钥」并保存。
   此后启动会自动检查，发现新版本即可一键「立即更新」（或开启自动下载安装）。

> 更新源若使用 GitHub Releases API（`…/releases/latest`）这类只有下载地址的清单，
> 由于没有签名，客户端只提供「手动下载」，不会自动安装。

## 说明与限制

- 当前仅面向 Windows 平台（`process_launcher` / `icon_service` 的 Win32 实现位于 `cfg(target_os = "windows")`）。
- 配置导入导出使用 Rust 端文件读写，避免了 fs 插件作用域限制。
- 图标加载依赖 Tauri 的 asset 协议（`tauri.conf.json` 已启用 `assetProtocol` 且 Cargo 已开启 `protocol-asset`）。
