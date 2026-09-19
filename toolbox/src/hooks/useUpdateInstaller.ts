import { useCallback, useEffect, useRef, useState } from 'react';
import {
  api,
  listenUpdateProgress,
  type UpdateInstallResult,
  type UpdateProgress,
} from '../lib/tauri';

/** 进度阶段 → 中文提示 */
const STAGE_LABELS: Record<string, string> = {
  preparing: '正在获取更新信息',
  downloading: '正在下载更新包',
  verifying: '正在校验签名',
  installing: '正在安装',
  done: '更新完成',
};

export function stageLabel(stage: string | null): string {
  if (!stage) return '';
  return STAGE_LABELS[stage] ?? stage;
}

export interface UpdateInstallerState {
  /** 是否正在下载/安装 */
  busy: boolean;
  /** 当前阶段（preparing/downloading/verifying/installing/done） */
  stage: string | null;
  /** 下载进度百分比（0-100，总大小未知时为 0） */
  percent: number;
  downloaded: number;
  total: number;
  /** 安装结果 */
  result: UpdateInstallResult | null;
  /** 失败原因 */
  error: string | null;
  /** 开始下载并安装 */
  start: () => Promise<void>;
  /** 清空状态（用于重新开始或关闭提示） */
  reset: () => void;
}

/**
 * 「下载并安装更新」的共享逻辑：订阅后端进度事件、执行安装、暴露状态。
 * 提示条与设置页共用，避免两处各写一份。
 */
export function useUpdateInstaller(): UpdateInstallerState {
  const [busy, setBusy] = useState(false);
  const [stage, setStage] = useState<string | null>(null);
  const [percent, setPercent] = useState(0);
  const [downloaded, setDownloaded] = useState(0);
  const [total, setTotal] = useState(0);
  const [result, setResult] = useState<UpdateInstallResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  const busyRef = useRef(false);
  const unlistenRef = useRef<(() => void) | null>(null);

  // 组件卸载时取消事件订阅，避免泄漏
  useEffect(() => {
    return () => {
      unlistenRef.current?.();
      unlistenRef.current = null;
    };
  }, []);

  const applyProgress = useCallback((progress: UpdateProgress) => {
    setStage(progress.stage);
    setDownloaded(progress.downloaded);
    setTotal(progress.total);
    setPercent(
      progress.total > 0
        ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
        : 0,
    );
  }, []);

  const start = useCallback(async () => {
    if (busyRef.current) return; // 防重入：连点或多处触发只跑一次
    busyRef.current = true;
    setBusy(true);
    setError(null);
    setResult(null);
    setStage('preparing');
    setPercent(0);
    setDownloaded(0);
    setTotal(0);

    // 先订阅再发起调用，避免漏掉最早的进度事件
    try {
      unlistenRef.current = await listenUpdateProgress(applyProgress);
    } catch (err) {
      console.warn('订阅更新进度失败:', err);
    }

    try {
      const installResult = await api.downloadAndInstallUpdate();
      setResult(installResult);
      setStage('done');
      setPercent(100);
    } catch (err) {
      setError(String(err));
      setStage(null);
    } finally {
      unlistenRef.current?.();
      unlistenRef.current = null;
      busyRef.current = false;
      setBusy(false);
    }
  }, [applyProgress]);

  const reset = useCallback(() => {
    setStage(null);
    setPercent(0);
    setDownloaded(0);
    setTotal(0);
    setResult(null);
    setError(null);
  }, []);

  return { busy, stage, percent, downloaded, total, result, error, start, reset };
}
