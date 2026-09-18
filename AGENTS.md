# AGENTS.md — 百宝箱 (Toolbox)

## Project Context

Windows app launcher and management tool. Tauri 2.0 (Rust backend) + React + TypeScript + Vite. Target: Windows 10 1809+ / Windows 11 x64.

## Tech Stack

- **Backend**: Rust via Tauri 2.0, SQLite (rusqlite), windows-rs APIs
- **Frontend**: React 18+, TypeScript, Vite, zustand (state), CSS Modules/Tailwind
- **Build**: `cargo tauri build` (Rust + Vite bundle), outputs MSI or portable .exe
- **Target**: < 15MB installer, < 50MB RAM idle, < 1s cold start

## Directory Structure (planned)

```
toolbox/
├── src-tauri/          # Rust backend
│   ├── src/
│   │   ├── main.rs     # Tauri entry
│   │   ├── commands/   # IPC command handlers
│   │   ├── services/   # Business logic (data, icon, python, process, layout)
│   │   ├── models/     # Rust data models (serde)
│   │   ├── db/         # SQLite migrations
│   │   └── utils/
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/                # React frontend
│   ├── components/     # AppIcon, LayoutCanvas, GroupTabs
│   ├── pages/
│   ├── hooks/
│   ├── store/          # zustand
│   └── styles/
├── package.json
└── vite.config.ts
```

## Development Commands (once scaffolded)

- `npm install` — install frontend deps
- `cargo tauri dev` — start dev server (Vite + Rust hot reload)
- `cargo tauri build` — production build
- `cargo test` — run Rust unit tests
- `npm run lint` / `npm run typecheck` — frontend checks (add when scaffolding)

## Key Conventions

- **IPC boundary**: Frontend communicates with Rust exclusively through `invoke()` commands. No direct OS calls from JS.
- **Data flow**: Commands → Services → DAL (SQLite). Keep layers separate.
- **Python scripts**: Special handling — `is_python_script` flag triggers interpreter detection and invocation via Rust `Command`.
- **Layout modes**: Auto-grid (CSS, no storage) vs manual free-position (pixel coords in `layout_info` table, persisted).
- **Icons**: Extracted from .exe via `ExtractAssociatedIconW` (windows-rs), cached as PNG.
- **Settings**: Stored in `%APPDATA%\toolbox` (SQLite + JSON config).
- **Admin launch**: Via `ShellExecuteW` with `runas` verb, UAC-controlled.

## Testing

- Rust: unit tests in `src-tauri/src/` modules (`#[cfg(test)]`)
- Frontend: component tests (add test framework during scaffolding)
- Integration: manual testing on Windows (Tauri apps require Windows for full IPC testing)

## Gotchas

- WebView2 is required but pre-installed on Win10 1809+. Don't bundle it.
- Tauri 2.0 API differs from v1 — use `@tauri-apps/api/core` not `@tauri-apps/api/tauri`.
- SQLite migrations must be idempotent — use `IF NOT EXISTS` patterns.
- Drag-and-drop uses `pointer` events (not mouse) to avoid OS gesture conflicts.
- `tauri.conf.json` controls permissions — whitelist IPC commands explicitly.
