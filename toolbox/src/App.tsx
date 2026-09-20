import { useCallback, useEffect, useState } from 'react';
import { api, ALL_GROUPS_ID, type UpdateCheckResult } from './lib/tauri';
import { useAppStore } from './store/appStore';
import { Toolbar } from './components/Toolbar';
import { GroupTabs } from './components/GroupTabs';
import { LayoutCanvas } from './components/LayoutCanvas';
import { Dashboard } from './pages/Dashboard';
import { Marketplace } from './pages/Marketplace';
import { AppDialog } from './components/AppDialog';
import { UpdateNotice } from './components/UpdateNotice';
import { Settings } from './pages/Settings';
import './App.css';

export default function App() {
  const { activeTab, currentGroupId, setApps } = useAppStore();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [appDialogOpen, setAppDialogOpen] = useState(false);
  const [appDialogMode, setAppDialogMode] = useState<'add' | 'edit'>('add');
  const [editingAppId, setEditingAppId] = useState<string | undefined>();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateCheckResult | null>(null);
  const [autoInstall, setAutoInstall] = useState(false);

  /** 根据分组 id 加载应用；ALL_GROUPS_ID 代表展示全部 */
  const loadAppsFor = useCallback(async (groupId: string | null) => {
    try {
      const apps =
        !groupId || groupId === ALL_GROUPS_ID
          ? await api.getAllApps()
          : await api.getAppsByGroup(groupId);
      setApps(apps);
      setError(null);
    } catch (err) {
      console.error('Failed to load apps:', err);
      setError(String(err));
    }
  }, [setApps]);

  /** 全量刷新：重新读取分组、决定默认分组（全部），并加载应用 */
  const loadData = useCallback(async () => {
    try {
      const { setGroups, setCurrentGroupId, currentGroupId: prevId } =
        useAppStore.getState();
      const groups = await api.getGroups();
      setGroups(groups);

      // 默认选中“全部”；若之前已选中某真实分组则保持不变
      let groupId = prevId;
      if (!groupId || (groupId !== ALL_GROUPS_ID && !groups.some((g) => g.id === groupId))) {
        groupId = ALL_GROUPS_ID;
        setCurrentGroupId(ALL_GROUPS_ID);
      }

      await loadAppsFor(groupId);
    } catch (err) {
      console.error('Failed to load data:', err);
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [loadAppsFor]);

  // 首次加载
  useEffect(() => {
    void loadData();
  }, [loadData]);

  // 切换分组时重新加载应用（修复此前分组切换不刷新列表的问题）
  useEffect(() => {
    void loadAppsFor(currentGroupId);
  }, [currentGroupId, loadAppsFor]);

  // 启动后自动检查更新：后端按间隔复用上次结果，不会每次启动都联网
  useEffect(() => {
    const timer = window.setTimeout(() => {
      void (async () => {
        try {
          const result = await api.checkUpdate(false);
          if (!result.has_update) return;
          // 读取设置，决定是否需要「自动下载并安装」
          try {
            const state = await api.getUpdateState();
            setAutoInstall(state.settings.auto_install);
          } catch (err) {
            console.warn('读取更新设置失败:', err);
          }
          setUpdateInfo(result);
        } catch (err) {
          console.error('检查更新失败:', err);
        }
      })();
    }, 2500);
    return () => window.clearTimeout(timer);
  }, []);

  const handleAddApp = () => {
    setAppDialogMode('add');
    setEditingAppId(undefined);
    setAppDialogOpen(true);
  };

  const handleEditApp = (appId: string) => {
    setAppDialogMode('edit');
    setEditingAppId(appId);
    setAppDialogOpen(true);
  };

  if (loading) {
    return (
      <div className="app-loading">
        <div className="loading-spinner" />
        <p>加载中...</p>
      </div>
    );
  }

  if (error) {
    return (
      <div className="app-loading">
        <p style={{ color: '#ff6b6b' }}>加载失败：{error}</p>
        <button
          onClick={() => {
            setError(null);
            setLoading(true);
            void loadData();
          }}
        >
          重试
        </button>
      </div>
    );
  }

  return (
    <div className="app">
      <Toolbar onAddApp={handleAddApp} onSettings={() => setSettingsOpen(true)} />
      {updateInfo && (
        <UpdateNotice
          info={updateInfo}
          autoInstall={autoInstall}
          onDismiss={() => setUpdateInfo(null)}
          onIgnored={() => setUpdateInfo(null)}
        />
      )}
      {activeTab === 'dashboard' ? (
        <Dashboard onRefresh={() => void loadData()} />
      ) : activeTab === 'marketplace' ? (
        <Marketplace onInstalled={() => void loadData()} />
      ) : (
        <>
          <GroupTabs
            onCreateGroup={async (name) => {
              await api.createGroup(name);
              await loadData();
            }}
            onChanged={() => void loadData()}
          />
          <LayoutCanvas
            onRefresh={() => void loadData()}
            onEditApp={handleEditApp}
          />
        </>
      )}

      {appDialogOpen && (
        <AppDialog
          mode={appDialogMode}
          appId={editingAppId}
          onClose={() => setAppDialogOpen(false)}
          onSaved={() => void loadData()}
        />
      )}

      {settingsOpen && (
        <Settings
          onClose={() => setSettingsOpen(false)}
          onImported={() => void loadData()}
        />
      )}
    </div>
  );
}
