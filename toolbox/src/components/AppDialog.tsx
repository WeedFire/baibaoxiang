import { useCallback, useEffect, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import {
  api,
  ALL_GROUPS_ID,
  defaultNameFromPath,
  iconUrl,
  looksLikePythonScript,
  LaunchKind,
  type AppItem,
  type PathInspection,
  type PythonInstallation,
} from '../lib/tauri';
import { useAppStore } from '../store/appStore';
import './AppDialog.css';

interface AppDialogProps {
  mode: 'add' | 'edit';
  appId?: string;
  onClose: () => void;
  onSaved: () => void;
}

interface FormData {
  name: string;
  executable_path: string;
  arguments: string;
  working_directory: string;
  startup_window_style: number;
  /** 启动方式，见 LaunchKind */
  launch_kind: number;
  python_interpreter_path: string;
  show_console: boolean;
  run_as_admin: boolean;
  allow_multiple_instances: boolean;
  group_id: string;
}

const defaultForm: FormData = {
  name: '',
  executable_path: '',
  arguments: '',
  working_directory: '',
  startup_window_style: 0,
  launch_kind: LaunchKind.Program,
  python_interpreter_path: '',
  show_console: false,
  run_as_admin: false,
  allow_multiple_instances: true,
  group_id: 'default',
};

const PROGRAM_FILTERS = [
  { name: '程序与脚本', extensions: ['exe', 'lnk', 'bat', 'cmd', 'ps1', 'py', 'pyw', 'msc', 'cpl'] },
  { name: '所有文件', extensions: ['*'] },
];

const PYTHON_FILTERS = [{ name: 'Python 解释器', extensions: ['exe'] }];

function sourceLabel(source: string): string {
  switch (source) {
    case 'py-launcher':
      return '启动器';
    case 'venv':
      return '虚拟环境';
    case 'conda':
      return 'Conda';
    case 'programs-dir':
      return '安装目录';
    case 'path':
      return 'PATH';
    case 'bundled':
      return '内置';
    default:
      return source;
  }
}

export function AppDialog({ mode, appId, onClose, onSaved }: AppDialogProps) {
  const { groups, currentGroupId } = useAppStore();
  // “全部”视图下没有真实分组，新建应用默认归入第一个真实分组
  const defaultGroupId =
    currentGroupId && currentGroupId !== ALL_GROUPS_ID && groups.some((g) => g.id === currentGroupId)
      ? currentGroupId
      : groups[0]?.id ?? 'default';
  const [form, setForm] = useState<FormData>({
    ...defaultForm,
    group_id: defaultGroupId,
  });
  const [errors, setErrors] = useState<Partial<Record<keyof FormData, string>>>({});
  const [formError, setFormError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [loading, setLoading] = useState(mode === 'edit');
  const [pythonOptions, setPythonOptions] = useState<PythonInstallation[]>([]);
  const [detecting, setDetecting] = useState(false);
  const [showPythonDropdown, setShowPythonDropdown] = useState(false);
  const [iconPreview, setIconPreview] = useState<string | null>(null);
  const [pathInfo, setPathInfo] = useState<PathInspection | null>(null);

  const kind = form.launch_kind;
  const isPython = kind === LaunchKind.Python;
  // 只有“程序/脚本”类才有本地文件可以预览图标、校验是否存在
  const hasLocalTarget = kind === LaunchKind.Program || isPython;

  const pathLabel =
    kind === LaunchKind.Web
      ? '网页地址'
      : kind === LaunchKind.Command
        ? '命令'
        : isPython
          ? 'Python 脚本'
          : '路径';
  const pathPlaceholder =
    kind === LaunchKind.Web
      ? 'https://example.com（也可只写 example.com）'
      : kind === LaunchKind.Command
        ? 'ipconfig /all'
        : String.raw`C:\Program Files\App\app.exe 或 pyTools\main.py`;
  const pathHint =
    kind === LaunchKind.Web
      ? '用系统默认浏览器打开；省略协议头时自动按 https 处理'
      : kind === LaunchKind.Command
        ? '交给 cmd.exe 执行，支持管道与重定向；勾选「显示控制台」执行完保留窗口'
        : '支持相对路径，相对程序安装目录解析（如 pyTools\\excel_merge\\run_excel_merge.py）';
  const pathIcon = kind === LaunchKind.Web ? '🌐' : kind === LaunchKind.Command ? '💻' : '📦';
  const canBrowseFile = kind === LaunchKind.Program || isPython;

  useEffect(() => {
    if (mode === 'edit' && appId) {
      void loadApp(appId);
    }
  }, [mode, appId]);

  const loadApp = async (id: string) => {
    setLoading(true);
    try {
      const app: AppItem = await api.getAppById(id);
      setForm({
        name: app.name,
        executable_path: app.executable_path,
        arguments: app.arguments ?? '',
        working_directory: app.working_directory ?? '',
        startup_window_style: app.startup_window_style,
        // 老数据可能只有 is_python_script，这里补成 launch_kind
        launch_kind: app.launch_kind ?? (app.is_python_script ? LaunchKind.Python : LaunchKind.Program),
        python_interpreter_path: app.python_interpreter_path ?? '',
        show_console: app.show_console,
        run_as_admin: app.run_as_admin,
        allow_multiple_instances: app.allow_multiple_instances,
        group_id: app.group_id,
      });
    } catch (err) {
      setFormError(`读取应用失败：${String(err)}`);
    } finally {
      setLoading(false);
    }
  };

  // 路径变化后解析真实位置（相对路径以程序安装目录为根）并自动预览图标
  useEffect(() => {
    const target = form.executable_path.trim();
    if (!target || !hasLocalTarget) {
      setPathInfo(null);
      setIconPreview(null);
      return;
    }
    let cancelled = false;
    const timer = setTimeout(async () => {
      try {
        const info = await api.inspectAppPath(target);
        if (cancelled) return;
        setPathInfo(info);
        if (!info.exists) {
          setIconPreview(null);
          return;
        }
        // 统一用解析后的绝对路径取图标，避免后端重复解析
        const icon = await api.extractIcon(
          info.resolved,
          isPython ? form.python_interpreter_path : null,
        );
        if (!cancelled) setIconPreview(iconUrl(icon));
      } catch {
        if (!cancelled) {
          setPathInfo(null);
          setIconPreview(null);
        }
      }
    }, 350);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [
    form.executable_path,
    hasLocalTarget,
    isPython,
    form.python_interpreter_path,
  ]);

  const detectPython = useCallback(async (scriptPath: string) => {
    setDetecting(true);
    setShowPythonDropdown(true);
    try {
      // 优先展示脚本目录附近的虚拟环境，再展示系统解释器
      const venv = scriptPath ? await api.detectScriptVenv(scriptPath) : null;
      const all = await api.detectPython();
      const merged = venv ? [venv, ...all.filter((p) => p.path !== venv.path)] : all;
      setPythonOptions(merged);
      return merged;
    } catch (err) {
      console.error('Failed to detect Python:', err);
      setPythonOptions([]);
      return [];
    } finally {
      setDetecting(false);
    }
  }, []);

  const updateField = <K extends keyof FormData>(key: K, value: FormData[K]) => {
    setForm((prev) => ({ ...prev, [key]: value }));
    if (errors[key]) {
      setErrors((prev) => ({ ...prev, [key]: undefined }));
    }
  };

  const handlePathChange = (value: string) => {
    setForm((prev) => {
      const next = { ...prev, executable_path: value };
      const trimmed = value.trim();
      if (/^https?:\/\//i.test(trimmed)) {
        next.launch_kind = LaunchKind.Web;
      } else if (looksLikePythonScript(value)) {
        // 不强制打开控制台：GUI 脚本（自带窗口）应保持不勾选，避免弹出命令行页面；
        // 纯命令行脚本可在下方「显示控制台」中手动勾选以查看输出。
        next.launch_kind = LaunchKind.Python;
      }
      if (!prev.name.trim() && next.launch_kind !== LaunchKind.Command) {
        next.name = defaultNameFromPath(trimmed);
      }
      return next;
    });
    if (errors.executable_path) {
      setErrors((prev) => ({ ...prev, executable_path: undefined }));
    }
  };

  // Python 脚本若未指定解释器，自动选一个可用的
  useEffect(() => {
    if (!isPython || form.python_interpreter_path.trim()) return;
    let cancelled = false;
    void (async () => {
      const list = await detectPython(form.executable_path);
      if (!cancelled && list.length > 0) {
        // 内置解释器优先保存成相对写法，换台机器/换安装目录依然可用
        const preferred = list[0].relative_path || list[0].path;
        setForm((prev) =>
          prev.python_interpreter_path.trim()
            ? prev
            : { ...prev, python_interpreter_path: preferred },
        );
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [isPython, form.executable_path, form.python_interpreter_path, detectPython]);

  const browse = async (
    kind: 'program' | 'python' | 'directory',
    onPicked: (value: string) => void,
  ) => {
    try {
      const selected = await open({
        multiple: false,
        directory: kind === 'directory',
        filters: kind === 'program' ? PROGRAM_FILTERS : kind === 'python' ? PYTHON_FILTERS : undefined,
        title:
          kind === 'program' ? '选择程序或脚本' : kind === 'python' ? '选择 Python 解释器' : '选择工作目录',
      });
      const value = Array.isArray(selected) ? selected[0] : selected;
      if (value) onPicked(value);
    } catch (err) {
      console.error('选择文件失败:', err);
    }
  };

  const validate = (): boolean => {
    const newErrors: Partial<Record<keyof FormData, string>> = {};
    if (!form.name.trim()) newErrors.name = '请输入应用名称';
    if (!form.executable_path.trim()) newErrors.executable_path = '请输入路径';
    if (isPython && !form.python_interpreter_path.trim()) {
      newErrors.python_interpreter_path = 'Python 脚本需要指定解释器';
    }
    setErrors(newErrors);
    return Object.keys(newErrors).length === 0;
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setFormError(null);
    if (!validate()) return;

    setSaving(true);
    try {
      const payload = {
        group_id: form.group_id,
        name: form.name.trim(),
        executable_path: form.executable_path.trim(),
        arguments: form.arguments.trim() || null,
        working_directory: form.working_directory.trim() || null,
        startup_window_style: form.startup_window_style,
        launch_kind: kind,
        is_python_script: isPython,
        python_interpreter_path: isPython ? form.python_interpreter_path.trim() : null,
        show_console: form.show_console,
        run_as_admin: form.run_as_admin,
        allow_multiple_instances: form.allow_multiple_instances,
        icon_path: null,
      };

      if (mode === 'add') {
        await api.addApp(payload);
      } else if (appId) {
        await api.updateApp({
          ...payload,
          id: appId,
          sort_order: 0,
        });
      }
      onSaved();
      onClose();
    } catch (err) {
      setFormError(`保存失败：${String(err)}`);
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return (
      <div className="dialog-overlay">
        <div className="dialog dialog-loading">
          <div className="loading-spinner" />
          <p>加载中...</p>
        </div>
      </div>
    );
  }

  return (
    // 点击遮罩不再关闭：编辑过程中误点空白处会丢掉已填内容
    <div className="dialog-overlay">
      <div className="dialog">
        <div className="dialog-header">
          <h2>{mode === 'add' ? '添加应用' : '编辑应用'}</h2>
          <button className="dialog-close" onClick={onClose} title="关闭">
            ×
          </button>
        </div>

        <form className="dialog-form" onSubmit={handleSubmit}>
          {formError && <div className="form-banner-error">{formError}</div>}

          <div className="form-group">
            <label htmlFor="name">名称 *</label>
            <input
              id="name"
              type="text"
              value={form.name}
              onChange={(e) => updateField('name', e.target.value)}
              placeholder="例如：VS Code"
              className={errors.name ? 'error' : ''}
            />
            {errors.name && <span className="form-error">{errors.name}</span>}
          </div>

          <div className="form-group">
            <label htmlFor="kind">类型</label>
            <select
              id="kind"
              value={String(kind)}
              onChange={(e) => {
                const nextKind = Number(e.target.value);
                setForm((prev) => ({
                  ...prev,
                  launch_kind: nextKind,
                  // 切到非 Python 类型时清掉解释器，避免留下无效配置
                  python_interpreter_path:
                    nextKind === LaunchKind.Python ? prev.python_interpreter_path : '',
                }));
              }}
            >
              <option value={LaunchKind.Program}>程序 / 可执行文件</option>
              <option value={LaunchKind.Python}>Python 脚本</option>
              <option value={LaunchKind.Command}>CMD 命令</option>
              <option value={LaunchKind.Web}>网页</option>
            </select>
          </div>

          <div className="form-group">
            <label htmlFor="path">{pathLabel} *</label>
            <div className="input-row">
              {iconPreview ? (
                <img className="input-icon-preview" src={iconPreview} alt="" />
              ) : (
                <span className="input-icon-preview placeholder">{pathIcon}</span>
              )}
              <input
                id="path"
                type="text"
                value={form.executable_path}
                onChange={(e) => handlePathChange(e.target.value)}
                placeholder={pathPlaceholder}
                className={errors.executable_path ? 'error' : ''}
              />
              {canBrowseFile && (
                <button
                  type="button"
                  className="btn-detect"
                  onClick={() => browse('program', handlePathChange)}
                >
                  浏览
                </button>
              )}
            </div>
            {errors.executable_path && (
              <span className="form-error">{errors.executable_path}</span>
            )}
            {pathInfo &&
              (pathInfo.exists
                ? pathInfo.is_relative && (
                    <span className="form-hint">相对路径，解析为：{pathInfo.resolved}</span>
                  )
                : (
                    <span className="form-error">路径不存在：{pathInfo.resolved}</span>
                  ))}
            <span className="form-hint">{pathHint}</span>
          </div>

          <div className="form-group">
            <label htmlFor="args">参数</label>
            <input
              id="args"
              type="text"
              value={form.arguments}
              onChange={(e) => updateField('arguments', e.target.value)}
              placeholder='--flag value 或 "D:\my data\file.txt"'
            />
            <span className="form-hint">含空格的参数请用英文双引号包裹</span>
          </div>

          <div className="form-group">
            <label htmlFor="workdir">工作目录</label>
            <div className="input-row">
              <input
                id="workdir"
                type="text"
                value={form.working_directory}
                onChange={(e) => updateField('working_directory', e.target.value)}
                placeholder="留空则自动使用程序/脚本所在目录"
              />
              <button
                type="button"
                className="btn-detect"
                onClick={() =>
                  browse('directory', (v) => updateField('working_directory', v))
                }
              >
                浏览
              </button>
            </div>
          </div>

          <div className="form-row">
            <div className="form-group">
              <label htmlFor="group">分组</label>
              <select
                id="group"
                value={form.group_id}
                onChange={(e) => updateField('group_id', e.target.value)}
              >
                {groups.map((g) => (
                  <option key={g.id} value={g.id}>
                    {g.name}
                  </option>
                ))}
              </select>
            </div>

            <div className="form-group">
              <label htmlFor="window">窗口样式</label>
              <select
                id="window"
                value={form.startup_window_style}
                onChange={(e) =>
                  updateField('startup_window_style', Number(e.target.value))
                }
              >
                <option value={0}>正常</option>
                <option value={1}>最大化</option>
                <option value={2}>最小化</option>
              </select>
            </div>
          </div>

          <div className="form-checkboxes">
            {isPython && (
              <div className="form-group nested">
                <label htmlFor="python">Python 解释器 *</label>
                <div className="input-row">
                  <input
                    id="python"
                    type="text"
                    value={form.python_interpreter_path}
                    onChange={(e) =>
                      updateField('python_interpreter_path', e.target.value)
                    }
                    placeholder="python.exe 的完整路径，或相对路径"
                    className={errors.python_interpreter_path ? 'error' : ''}
                  />
                  <button
                    type="button"
                    className="btn-detect"
                    onClick={() => browse('python', (v) => updateField('python_interpreter_path', v))}
                  >
                    浏览
                  </button>
                  <button
                    type="button"
                    className="btn-detect"
                    onClick={() => void detectPython(form.executable_path)}
                    disabled={detecting}
                  >
                    {detecting ? '检测中...' : '检测'}
                  </button>
                </div>
                {errors.python_interpreter_path && (
                  <span className="form-error">{errors.python_interpreter_path}</span>
                )}
                <span className="form-hint">
                  可填相对路径，同样相对程序安装目录解析（如 python\python.exe）
                </span>

                {showPythonDropdown && (
                  <div className="python-dropdown">
                    {pythonOptions.length === 0 && !detecting && (
                      <div className="python-empty">未检测到 Python，请手动浏览选择</div>
                    )}
                    {pythonOptions.map((opt) => (
                      <button
                        key={opt.path}
                        type="button"
                        className="python-option"
                        onClick={() => {
                          updateField(
                            'python_interpreter_path',
                            opt.relative_path || opt.path,
                          );
                          setShowPythonDropdown(false);
                        }}
                      >
                        <span className="python-option-path">{opt.path}</span>
                        <span className="python-option-meta">
                          {opt.version ? `Python ${opt.version}` : '版本未知'}
                          {' · '}
                          {sourceLabel(opt.source)}
                        </span>
                      </button>
                    ))}
                  </div>
                )}
              </div>
            )}

            {/* 网页交给浏览器打开，进程相关的选项对它没有意义 */}
            {kind !== LaunchKind.Web && (
              <>
                <label className="checkbox-label">
                  <input
                    type="checkbox"
                    checked={form.run_as_admin}
                    onChange={(e) => updateField('run_as_admin', e.target.checked)}
                  />
                  <span>以管理员身份运行</span>
                </label>

                <label className="checkbox-label">
                  <input
                    type="checkbox"
                    checked={form.allow_multiple_instances}
                    onChange={(e) =>
                      updateField('allow_multiple_instances', e.target.checked)
                    }
                  />
                  <span>允许多实例（取消后已在运行则不重复启动）</span>
                </label>
              </>
            )}

            {/* 只有脚本/命令需要控制台：勾选后执行完保留窗口方便看输出 */}
            {(isPython || kind === LaunchKind.Command) && (
              <label className="checkbox-label">
                <input
                  type="checkbox"
                  checked={form.show_console}
                  onChange={(e) => updateField('show_console', e.target.checked)}
                />
                <span>
                  {kind === LaunchKind.Command
                    ? '保留控制台窗口（执行完不关闭，便于查看输出）'
                    : '显示控制台（命令行脚本勾选以查看输出；GUI 脚本保持不勾选，否则会多弹一个黑窗）'}
                </span>
              </label>
            )}
          </div>

          <div className="dialog-footer">
            <button type="button" className="btn-cancel" onClick={onClose}>
              取消
            </button>
            <button type="submit" className="btn-save" disabled={saving}>
              {saving ? '保存中...' : '保存'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
