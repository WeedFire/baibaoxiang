import { useAppStore } from '../store/appStore';
import type { TabId } from '../store/appStore';
import './Toolbar.css';

interface ToolbarProps {
  onAddApp: () => void;
  onSettings: () => void;
}

export function Toolbar({ onAddApp, onSettings }: ToolbarProps) {
  const { activeTab, setActiveTab, layoutMode, setLayoutMode, isLocked, toggleLock } = useAppStore();

  const tabs: { id: TabId; label: string }[] = [
    { id: 'dashboard', label: '主页' },
    { id: 'apps', label: '应用' },
    { id: 'marketplace', label: '插件市场' },
  ];

  return (
    <div className="toolbar">
      <div className="toolbar-left">
        <h1 className="toolbar-title">百宝箱</h1>
        <nav className="toolbar-tabs">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              className={`toolbar-tab ${activeTab === tab.id ? 'active' : ''}`}
              onClick={() => setActiveTab(tab.id)}
            >
              {tab.label}
            </button>
          ))}
        </nav>
      </div>
      <div className="toolbar-right">
        {activeTab === 'apps' && (
          <>
            <button
              className="toolbar-btn"
              onClick={() => setLayoutMode(layoutMode === 'auto' ? 'manual' : 'auto')}
              title={layoutMode === 'auto' ? '切换到手动布局' : '切换到自动布局'}
            >
              {layoutMode === 'auto' ? '⊞' : '⊡'}
            </button>
            <button
              className={`toolbar-btn ${isLocked ? 'active' : ''}`}
              onClick={toggleLock}
              title={isLocked ? '解锁布局' : '锁定布局'}
            >
              {isLocked ? '🔒' : '🔓'}
            </button>
            <button className="toolbar-btn add-btn" onClick={onAddApp} title="添加应用">
              +
            </button>
          </>
        )}
        <button className="toolbar-btn" onClick={onSettings} title="设置">
          ⚙️
        </button>
      </div>
    </div>
  );
}
