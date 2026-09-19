import { useCallback, useEffect, useState } from 'react';
import { api, iconUrl, LaunchKind, type AppItem, type LaunchStats } from '../lib/tauri';
import { useAppStore } from '../store/appStore';
import './Dashboard.css';

interface DashboardProps {
  onRefresh: () => void;
}

export function Dashboard({ onRefresh }: DashboardProps) {
  const [stats, setStats] = useState<LaunchStats>({ recent: [], frequent: [] });
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadStats = useCallback(async () => {
    try {
      setError(null);
      setStats(await api.getLaunchStats());
    } catch (err) {
      console.error('Failed to load launch stats:', err);
      setError('无法加载数据');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadStats();
  }, [loadStats]);

  const handleLaunch = async (appId: string) => {
    try {
      await api.launchApp(appId);
      await loadStats();
      onRefresh();
    } catch (err) {
      console.error('Failed to launch app:', err);
      setError(String(err));
    }
  };

  if (loading) {
    return (
      <div className="dashboard-loading">
        <div className="loading-spinner" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="dashboard">
        <div className="dashboard-error">
          <span className="error-icon">⚠️</span>
          <p>{error}</p>
          <button className="retry-btn" onClick={() => void loadStats()}>
            重试
          </button>
        </div>
      </div>
    );
  }

  const isEmpty = stats.recent.length === 0 && stats.frequent.length === 0;

  if (isEmpty) {
    return (
      <div className="dashboard">
        <div className="dashboard-empty">
          <div className="empty-icon">🚀</div>
          <h2 className="empty-title">欢迎使用百宝箱</h2>
          <p className="empty-desc">
            在「应用」标签页添加你的第一个应用或 Python 脚本，
            <br />
            启动后这里会显示最近使用和常用应用。
          </p>
          <button
            className="empty-action-btn"
            onClick={() => useAppStore.getState().setActiveTab('apps')}
          >
            前往添加应用
          </button>
        </div>
      </div>
    );
  }

  const renderCard = (app: AppItem) => {
    const icon = iconUrl(app.icon_path);
    const kind = app.launch_kind ?? (app.is_python_script ? LaunchKind.Python : LaunchKind.Program);
    const placeholder =
      kind === LaunchKind.Web ? '🌐' : kind === LaunchKind.Command ? '💻' : app.is_python_script ? '🐍' : '📦';
    return (
      <div key={app.id} className="app-card" onClick={() => void handleLaunch(app.id)}>
        <div className="app-card-icon">
          {icon ? (
            <img src={icon} alt={app.name} />
          ) : (
            <div className="app-card-placeholder">{placeholder}</div>
          )}
        </div>
        <div className="app-card-info">
          <span className="app-card-name">{app.name}</span>
          <span className="app-card-path">{app.executable_path}</span>
        </div>
      </div>
    );
  };

  return (
    <div className="dashboard">
      {stats.recent.length > 0 && (
        <section className="dashboard-section">
          <h2 className="section-title">最近使用</h2>
          <div className="app-list">{stats.recent.map(renderCard)}</div>
        </section>
      )}

      {stats.frequent.length > 0 && (
        <section className="dashboard-section">
          <h2 className="section-title">常用应用</h2>
          <div className="app-list">{stats.frequent.map(renderCard)}</div>
        </section>
      )}
    </div>
  );
}
