import { useCallback, useEffect, useRef, useState } from 'react';
import {
  api,
  listenMarketplaceProgress,
  type MarketplaceInstallResult,
  type MarketplaceProgress,
} from '../lib/tauri';

/** 进度阶段 → 中文提示 */
const STAGE_LABELS: Record<string, string> = {
  downloading: '正在下载',
  verifying: '正在校验签名',
  installing: '正在解压安装',
  done: '安装完成',
};

export function stageLabel(stage: string | null): string {
  if (!stage) return '';
  return STAGE_LABELS[stage] ?? stage;
}

export interface MarketplaceInstallerState {
  /** 是否有插件正在安装 */
  busy: boolean;
  /** 正在安装的插件 id（用于卡片级进度展示） */
  installingId: string | null;
  /** 当前阶段 */
  stage: string | null;
  /** 下载进度百分比（0-100，总大小未知时为 0） */
  percent: number;
  /** 后端附带的可读提示 */
  message: string | null;
  /** 安装结果 */
  result: MarketplaceInstallResult | null;
  /** 失败原因 */
  error: string | null;
  /** 开始安装指定插件（返回结果，失败为 null） */
  start: (pluginId: string) => Promise<MarketplaceInstallResult | null>;
  /** 清空状态 */
  reset: () => void;
}

/**
 * 插件市场「下载并安装」的共享逻辑：订阅后端 `marketplace://progress` 事件、执行安装、暴露状态。
 * 同一时刻只安装一个插件（后端亦为串行），进度事件为全局广播。
 */
export function useMarketplaceInstaller(): MarketplaceInstallerState {
  const [busy, setBusy] = useState(false);
  const [installingId, setInstallingId] = useState<string | null>(null);
  const [stage, setStage] = useState<string | null>(null);
  const [percent, setPercent] = useState(0);
  const [message, setMessage] = useState<string | null>(null);
  const [result, setResult] = useState<MarketplaceInstallResult | null>(null);
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

  const applyProgress = useCallback((progress: MarketplaceProgress) => {
    setStage(progress.stage);
    setMessage(progress.message);
    setPercent(
      progress.total > 0
        ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
        : 0,
    );
  }, []);

  const start = useCallback(
    async (pluginId: string): Promise<MarketplaceInstallResult | null> => {
      if (busyRef.current) return null; // 防重入
      busyRef.current = true;
      setBusy(true);
      setInstallingId(pluginId);
      setError(null);
      setResult(null);
      setStage('downloading');
      setPercent(0);
      setMessage(null);

      // 先订阅再发起调用，避免漏掉最早的进度事件
      try {
        unlistenRef.current = await listenMarketplaceProgress(applyProgress);
      } catch (err) {
        console.warn('订阅插件市场进度失败:', err);
      }

      try {
        const installResult = await api.installMarketplacePlugin(pluginId);
        setResult(installResult);
        setStage('done');
        setPercent(100);
        return installResult;
      } catch (err) {
        setError(String(err));
        setStage(null);
        return null;
      } finally {
        unlistenRef.current?.();
        unlistenRef.current = null;
        busyRef.current = false;
        setBusy(false);
      }
    },
    [applyProgress],
  );

  const reset = useCallback(() => {
    setStage(null);
    setPercent(0);
    setMessage(null);
    setResult(null);
    setError(null);
    setInstallingId(null);
  }, []);

  return { busy, installingId, stage, percent, message, result, error, start, reset };
}
