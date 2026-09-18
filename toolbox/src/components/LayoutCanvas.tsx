import { useCallback, useEffect, useRef, useState } from 'react';
import { api, type LayoutInfo } from '../lib/tauri';
import { useAppStore } from '../store/appStore';
import { AppIcon } from './AppIcon';
import './LayoutCanvas.css';

interface LayoutCanvasProps {
  onRefresh: () => void;
  onEditApp: (appId: string) => void;
}

interface DragState {
  appId: string;
  startX: number;
  startY: number;
  origX: number;
  origY: number;
}

const GRID_SIZE = 110;

export function LayoutCanvas({ onRefresh, onEditApp }: LayoutCanvasProps) {
  const { apps, layoutMode, isLocked } = useAppStore();
  const [layouts, setLayouts] = useState<Map<string, LayoutInfo>>(new Map());
  const [dragging, setDragging] = useState<DragState | null>(null);
  const [tempPos, setTempPos] = useState<Map<string, { x: number; y: number }>>(new Map());
  const canvasRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);

  useEffect(() => {
    if (layoutMode !== 'manual') return;
    let cancelled = false;
    void (async () => {
      try {
        const data = await api.getAllLayouts();
        if (cancelled) return;
        setLayouts(new Map(data.map((l) => [l.app_id, l])));
      } catch (err) {
        console.error('Failed to load layouts:', err);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [layoutMode, apps]);

  const getPosition = useCallback(
    (appId: string) => {
      const temp = tempPos.get(appId);
      if (temp) return temp;
      const layout = layouts.get(appId);
      if (layout) return { x: layout.pos_x, y: layout.pos_y };
      const idx = apps.findIndex((a) => a.id === appId);
      return {
        x: (idx % 6) * GRID_SIZE,
        y: Math.floor(idx / 6) * GRID_SIZE,
      };
    },
    [tempPos, layouts, apps],
  );

  const handlePointerDown = useCallback(
    (e: React.PointerEvent, appId: string) => {
      if (isLocked) return;
      e.preventDefault();
      e.stopPropagation();
      const pos = getPosition(appId);
      const state: DragState = {
        appId,
        startX: e.clientX,
        startY: e.clientY,
        origX: pos.x,
        origY: pos.y,
      };
      dragRef.current = state;
      setDragging(state);
    },
    [isLocked, getPosition],
  );

  const handlePointerMove = useCallback((e: React.PointerEvent) => {
    const state = dragRef.current;
    if (!state) return;
    const maxX = (canvasRef.current?.clientWidth ?? GRID_SIZE * 6) - GRID_SIZE;
    const maxY = (canvasRef.current?.clientHeight ?? GRID_SIZE * 6) - GRID_SIZE;
    const newX = Math.min(Math.max(0, state.origX + e.clientX - state.startX), Math.max(0, maxX));
    const newY = Math.min(Math.max(0, state.origY + e.clientY - state.startY), Math.max(0, maxY));
    setTempPos((prev) => new Map(prev).set(state.appId, { x: newX, y: newY }));
  }, []);

  const handlePointerUp = useCallback(async () => {
    const state = dragRef.current;
    dragRef.current = null;
    if (!state) return;

    const pos = tempPos.get(state.appId);
    setDragging(null);
    if (!pos) return;

    try {
      await api.saveLayout(state.appId, pos.x, pos.y);
      setLayouts((prev) =>
        new Map(prev).set(state.appId, {
          app_id: state.appId,
          pos_x: pos.x,
          pos_y: pos.y,
        }),
      );
    } catch (err) {
      console.error('Failed to save layout:', err);
    }
    setTempPos(new Map());
  }, [tempPos]);

  if (layoutMode === 'auto') {
    return (
      <div className="layout-canvas layout-auto">
        {apps.map((app) => (
          <AppIcon
            key={app.id}
            app={app}
            isLocked={isLocked}
            onRefresh={onRefresh}
            onEdit={() => onEditApp(app.id)}
          />
        ))}
        {apps.length === 0 && (
          <div className="layout-empty">
            <p>暂无应用</p>
            <p className="layout-empty-hint">点击右上角 + 添加应用或 Python 脚本</p>
          </div>
        )}
      </div>
    );
  }

  return (
    <div
      ref={canvasRef}
      className={`layout-canvas layout-manual ${dragging ? 'is-dragging' : ''}`}
      onPointerMove={handlePointerMove}
      onPointerUp={() => void handlePointerUp()}
      onPointerLeave={() => void handlePointerUp()}
    >
      {apps.map((app) => {
        const pos = getPosition(app.id);
        return (
          <div
            key={app.id}
            className={`layout-manual-item ${dragging?.appId === app.id ? 'dragging' : ''}`}
            style={{ left: `${pos.x}px`, top: `${pos.y}px` }}
            onPointerDown={(e) => handlePointerDown(e, app.id)}
          >
            <AppIcon
              app={app}
              isLocked={isLocked}
              onRefresh={onRefresh}
              onEdit={() => onEditApp(app.id)}
            />
          </div>
        );
      })}
      {apps.length === 0 && (
        <div className="layout-empty-manual">
          <p>暂无应用</p>
          <p className="layout-empty-hint">切换到自动布局或添加应用</p>
        </div>
      )}
    </div>
  );
}
