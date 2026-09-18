import { useCallback, useEffect, useState } from 'react';
import { api, type UpdateCheckResult } from './lib/tauri';
import { useAppStore } from './store/appStore';
import { Toolbar } from './components/Toolbar';
import { GroupTabs } from './components/GroupTabs';
import { LayoutCanvas } from './components/LayoutCanvas';
import { Dashboard } from './pages/Dashboard';
import { AppDialog } from './components/AppDialog';
import { UpdateNotice } from './components/UpdateNotice';
import { Settings } from './pages/Settings';
import './App.css';

export default function App() {
  const { activeTab, setGroups, setCurrentGroupId, currentGroupId, setApps } =
    useAppStore();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [appDialogOpen, setAppDialogOpen] = useState(false);
  const [appDialogMode, setAppDialogMode] = useState<'add' | 'edit'>('add');
  const [editingAppId, setEditingAppId] = useState<string | undefined>();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateCheckResult | null>(null);

  const loadData = useCallback(async () => {
    try {
      const groups = await api.getGroups();
      setGroups(groups);

      // 首次加载或当前分组已被删除时，自动选中第一个分组
      let groupId = currentGroupId;
      if (!groupId || !groups.some((g) => g.id === groupId)) {
        groupId = groups[0]?.id ?? null;
        if (groupId) setCurrentGroupId(groupId);
      }

      setApps(groupId ? await api.getAppsByGroup(groupId) : []);
      setError(null);
    } catch (err) {
      console.error('Failed to load data:', err);
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [currentGroupId, setGroups, setCurrentGroupId, setApps]);

  useEffect(() => {
    void loadData();
  }, [loadData]);

  // 启动后自动检查更新：后端按间隔复用上次结果，不会每次启动都联网
  useEffect(() => {
    const timer = window.setTimeout(() => {
      void (async () => {
        try {
          const result = await api.checkUpdate(false);
          if (result.has_update) setUpdateInfo(result);
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
          onDismiss={() => setUpdateInfo(null)}
          onIgnored={() => setUpdateInfo(null)}
        />
      )}
      {activeTab === 'dashboard' ? (
        <Dashboard onRefresh={() => void loadData()} />
      ) : (
        <>
          <GroupTabs
            onCreateGroup={async (name) => {
              await api.createGroup(name);
              await loadData();
            }}
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
