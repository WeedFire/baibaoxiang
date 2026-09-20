import { create } from 'zustand';
import type { AppGroup, AppItem } from '../lib/tauri';

export type { AppGroup, AppItem };

export type TabId = 'dashboard' | 'apps' | 'marketplace';

interface AppState {
  activeTab: TabId;
  groups: AppGroup[];
  currentGroupId: string | null;
  apps: AppItem[];
  layoutMode: 'auto' | 'manual';
  isLocked: boolean;
  setActiveTab: (tab: TabId) => void;
  setGroups: (groups: AppGroup[]) => void;
  setCurrentGroupId: (id: string | null) => void;
  setApps: (apps: AppItem[]) => void;
  setLayoutMode: (mode: 'auto' | 'manual') => void;
  toggleLock: () => void;
}

export const useAppStore = create<AppState>((set) => ({
  activeTab: 'dashboard',
  groups: [],
  currentGroupId: null,
  apps: [],
  layoutMode: 'auto',
  isLocked: false,
  setActiveTab: (tab) => set({ activeTab: tab }),
  setGroups: (groups) => set({ groups }),
  setCurrentGroupId: (id) => set({ currentGroupId: id }),
  setApps: (apps) => set({ apps }),
  setLayoutMode: (mode) => set({ layoutMode: mode }),
  toggleLock: () => set((state) => ({ isLocked: !state.isLocked })),
}));
