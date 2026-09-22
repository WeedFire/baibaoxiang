import { useEffect, useState } from 'react';
import { LAUNCH_NOTICE_EVENT } from '../lib/launchNotice';
import { listenLaunchProgress } from '../lib/tauri';
import './LaunchNotices.css';

interface LaunchNotice {
  id: number;
  text: string;
}

let noticeSeq = 0;

/**
 * 全局启动提示。
 *
 * 订阅后端 `app://launch-progress`（例如「正在自动安装依赖」「缺少模块 X，正在安装…」），
 * 并接收 `pushLaunchNotice` 推送的消息——这样无论从哪个页面（应用页 / 仪表盘）启动应用，
 * 提示都一定能被看到。
 */
export function LaunchNotices() {
  const [items, setItems] = useState<LaunchNotice[]>([]);

  useEffect(() => {
    const add = (text: string) => {
      if (!text) return;
      const id = ++noticeSeq;
      setItems((prev) => [...prev.slice(-3), { id, text }]);
      window.setTimeout(() => {
        setItems((prev) => prev.filter((item) => item.id !== id));
      }, 8000);
    };

    let unlisten: (() => void) | null = null;
    void listenLaunchProgress((progress) => add(progress.message)).then(
      (fn) => {
        unlisten = fn;
      },
      (err) => console.warn('订阅启动提示失败:', err),
    );

    const onCustom = (event: Event) => add((event as CustomEvent<string>).detail);
    window.addEventListener(LAUNCH_NOTICE_EVENT, onCustom);

    return () => {
      unlisten?.();
      window.removeEventListener(LAUNCH_NOTICE_EVENT, onCustom);
    };
  }, []);

  if (items.length === 0) return null;

  return (
    <div className="launch-notices">
      {items.map((item) => (
        <div key={item.id} className="launch-notice">
          {item.text}
        </div>
      ))}
    </div>
  );
}
