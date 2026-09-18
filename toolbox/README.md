# 百宝箱（Toolbox）

Windows 桌面应用与 **Python 脚本**快速启动管理工具。基于 Tauri 2 + React 19 构建，支持应用分组、网格/自由布局、图标提取、单实例控制、配置导入导出。

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
  - `toolbox.db`：全部分组、应用、布局与启动历史
  - `icons/`：提取并缓存的应用图标 PNG

## 说明与限制

- 当前仅面向 Windows 平台（`process_launcher` / `icon_service` 的 Win32 实现位于 `cfg(target_os = "windows")`）。
- 配置导入导出使用 Rust 端文件读写，避免了 fs 插件作用域限制。
- 图标加载依赖 Tauri 的 asset 协议（`tauri.conf.json` 已启用 `assetProtocol` 且 Cargo 已开启 `protocol-asset`）。
