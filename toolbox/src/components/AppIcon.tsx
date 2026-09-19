import { useState } from 'react';
import { confirm } from '@tauri-apps/plugin-dialog';
import { api, iconUrl, LaunchKind, type AppItem } from '../lib/tauri';
import { ContextMenu, type ContextMenuItem } from './ContextMenu';
import './AppIcon.css';

interface AppIconProps {
  app: AppItem;
  isLocked: boolean;
  onRefresh: () => void;
  onEdit: () => void;
}

type LaunchState = 'idle' | 'launching' | 'success' | 'error';

export function AppIcon({ app, isLocked, onRefresh, onEdit }: AppIconProps) {
  const [isHovered, setIsHovered] = useState(false);
  const [launchState, setLaunchState] = useState<LaunchState>('idle');
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);

  const icon = iconUrl(app.icon_path);
  const kind = app.launch_kind ?? (app.is_python_script ? LaunchKind.Python : LaunchKind.Program);
  const isPython = kind === LaunchKind.Python;
  // 网页与命令没有本地文件，不能“打开文件位置”
  const hasLocalTarget = kind === LaunchKind.Program || isPython;
  const placeholder =
    kind === LaunchKind.Web ? '🌐' : kind === LaunchKind.Command ? '💻' : isPython ? '🐍' : '📦';
  const badge =
    kind === LaunchKind.Web ? 'WEB' : kind === LaunchKind.Command ? 'CMD' : isPython ? 'PY' : null;

  const runLaunch = async (asAdmin: boolean) => {
    if (launchState === 'launching') return;
    setLaunchState('launching');
    setErrorMsg(null);
    try {
      const result = asAdmin
        ? await api.launchAppAsAdmin(app.id)
        : await api.launchApp(app.id);
      if (result.ok) {
        setLaunchState('success');
        window.setTimeout(() => setLaunchState('idle'), 1200);
        onRefresh();
      } else {
        setLaunchState('error');
        setErrorMsg(result.message);
        window.setTimeout(() => {
          setLaunchState('idle');
          setErrorMsg(null);
        }, 3000);
      }
    } catch (error) {
      setLaunchState('error');
      setErrorMsg(String(error));
      window.setTimeout(() => {
        setLaunchState('idle');
        setErrorMsg(null);
      }, 4000);
    }
  };

  const handleLaunch = () => void runLaunch(false);

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({ x: e.clientX, y: e.clientY });
  };

  const handleDelete = async () => {
    const ok = await confirm(`确定删除「${app.name}」吗？`, {
      title: '删除应用',
      kind: 'warning',
    });
    if (!ok) return;
    try {
      await api.deleteApp(app.id);
      onRefresh();
    } catch (error) {
      setErrorMsg(String(error));
    }
  };

  const handleOpenFolder = async () => {
    try {
      await api.openFileLocation(app.executable_path);
    } catch (error) {
      setErrorMsg(String(error));
    }
  };

  const contextItems: ContextMenuItem[] = [
    { label: '启动', icon: '▶️', onClick: handleLaunch },
    { label: '以管理员身份运行', icon: '🛡️', onClick: () => void runLaunch(true) },
    { label: '编辑', icon: '✏️', onClick: onEdit },
  ];
  if (hasLocalTarget) {
    contextItems.push({
      label: '打开文件位置',
      icon: '📂',
      onClick: () => void handleOpenFolder(),
    });
  }
  contextItems.push({
    label: '删除',
    icon: '🗑️',
    onClick: () => void handleDelete(),
    danger: true,
  });

  const getStateClass = () => {
    switch (launchState) {
      case 'launching':
        return 'launching';
      case 'success':
        return 'launch-success';
      case 'error':
        return 'launch-error';
      default:
        return '';
    }
  };

  return (
    <>
      <div
        className={`app-icon ${getStateClass()}`}
        onMouseEnter={() => setIsHovered(true)}
        onMouseLeave={() => setIsHovered(false)}
        onClick={handleLaunch}
        onContextMenu={handleContextMenu}
        title={`${app.name}\n${app.executable_path}`}
      >
        <div className="app-icon-image">
          {icon ? (
            <img src={icon} alt={app.name} />
          ) : (
            <div className="app-icon-placeholder">{placeholder}</div>
          )}
          {launchState === 'launching' && (
            <div className="app-icon-loading">
              <div className="mini-spinner" />
            </div>
          )}
          {launchState === 'success' && <div className="app-icon-success">✓</div>}
        </div>
        <div className="app-icon-name">{app.name}</div>
        {badge && <div className="app-icon-badge">{badge}</div>}

        {errorMsg && <div className="app-icon-error-tooltip">{errorMsg}</div>}

        {isHovered && !isLocked && (
          <div className="app-icon-actions">
            <button
              className="action-btn admin-btn"
              onClick={(e) => {
                e.stopPropagation();
                void runLaunch(true);
              }}
              title="以管理员身份运行"
            >
              🛡️
            </button>
            <button
              className="action-btn edit-btn"
              onClick={(e) => {
                e.stopPropagation();
                onEdit();
              }}
              title="编辑"
            >
              ✏️
            </button>
            <button
              className="action-btn delete-btn"
              onClick={(e) => {
                e.stopPropagation();
                void handleDelete();
              }}
              title="删除"
            >
              🗑️
            </button>
          </div>
        )}
      </div>

      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          onClose={() => setContextMenu(null)}
          items={contextItems}
        />
      )}
    </>
  );
}
