import { useState } from 'react';
import { api, type UpdateCheckResult } from '../lib/tauri';
import './UpdateNotice.css';

interface UpdateNoticeProps {
  info: UpdateCheckResult;
  /** 本次会话不再提示 */
  onDismiss: () => void;
  /** 已忽略该版本 */
  onIgnored: () => void;
}

/** 发现新版本时顶部的提示条，可展开查看更新说明。 */
export function UpdateNotice({ info, onDismiss, onIgnored }: UpdateNoticeProps) {
  const [showNotes, setShowNotes] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const latest = info.latest_version ?? '';
  const hasUrl = Boolean(info.download_url);

  const handleOpenDownload = async () => {
    if (!info.download_url) return;
    setBusy(true);
    setError(null);
    try {
      await api.openExternalUrl(info.download_url);
      onDismiss();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const handleIgnore = async () => {
    setError(null);
    try {
      await api.setIgnoredUpdateVersion(latest);
      onIgnored();
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <div className="update-notice">
      <div className="update-notice-main">
        <span className="update-notice-badge">新版本</span>
        <span className="update-notice-text">
          发现 <strong>v{latest}</strong>
          <span className="update-notice-current">（当前 v{info.current_version}）</span>
        </span>

        <div className="update-notice-actions">
          {info.notes && (
            <button
              type="button"
              className="update-btn ghost"
              onClick={() => setShowNotes((prev) => !prev)}
            >
              {showNotes ? '收起说明' : '更新说明'}
            </button>
          )}
          {hasUrl ? (
            <button
              type="button"
              className="update-btn primary"
              onClick={() => void handleOpenDownload()}
              disabled={busy}
            >
              {busy ? '打开中...' : '前往下载'}
            </button>
          ) : (
            <span className="update-notice-hint">更新源未提供下载地址</span>
          )}
          {!info.mandatory && (
            <>
              <button
                type="button"
                className="update-btn ghost"
                onClick={() => void handleIgnore()}
                title={`不再提示 v${latest}`}
              >
                忽略此版本
              </button>
              <button type="button" className="update-btn ghost" onClick={onDismiss}>
                稍后
              </button>
            </>
          )}
        </div>
      </div>

      {showNotes && info.notes && <pre className="update-notice-notes">{info.notes}</pre>}
      {error && <div className="update-notice-error">{error}</div>}
    </div>
  );
}
