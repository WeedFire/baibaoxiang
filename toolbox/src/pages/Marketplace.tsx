import { useCallback, useEffect, useState } from 'react';
import { api, iconUrl, type MarketplacePlugin } from '../lib/tauri';
import { useMarketplaceInstaller, stageLabel } from '../hooks/useMarketplaceInstaller';
import pythonIcon from '../assets/python.png';
import './Marketplace.css';

const KIND_TABS = [
  { key: 'all', label: '全部' },
  { key: 'python_script', label: 'Python 脚本' },
  { key: 'program', label: '程序包' },
  { key: 'dependency', label: '依赖包' },
] as const;

type KindKey = (typeof KIND_TABS)[number]['key'];

function kindBadge(kind: string): string {
  switch (kind) {
    case 'python_script':
      return 'Python 脚本';
    case 'program':
      return '程序包';
    case 'dependency':
      return '依赖包';
    default:
      return kind;
  }
}

function pluginFallbackEmoji(kind: string): string {
  if (kind === 'dependency') return '📚';
  if (kind === 'python_script') return '🐍';
  return '📦';
}

export function Marketplace({ onInstalled }: { onInstalled: () => void }) {
  const [plugins, setPlugins] = useState<MarketplacePlugin[]>([]);
  const [filter, setFilter] = useState<KindKey>('all');
  const [loadError, setLoadError] = useState<string | null>(null);
  const installer = useMarketplaceInstaller();

  const load = useCallback(async () => {
    try {
      const list = await api.getMarketplace();
      setPlugins(list);
      setLoadError(null);
    } catch (err) {
      setLoadError(String(err));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const handleInstall = useCallback(
    async (plugin: MarketplacePlugin) => {
      const result = await installer.start(plugin.id);
      if (result) {
        await load(); // 刷新「已安装」状态
        onInstalled(); // 脚本类会自动添加应用，刷新应用列表
      }
    },
    [installer, load, onInstalled],
  );

  const visible =
    filter === 'all' ? plugins : plugins.filter((p) => p.kind === filter);

  return (
    <div className="marketplace">
      <div className="marketplace-header">
        <h2>插件市场</h2>
        <p className="marketplace-sub">下载小工具与脚本，自动安装到本地</p>
      </div>

      <div className="marketplace-filters">
        {KIND_TABS.map((t) => (
          <button
            key={t.key}
            className={`marketplace-filter ${filter === t.key ? 'active' : ''}`}
            onClick={() => setFilter(t.key)}
          >
            {t.label}
          </button>
        ))}
      </div>

      {loadError && <div className="marketplace-error">加载插件市场失败：{loadError}</div>}

      <div className="marketplace-grid">
        {visible.map((p) => {
          const isInstalling = installer.installingId === p.id && installer.busy;
          const customIcon = p.icon_url ? iconUrl(p.icon_url) : null;
          const icon = customIcon ?? (p.kind === 'python_script' ? pythonIcon : null);
          return (
            <div key={p.id} className={`plugin-card ${p.installed ? 'installed' : ''}`}>
              <div className="plugin-icon">
                {icon ? (
                  <img src={icon} alt="" />
                ) : (
                  <span className="plugin-emoji">{pluginFallbackEmoji(p.kind)}</span>
                )}
              </div>

              <div className="plugin-body">
                <div className="plugin-title-row">
                  <span className="plugin-name">{p.name}</span>
                  <span className="plugin-kind">{kindBadge(p.kind)}</span>
                </div>
                <div className="plugin-meta">
                  v{p.version}
                  {p.author ? ` · ${p.author}` : ''}
                  {p.python_requirement ? ` · ${p.python_requirement}` : ''}
                </div>
                <p className="plugin-desc">{p.description}</p>
              </div>

              <div className="plugin-footer">
                {p.installed ? (
                  <span className="plugin-installed">
                    ✓ 已安装
                    {p.installed_version ? ` (v${p.installed_version})` : ''}
                  </span>
                ) : isInstalling ? (
                  <div className="plugin-progress">
                    <div
                      className="plugin-progress-bar"
                      style={{ width: `${installer.percent}%` }}
                    />
                    <span className="plugin-progress-label">
                      {stageLabel(installer.stage)}
                      {installer.percent > 0 ? ` ${installer.percent}%` : ''}
                    </span>
                  </div>
                ) : (
                  <button
                    className="plugin-install-btn"
                    disabled={installer.busy}
                    onClick={() => void handleInstall(p)}
                  >
                    下载安装
                  </button>
                )}
                {isInstalling && installer.error && (
                  <div className="plugin-error">{installer.error}</div>
                )}
              </div>
            </div>
          );
        })}

        {visible.length === 0 && !loadError && (
          <div className="marketplace-empty">该分类下暂无插件</div>
        )}
      </div>
    </div>
  );
}
