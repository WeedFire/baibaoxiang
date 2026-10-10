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
- **自动更新**（更新源与公钥写死在代码里，用户无感知；签名格式与 Tauri 官方 `tauri signer` 互通）：
  - **检查更新**：启动后自动检查（间隔 6 小时内复用缓存），也可在设置页手动检查。
  - **下载并安装**：提示条/设置页一键完成「下载 → 验签 → 安装」，带实时进度；开启「自动下载并安装」后全自动完成。
  - **清单**：Tauri 风格的 `latest.json`（`platforms` 按平台分发 + `signature` 验签）。
  - **安装方式**：`.msi` 交由 `msiexec`、`.exe` 以 `/S` 静默安装（安装程序启动后应用自动退出）；便携版（下载文件与当前程序同名）原地自替换，重启生效。
  - **安全**：`PUBLIC_KEY` 配置后仅安装验签通过的包，签名不符/缺失直接拒绝。
  - **发布**：推送 `v*` tag 即触发 GitHub Actions 自动打包、签名、生成 `latest.json` 并发布 Release。

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

## 发布更新（自动打包，推送 tag 即可）

更新源地址与验签公钥都**写死在代码里**（`src-tauri/src/services/update_service.rs` 的 `SOURCE_URL` 与 `PUBLIC_KEY`），用户侧无感知。

1. **准备签名密钥（一次性）**：用 Tauri 官方签名器生成密钥对
   ```bash
   npm run tauri signer generate -w <保存目录> -p <密码>
   ```
   - 把 `key.pub` 的完整内容填进 `update_service.rs` 的 `PUBLIC_KEY`；
   - 把 `key` 文件的完整内容存为 GitHub Secret `TAURI_SIGNING_PRIVATE_KEY`，
     密码存为 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。

2. **发版本**：改 `tauri.conf.json` 的 `version`，提交后打 tag 并推送，
   GitHub Actions（`.github/workflows/release.yml`）自动：
   lint + 单测 → 构建 Windows 安装包 → `tauri signer sign` 签名 →
   生成 `latest.json` → 发布 GitHub Release（含 `.exe` / `.msi` / `latest.json`）。
   ```bash
   git tag v1.0.2 && git push origin v1.0.2
   ```
   （也可在 Actions 页面用 `workflow_dispatch` 手动触发并指定 tag。）

3. 客户端启动后自动检查（或设置页「立即检查」），发现新版本即可「立即更新」，
   或开启「自动下载并安装」后全自动完成。

> 若 `PUBLIC_KEY` 留空，客户端会跳过验签（正式发布前务必填真实公钥）。

## 说明与限制

- 当前仅面向 Windows 平台（`process_launcher` / `icon_service` 的 Win32 实现位于 `cfg(target_os = "windows")`）。
- 配置导入导出使用 Rust 端文件读写，避免了 fs 插件作用域限制。
- 图标加载依赖 Tauri 的 asset 协议（`tauri.conf.json` 已启用 `assetProtocol` 且 Cargo 已开启 `protocol-asset`）。
