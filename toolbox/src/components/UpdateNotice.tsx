import { useEffect, useRef, useState } from 'react';
import { api, type UpdateCheckResult } from '../lib/tauri';
import { stageLabel, useUpdateInstaller } from '../hooks/useUpdateInstaller';
import './UpdateNotice.css';

interface UpdateNoticeProps {
  info: UpdateCheckResult;
  /** 本次会话不再提示 */
  onDismiss: () => void;
  /** 已忽略该版本 */
  onIgnored: () => void;
  /** 设置为「自动下载并安装」时，发现新版本后自动开始安装 */
  autoInstall?: boolean;
}

/** 发现新版本时顶部的提示条：可一键下载安装（带进度），也可查看说明或忽略。 */
export function UpdateNotice({ info, onDismiss, onIgnored, autoInstall = false }: UpdateNoticeProps) {
  const [showNotes, setShowNotes] = useState(false);
  const [busyOpen, setBusyOpen] = useState(false);
  const [openError, setOpenError] = useState<string | null>(null);
  const { busy, stage, percent, downloaded, total, result, error, start, reset } =
    useUpdateInstaller();

  const latest = info.latest_version ?? '';
  const hasUrl = Boolean(info.download_url);

  // 自动安装：只触发一次，且仅当有新版本、未被忽略、清单提供了下载地址
  const autoStarted = useRef(false);
  useEffect(() => {
    if (!autoInstall || autoStarted.current) return;
    if (!info.has_update || info.ignored || !hasUrl) return;
    autoStarted.current = true;
    void start();
  }, [autoInstall, info.has_update, info.ignored, hasUrl, start]);

  const handleOpenDownload = async () => {
    if (!info.download_url) return;
    setBusyOpen(true);
    setOpenError(null);
    try {
      await api.openExternalUrl(info.download_url);
      onDismiss();
    } catch (err) {
      setOpenError(String(err));
    } finally {
      setBusyOpen(false);
    }
  };

  const handleIgnore = async () => {
    setOpenError(null);
    try {
      await api.setIgnoredUpdateVersion(latest);
      onIgnored();
    } catch (err) {
      setOpenError(String(err));
    }
  };

  const openDownloadFolder = async () => {
    if (!result?.file_path) return;
    try {
      await api.openFileLocation(result.file_path);
    } catch (err) {
      setOpenError(String(err));
    }
  };

  const finished = Boolean(result);

  return (
    <div className="update-notice">
      <div className="update-notice-main">
        <span className="update-notice-badge">新版本</span>
        <span className="update-notice-text">
          发现 <strong>v{latest}</strong>
          <span className="update-notice-current">（当前 v{info.current_version}）</span>
        </span>

        <div className="update-notice-actions">
          {info.notes && !finished && (
            <button
              type="button"
              className="update-btn ghost"
              onClick={() => setShowNotes((prev) => !prev)}
              disabled={busy}
            >
              {showNotes ? '收起说明' : '更新说明'}
            </button>
          )}

          {/* 下载安装区：进行中显示进度，完成后显示结果 */}
          {!finished && (
            <button
              type="button"
              className="update-btn primary"
              onClick={() => void start()}
              disabled={busy}
              title={hasUrl ? '下载并自动安装' : '更新源未提供下载地址'}
            >
              {busy ? `${stageLabel(stage)}…` : '立即更新'}
            </button>
          )}

          {hasUrl && !busy && !finished && (
            <button
              type="button"
              className="update-btn ghost"
              onClick={() => void handleOpenDownload()}
              disabled={busyOpen}
              title="打开下载页自行安装"
            >
              {busyOpen ? '打开中...' : '手动下载'}
            </button>
          )}

          {finished && (
            <button type="button" className="update-btn ghost" onClick={openDownloadFolder}>
              打开安装包位置
            </button>
          )}

          {!info.mandatory && !busy && !finished && (
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

          {finished && (
            <button
              type="button"
              className="update-btn ghost"
              onClick={() => {
                reset();
                onDismiss();
              }}
            >
              关闭
            </button>
          )}
        </div>
      </div>

      {busy && (
        <div className="update-progress">
          <div className="update-progress-bar">
            <div
              className="update-progress-fill"
              style={{ width: `${stage === 'downloading' ? percent : 100}%` }}
            />
          </div>
          <span className="update-progress-text">
            {stageLabel(stage)}
            {stage === 'downloading' && total > 0
              ? ` ${percent}%（${formatSize(downloaded)} / ${formatSize(total)}）`
              : '…'}
          </span>
        </div>
      )}

      {result && (
        <div className={`update-notice-result ${result.need_restart ? 'warn' : 'ok'}`}>
          {result.message}
          {result.need_restart && '（可稍后手动重启，或现在就关闭应用重新打开）'}
          {result.installer_started && '：安装程序启动后本应用会自动退出。'}
        </div>
      )}

      {showNotes && info.notes && <pre className="update-notice-notes">{info.notes}</pre>}
      {(error || openError) && <div className="update-notice-error">{error || openError}</div>}
    </div>
  );
}

function formatSize(bytes: number): string {
  if (!bytes) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 10 || unit === 0 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}
