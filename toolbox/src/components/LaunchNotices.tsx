import { useEffect, useState } from 'react';
import { LAUNCH_NOTICE_EVENT } from '../lib/launchNotice';
import { listenLaunchProgress, type LaunchProgress } from '../lib/tauri';
import './LaunchNotices.css';

interface Toast {
  id: number;
  text: string;
}

let toastSeq = 0;

/**
 * 全局启动提示。
 *
 * 订阅后端 `app://launch-progress`：`phase=progress` 的消息（例如「正在自动安装依赖」）
 * 作为**进行中**提示保持显示（带转圈），直到收到 `phase=done` 才清除——安装依赖可能要
 * 几十秒到几分钟，不能像普通提示那样 8 秒就消失。同时接收 `pushLaunchNotice` 的普通提示。
 */
export function LaunchNotices() {
  const [toasts, setToasts] = useState<Toast[]>([]);
  // 进行中的启动进度：app_id -> 最新消息（sticky）
  const [progress, setProgress] = useState<Record<string, string>>({});

  useEffect(() => {
    const pushToast = (text: string) => {
      if (!text) return;
      const id = ++toastSeq;
      setToasts((prev) => [...prev.slice(-3), { id, text }]);
      window.setTimeout(() => {
        setToasts((prev) => prev.filter((item) => item.id !== id));
      }, 8000);
    };

    const onProgress = (p: LaunchProgress) => {
      if (p.phase === 'done') {
        setProgress((prev) => {
          if (!(p.app_id in prev)) return prev;
          const next = { ...prev };
          delete next[p.app_id];
          return next;
        });
      } else {
        setProgress((prev) => ({ ...prev, [p.app_id]: p.message }));
      }
    };

    let unlisten: (() => void) | null = null;
    void listenLaunchProgress(onProgress).then(
      (fn) => {
        unlisten = fn;
      },
      (err) => console.warn('订阅启动提示失败:', err),
    );

    const onCustom = (event: Event) => pushToast((event as CustomEvent<string>).detail);
    window.addEventListener(LAUNCH_NOTICE_EVENT, onCustom);

    return () => {
      unlisten?.();
      window.removeEventListener(LAUNCH_NOTICE_EVENT, onCustom);
    };
  }, []);

  const progressEntries = Object.entries(progress);
  if (toasts.length === 0 && progressEntries.length === 0) return null;

  return (
    <div className="launch-notices">
      {progressEntries.map(([appId, text]) => (
        <div key={`progress-${appId}`} className="launch-notice launch-notice-progress">
          <span className="launch-notice-spinner" aria-hidden="true" />
          <span className="launch-notice-text">{text}</span>
        </div>
      ))}
      {toasts.map((item) => (
        <div key={item.id} className="launch-notice">
          {item.text}
        </div>
      ))}
    </div>
  );
}
