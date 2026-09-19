import { useState } from 'react';
import { api, ALL_GROUPS_ID } from '../lib/tauri';
import { useAppStore } from '../store/appStore';
import './GroupTabs.css';

interface GroupTabsProps {
  onCreateGroup: (name: string) => void | Promise<void>;
}

export function GroupTabs({ onCreateGroup }: GroupTabsProps) {
  const { groups, currentGroupId, setCurrentGroupId, setGroups } = useAppStore();
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [dragOverId, setDragOverId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState('');

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

        {groups.map((group) => (
          <button
            key={group.id}
            className={`group-tab ${currentGroupId === group.id ? 'active' : ''} ${
              draggingId === group.id ? 'dragging' : ''
            } ${dragOverId === group.id ? 'drag-over' : ''}`}
            onClick={() => setCurrentGroupId(group.id)}
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
        ))}

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
    </div>
  );
}
