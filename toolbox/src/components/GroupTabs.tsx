import { useRef, useState } from 'react';
import { confirm } from '@tauri-apps/plugin-dialog';
import { api, ALL_GROUPS_ID, type AppGroup } from '../lib/tauri';
import { useAppStore } from '../store/appStore';
import { ContextMenu, type ContextMenuItem } from './ContextMenu';
import './GroupTabs.css';

interface GroupTabsProps {
  onCreateGroup: (name: string) => void | Promise<void>;
  /** 分组被重命名或删除后回调，由外层重新加载分组与应用 */
  onChanged: () => void | Promise<void>;
}

export function GroupTabs({ onCreateGroup, onChanged }: GroupTabsProps) {
  const { groups, currentGroupId, setCurrentGroupId, setGroups } = useAppStore();
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [dragOverId, setDragOverId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState('');
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState('');
  const [menu, setMenu] = useState<{ x: number; y: number; group: AppGroup } | null>(null);
  // Esc 取消后输入框卸载可能触发 blur，用它可以区分“取消”与“提交”
  const cancelRenameRef = useRef(false);

  const handleDragStart = (e: React.DragEvent, groupId: string) => {
    setDraggingId(groupId);
    e.dataTransfer.effectAllowed = 'move';
  };

  const handleDragOver = (e: React.DragEvent, groupId: string) => {
    e.preventDefault();
    if (groupId !== draggingId) setDragOverId(groupId);
  };

  const handleDragLeave = () => setDragOverId(null);

  const handleDrop = async (e: React.DragEvent, targetId: string) => {
    e.preventDefault();
    setDragOverId(null);
    if (!draggingId || draggingId === targetId) {
      setDraggingId(null);
      return;
    }

    const newGroups = [...groups];
    const dragIndex = newGroups.findIndex((g) => g.id === draggingId);
    const targetIndex = newGroups.findIndex((g) => g.id === targetId);
    if (dragIndex === -1 || targetIndex === -1) {
      setDraggingId(null);
      return;
    }

    const [dragged] = newGroups.splice(dragIndex, 1);
    newGroups.splice(targetIndex, 0, dragged);
    const updated = newGroups.map((g, i) => ({ ...g, sort_order: i }));
    setGroups(updated);

    try {
      for (const group of updated) {
        await api.updateGroupSort(group.id, group.sort_order);
      }
    } catch (err) {
      console.error('Failed to save group order:', err);
    }
    setDraggingId(null);
  };

  const submitNewGroup = async () => {
    const name = newName.trim();
    if (!name) return;
    await onCreateGroup(name);
    setNewName('');
    setCreating(false);
  };

  const startRename = (group: AppGroup) => {
    cancelRenameRef.current = false;
    setEditingId(group.id);
    setEditingName(group.name);
  };

  const cancelRename = () => {
    cancelRenameRef.current = true;
    setEditingId(null);
    setEditingName('');
  };

  const submitRename = async () => {
    if (cancelRenameRef.current) {
      cancelRenameRef.current = false;
      return;
    }
    const id = editingId;
    const name = editingName.trim();
    setEditingId(null);
    setEditingName('');
    if (!id || !name) return;
    if (groups.find((g) => g.id === id)?.name === name) return;

    try {
      await api.renameGroup(id, name);
      await onChanged();
    } catch (err) {
      console.error('重命名分组失败:', err);
    }
  };

  const handleDeleteGroup = async (group: AppGroup) => {
    // 分组下的应用会被级联删除，确认框里先告知数量
    let extra = '';
    try {
      const apps = await api.getAppsByGroup(group.id);
      if (apps.length > 0) extra = `\n该分组下的 ${apps.length} 个应用也会一并删除。`;
    } catch {
      // 读取数量失败不阻断删除，确认框里省略数量提示
    }

    const ok = await confirm(`确定删除分组「${group.name}」吗？${extra}`, {
      title: '删除分组',
      kind: 'warning',
    });
    if (!ok) return;

    try {
      await api.deleteGroup(group.id);
      await onChanged();
    } catch (err) {
      console.error('删除分组失败:', err);
    }
  };

  const menuItems: ContextMenuItem[] = menu
    ? [
        { label: '重命名', icon: '✏️', onClick: () => startRename(menu.group) },
        {
          label: '删除分组',
          icon: '🗑️',
          danger: true,
          disabled: menu.group.is_default,
          onClick: () => void handleDeleteGroup(menu.group),
        },
      ]
    : [];

  return (
    <div className="group-tabs">
      <div className="group-tabs-list">
        {/* “全部”虚拟分组：默认展示所有已添加的应用，不可删除/不可拖动 */}
        <button
          key={ALL_GROUPS_ID}
          className={`group-tab group-tab-all ${currentGroupId === ALL_GROUPS_ID ? 'active' : ''}`}
          onClick={() => setCurrentGroupId(ALL_GROUPS_ID)}
        >
          全部
        </button>

        {groups.map((group) => {
          if (editingId === group.id) {
            return (
              <span key={group.id} className="group-tab-editing">
                <input
                  autoFocus
                  className="group-rename-input"
                  value={editingName}
                  onChange={(e) => setEditingName(e.target.value)}
                  onFocus={(e) => e.currentTarget.select()}
                  onBlur={() => void submitRename()}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') void submitRename();
                    if (e.key === 'Escape') cancelRename();
                  }}
                />
              </span>
            );
          }

          return (
            <button
              key={group.id}
              className={`group-tab ${currentGroupId === group.id ? 'active' : ''} ${
                draggingId === group.id ? 'dragging' : ''
              } ${dragOverId === group.id ? 'drag-over' : ''}`}
              onClick={() => setCurrentGroupId(group.id)}
              onDoubleClick={() => startRename(group)}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ x: e.clientX, y: e.clientY, group });
              }}
              title="双击重命名，右键更多操作"
              draggable
              onDragStart={(e) => handleDragStart(e, group.id)}
              onDragOver={(e) => handleDragOver(e, group.id)}
              onDragLeave={handleDragLeave}
              onDrop={(e) => void handleDrop(e, group.id)}
              onDragEnd={() => {
                setDraggingId(null);
                setDragOverId(null);
              }}
            >
              {group.name}
            </button>
          );
        })}

        {creating ? (
          <span className="group-create">
            <input
              autoFocus
              className="group-create-input"
              value={newName}
              placeholder="分组名称"
              onChange={(e) => setNewName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void submitNewGroup();
                if (e.key === 'Escape') {
                  setCreating(false);
                  setNewName('');
                }
              }}
            />
            <button className="group-create-ok" onClick={() => void submitNewGroup()}>
              确定
            </button>
            <button
              className="group-create-cancel"
              onClick={() => {
                setCreating(false);
                setNewName('');
              }}
            >
              取消
            </button>
          </span>
        ) : (
          <button
            className="group-add-btn"
            onClick={() => setCreating(true)}
            title="新建分组"
          >
            +
          </button>
        )}
      </div>

      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          items={menuItems}
        />
      )}
    </div>
  );
}
