import { useCallback, useEffect, useState } from 'react';
import { open, save } from '@tauri-apps/plugin-dialog';
import { api, type UpdateCheckResult, type UpdateState } from '../lib/tauri';
import './Settings.css';

interface SettingsProps {
  onClose: () => void;
  onImported: () => void;
}

const UPDATE_JSON_EXAMPLE = `{
  "version": "1.0.2",
  "notes": "本次更新的内容",
  "url": "https://example.com/百宝箱-1.0.2.exe",
  "mandatory": false
}`;

function formatTime(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString();
}

function describeUpdateCheck(result: UpdateCheckResult): {
  type: 'success' | 'error';
  text: string;
} {
  switch (result.status) {
    case 'unconfigured':
      return { type: 'error', text: result.message ?? '请先填写更新源地址' };
    case 'disabled':
      return { type: 'error', text: '自动检查已关闭，可点「立即检查」手动检查' };
    case 'error':
      return { type: 'error', text: result.message ?? '检查更新失败' };
    default:
      if (result.has_update) {
        return {
          type: 'success',
          text: `发现新版本 v${result.latest_version}${result.ignored ? '（已忽略）' : ''}`,
        };
      }
      return { type: 'success', text: `已是最新版本 v${result.current_version}` };
  }
}

export function Settings({ onClose, onImported }: SettingsProps) {
  const [exporting, setExporting] = useState(false);
  const [importing, setImporting] = useState(false);
  const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(
    null,
  );

  const [updateState, setUpdateState] = useState<UpdateState | null>(null);
  const [updateEnabled, setUpdateEnabled] = useState(true);
  const [updateSource, setUpdateSource] = useState('');
  const [savingUpdate, setSavingUpdate] = useState(false);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [updateMessage, setUpdateMessage] = useState<{
    type: 'success' | 'error';
    text: string;
  } | null>(null);

  const loadUpdateState = useCallback(async () => {
    try {
      const state = await api.getUpdateState();
      setUpdateState(state);
      setUpdateEnabled(state.settings.enabled);
      setUpdateSource(state.settings.source_url);
    } catch (err) {
      console.error('读取更新设置失败:', err);
      setUpdateMessage({ type: 'error', text: `读取更新设置失败：${String(err)}` });
    }
  }, []);

  useEffect(() => {
    void loadUpdateState();
  }, [loadUpdateState]);

  const handleSaveUpdate = async () => {
    setSavingUpdate(true);
    setUpdateMessage(null);
    try {
      await api.saveUpdateSettings(updateEnabled, updateSource);
      await loadUpdateState();
      setUpdateMessage({ type: 'success', text: '更新设置已保存' });
    } catch (err) {
      setUpdateMessage({ type: 'error', text: `保存失败：${String(err)}` });
    } finally {
      setSavingUpdate(false);
    }
  };

  const handleCheckUpdate = async () => {
    setCheckingUpdate(true);
    setUpdateMessage(null);
    try {
      const result = await api.checkUpdate(true);
      setUpdateMessage(describeUpdateCheck(result));
      await loadUpdateState();
    } catch (err) {
      setUpdateMessage({ type: 'error', text: `检查失败：${String(err)}` });
    } finally {
      setCheckingUpdate(false);
    }
  };

  const handleClearIgnored = async () => {
    setUpdateMessage(null);
    try {
      await api.setIgnoredUpdateVersion(null);
      await loadUpdateState();
      setUpdateMessage({ type: 'success', text: '已取消忽略该版本' });
    } catch (err) {
      setUpdateMessage({ type: 'error', text: `操作失败：${String(err)}` });
    }
  };

  const handleExport = async () => {
    setExporting(true);
    setMessage(null);
    try {
      const path = await save({
        defaultPath: 'toolbox-config.json',
        filters: [{ name: 'JSON', extensions: ['json'] }],
      });
      if (!path) return;
      await api.exportConfigToFile(path);
      setMessage({ type: 'success', text: `导出成功：${path}` });
    } catch (err) {
      console.error('Export failed:', err);
      setMessage({ type: 'error', text: `导出失败：${String(err)}` });
    } finally {
      setExporting(false);
    }
  };

  const handleImport = async () => {
    setImporting(true);
    setMessage(null);
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: 'JSON', extensions: ['json'] }],
      });
      const path = Array.isArray(selected) ? selected[0] : selected;
      if (!path) return;
      const count = await api.importConfigFromFile(path);
      setMessage({ type: 'success', text: `导入成功，共 ${count} 个应用` });
      onImported();
    } catch (err) {
      console.error('Import failed:', err);
      setMessage({ type: 'error', text: `导入失败：${String(err)}` });
    } finally {
      setImporting(false);
    }
  };

  return (
    // 只有“按下”就发生在遮罩上才关闭：在弹窗内按下、松手落在遮罩上时不会误关
    <div
      className="dialog-overlay"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="dialog settings-dialog">
        <div className="dialog-header">
          <h2>设置</h2>
          <button className="dialog-close" onClick={onClose} title="关闭">
            ×
          </button>
        </div>

        <div className="settings-content">
          <section className="settings-section">
            <h3>数据管理</h3>
            <p className="settings-desc">导出或导入应用配置，方便迁移或备份。</p>

            <div className="settings-actions">
              <button className="settings-btn" onClick={() => void handleExport()} disabled={exporting}>
                {exporting ? '导出中...' : '导出配置'}
              </button>
              <button className="settings-btn" onClick={() => void handleImport()} disabled={importing}>
                {importing ? '导入中...' : '导入配置'}
              </button>
            </div>

            {message && <div className={`settings-message ${message.type}`}>{message.text}</div>}
          </section>

          <section className="settings-section">
            <h3>版本更新</h3>
            <p className="settings-desc">
              当前版本 v{updateState?.current_version ?? '—'}
              {updateState?.checked_at
                ? ` · 上次检查：${formatTime(updateState.checked_at)}`
                : ' · 尚未检查'}
            </p>

            <label className="settings-checkbox">
              <input
                type="checkbox"
                checked={updateEnabled}
                onChange={(e) => setUpdateEnabled(e.target.checked)}
              />
              <span>启动时自动检查更新</span>
            </label>

            <div className="settings-field">
              <label htmlFor="update-source">更新源地址</label>
              <input
                id="update-source"
                type="text"
                value={updateSource}
                onChange={(e) => setUpdateSource(e.target.value)}
                placeholder="https://example.com/toolbox/update.json"
              />
              <span className="settings-hint">
                支持 http(s) 地址，也支持本地或局域网共享路径（如{' '}
                {'\\server\\share\\update.json'}）
              </span>
            </div>

            <div className="settings-actions">
              <button
                className="settings-btn"
                onClick={() => void handleSaveUpdate()}
                disabled={savingUpdate}
              >
                {savingUpdate ? '保存中...' : '保存'}
              </button>
              <button
                className="settings-btn"
                onClick={() => void handleCheckUpdate()}
                disabled={checkingUpdate}
              >
                {checkingUpdate ? '检查中...' : '立即检查'}
              </button>
            </div>

            {updateState?.ignored_version && (
              <p className="settings-desc">
                已忽略版本 v{updateState.ignored_version}
                <button
                  type="button"
                  className="settings-link"
                  onClick={() => void handleClearIgnored()}
                >
                  取消忽略
                </button>
              </p>
            )}

            {updateMessage && (
              <div className={`settings-message ${updateMessage.type}`}>{updateMessage.text}</div>
            )}

            <details className="settings-details">
              <summary>更新源 JSON 格式</summary>
              <pre>{UPDATE_JSON_EXAMPLE}</pre>
              <p className="settings-hint">
                版本号需高于当前版本才会提示；mandatory 为 true 时不提供「忽略/稍后」；
                notes 为纯文本更新说明，可省略。
              </p>
            </details>
          </section>

          <section className="settings-section">
            <h3>关于</h3>
            <p className="settings-desc">百宝箱 v{updateState?.current_version ?? '1.0.1'}</p>
            <p className="settings-desc muted">Windows 应用与 Python 脚本快速启动管理工具</p>
          </section>
        </div>
      </div>
    </div>
  );
}
