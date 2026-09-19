import { convertFileSrc, invoke } from '@tauri-apps/api/core';

/** 与 Rust `models::AppItem` 一一对应 */
/** 启动方式：与 Rust `LaunchKind` 一一对应 */
export const LaunchKind = {
  Program: 0,
  Python: 1,
  Command: 2,
  Web: 3,
} as const;

export type LaunchKindValue = (typeof LaunchKind)[keyof typeof LaunchKind];

export interface AppItem {
  id: string;
  group_id: string;
  name: string;
  executable_path: string;
  arguments?: string | null;
  working_directory?: string | null;
  startup_window_style: number;
  /** 启动方式，取值见 LaunchKind */
  launch_kind: number;
  is_python_script: boolean;
  python_interpreter_path?: string | null;
  show_console: boolean;
  run_as_admin: boolean;
  allow_multiple_instances: boolean;
  icon_path?: string | null;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

export interface AppGroup {
  id: string;
  name: string;
  sort_order: number;
  is_default: boolean;
}

/** 特殊分组 id：代表“全部”，展示所有已添加的应用（非数据库真实分组） */
export const ALL_GROUPS_ID = '__all__';

export interface LayoutInfo {
  app_id: string;
  pos_x: number;
  pos_y: number;
}

export interface PythonInstallation {
  path: string;
  version: string | null;
  source: string;
  is_venv: boolean;
  /** 内置解释器的可移植写法（相对程序安装目录），如 python\python.exe */
  relative_path?: string | null;
}

export interface LaunchResult {
  ok: boolean;
  message: string;
}

export interface PathInspection {
  /** 解析后的绝对路径：相对路径以程序运行目录（安装目录）为根展开 */
  resolved: string;
  exists: boolean;
  /** 输入是否为相对路径 */
  is_relative: boolean;
}

export interface UpdateSettings {
  /** 启动时自动检查更新 */
  enabled: boolean;
  /** 更新源：http(s) 地址或本地/共享路径 */
  source_url: string;
}

export interface UpdateCheckResult {
  /** ok / disabled / unconfigured / error */
  status: 'ok' | 'disabled' | 'unconfigured' | 'error';
  has_update: boolean;
  current_version: string;
  latest_version: string | null;
  notes: string | null;
  download_url: string | null;
  mandatory: boolean;
  /** 该版本已被用户忽略 */
  ignored: boolean;
  /** 结果来自上次检查的缓存 */
  from_cache: boolean;
  checked_at: number | null;
  source_url: string;
  message: string | null;
}

export interface UpdateState {
  current_version: string;
  settings: UpdateSettings;
  ignored_version: string | null;
  latest_version: string | null;
  checked_at: number | null;
}

export interface LaunchStats {
  recent: AppItem[];
  frequent: AppItem[];
}

export type AddAppRequest = Omit<
  AppItem,
  'id' | 'sort_order' | 'created_at' | 'updated_at'
>;

export type UpdateAppRequest = Omit<AppItem, 'created_at' | 'updated_at'>;

/** 把后端返回的绝对路径转换成 WebView 可加载的 URL */
export function iconUrl(path?: string | null): string | null {
  if (!path) return null;
  try {
    return convertFileSrc(path);
  } catch {
    return null;
  }
}

export function looksLikePythonScript(path: string): boolean {
  return /\.(py|pyw|pyc)$/i.test(path.trim());
}

/** 从路径推导默认名称 */
export function defaultNameFromPath(path: string): string {
  const trimmed = path.trim().replace(/[\\/]+$/, '');
  const base = trimmed.split(/[\\/]/).pop() ?? '';
  return base.replace(/\.[^.]+$/, '');
}

export const api = {
  // ---- 分组 ----
  getGroups: () => invoke<AppGroup[]>('get_groups'),
  createGroup: (name: string) => invoke<AppGroup>('create_group', { name }),
  deleteGroup: (groupId: string) =>
    invoke<void>('delete_group', { groupId }),
  renameGroup: (groupId: string, name: string) =>
    invoke<void>('rename_group', { groupId, name }),
  updateGroupSort: (groupId: string, sortOrder: number) =>
    invoke<void>('update_group_sort', { groupId, sortOrder }),

  // ---- 应用 ----
  getAppsByGroup: (groupId: string) =>
    invoke<AppItem[]>('get_apps_by_group', { groupId }),
  getAllApps: () => invoke<AppItem[]>('get_all_apps'),
  getAppById: (appId: string) => invoke<AppItem>('get_app_by_id', { appId }),
  addApp: (req: AddAppRequest) => invoke<AppItem>('add_app', { req }),
  updateApp: (req: UpdateAppRequest) => invoke<AppItem>('update_app', { req }),
  deleteApp: (appId: string) => invoke<void>('delete_app', { appId }),

  // ---- 启动与系统 ----
  launchApp: (appId: string) => invoke<LaunchResult>('launch_app', { appId }),
  launchAppAsAdmin: (appId: string) =>
    invoke<LaunchResult>('launch_app_as_admin', { appId }),
  getLaunchStats: () => invoke<LaunchStats>('get_launch_stats'),
  detectPython: () => invoke<PythonInstallation[]>('detect_python'),
  detectScriptVenv: (scriptPath: string) =>
    invoke<PythonInstallation | null>('detect_script_venv', { scriptPath }),
  extractIcon: (filePath: string, interpreter?: string | null) =>
    invoke<string | null>('extract_icon', { filePath, interpreter: interpreter ?? null }),
  openFileLocation: (path: string) =>
    invoke<void>('open_file_location', { path }),
  /** 解析路径（相对路径以程序运行目录为根）并判断是否存在 */
  inspectAppPath: (path: string) =>
    invoke<PathInspection>('inspect_app_path', { path }),

  // ---- 布局 ----
  saveLayout: (appId: string, posX: number, posY: number) =>
    invoke<void>('save_layout', { appId, posX, posY }),
  getLayout: (appId: string) => invoke<LayoutInfo | null>('get_layout', { appId }),
  getAllLayouts: () => invoke<LayoutInfo[]>('get_all_layouts'),
  clearGroupLayouts: (groupId: string) =>
    invoke<void>('clear_group_layouts', { groupId }),

  // ---- 配置 ----
  exportConfigToFile: (path: string) =>
    invoke<void>('export_config_to_file', { path }),
  importConfigFromFile: (path: string) =>
    invoke<number>('import_config_from_file', { path }),

  // ---- 版本更新 ----
  getUpdateState: () => invoke<UpdateState>('get_update_state'),
  saveUpdateSettings: (enabled: boolean, sourceUrl: string) =>
    invoke<void>('save_update_settings', { enabled, sourceUrl }),
  /** force = false 时遵守自动检查间隔，直接返回上次结果 */
  checkUpdate: (force = false) =>
    invoke<UpdateCheckResult>('check_update', { force }),
  setIgnoredUpdateVersion: (version: string | null) =>
    invoke<void>('set_ignored_update_version', { version }),
  openExternalUrl: (url: string) => invoke<void>('open_external_url', { url }),
};
