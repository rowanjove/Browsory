import { create } from 'zustand';
import { tauriApi } from '../services/tauri';
import { HistoryFilter, SmartCollection, VisitDetail, VisitListItem } from '../types';

export type TimeRangeKey = 'all' | 'today' | 'yesterday' | 'last7Days' | 'last30Days' | 'last90Days' | 'thisYear' | 'custom';

export interface ColumnVisibility {
  time: boolean;
  title: boolean;
  url: boolean;
  domain: boolean;
  browser: boolean;
  profile: boolean;
  duration: boolean;
}

interface HistoryStore {
  // Filters
  search: string;
  selectedBrowsers: string[];
  selectedProfiles: string[];
  timeRange: TimeRangeKey;
  customStartTime: number | null;
  customEndTime: number | null;
  selectedDate: string | null;
  onlyFavorites: boolean;
  selectedTag: string | null;
  sortBy: 'time_desc' | 'time_asc' | 'visit_count' | 'duration';
  smartCollections: SmartCollection[];
  activeCollectionId: number | null;

  // Visible Columns (Default strictly follows PRD: time, title, url only)
  columns: ColumnVisibility;

  // Pagination & Data
  items: VisitListItem[];
  cursorTime: number | null;
  cursorId: number | null;
  hasMore: boolean;
  isLoading: boolean;

  // Selection & Detail Drawer
  selectedIds: Set<number>;
  detailVisitId: number | null;
  detailData: VisitDetail | null;
  isLoadingDetail: boolean;

  // Actions
  setSearch: (search: string) => void;
  setSelectedBrowsers: (browsers: string[]) => void;
  setSelectedProfiles: (profiles: string[]) => void;
  setTimeRange: (range: TimeRangeKey) => void;
  setCustomTimeRange: (start: number | null, end: number | null) => void;
  setSortBy: (sortBy: 'time_desc' | 'time_asc' | 'visit_count' | 'duration') => void;
  jumpToDate: (dateStr: string | null) => void;
  goToPreviousDay: () => void;
  goToNextDay: () => void;
  toggleOnlyFavorites: () => void;
  setSelectedTag: (tag: string | null) => void;
  toggleColumn: (col: keyof ColumnVisibility) => void;

  fetchHistory: (reset?: boolean) => Promise<void>;
  toggleSelectId: (id: number) => void;
  selectAllVisible: () => void;
  clearSelection: () => void;

  openDetail: (visitId: number) => Promise<void>;
  closeDetail: () => void;
  deleteSelected: () => Promise<number>;
  toggleFavoriteUrl: (urlId: number) => Promise<boolean>;
  addTagToUrl: (urlId: number, tag: string) => Promise<void>;
  removeTagFromUrl: (urlId: number, tag: string) => Promise<void>;

  loadSmartCollections: () => Promise<void>;
  saveCurrentAsCollection: (name: string) => Promise<SmartCollection>;
  deleteCollection: (id: number) => Promise<void>;
  applyCollection: (col: SmartCollection | null) => void;
}

let fetchSequenceId = 0;

export const useHistoryStore = create<HistoryStore>((set, get) => ({
  search: '',
  selectedBrowsers: [],
  selectedProfiles: [],
  timeRange: 'all',
  customStartTime: null,
  customEndTime: null,
  selectedDate: null,
  onlyFavorites: false,
  selectedTag: null,
  sortBy: 'time_desc',
  smartCollections: [],
  activeCollectionId: null,

  columns: {
    time: true,
    title: true,
    url: true,
    domain: false,
    browser: false,
    profile: false,
    duration: false,
  },

  items: [],
  cursorTime: null,
  cursorId: null,
  hasMore: true,
  isLoading: false,

  selectedIds: new Set<number>(),
  detailVisitId: null,
  detailData: null,
  isLoadingDetail: false,

  setSearch: (search) => {
    set({ search });
    get().fetchHistory(true);
  },

  setSelectedBrowsers: (browsers) => {
    set({ selectedBrowsers: browsers });
    get().fetchHistory(true);
  },

  setSelectedProfiles: (profiles) => {
    set({ selectedProfiles: profiles });
    get().fetchHistory(true);
  },

  setTimeRange: (timeRange) => {
    set({ timeRange, selectedDate: null });
    get().fetchHistory(true);
  },

  setCustomTimeRange: (start, end) => {
    set({ timeRange: 'custom', customStartTime: start, customEndTime: end, selectedDate: null });
    get().fetchHistory(true);
  },

  setSortBy: (sortBy) => {
    set({ sortBy });
    get().fetchHistory(true);
  },

  jumpToDate: (dateStr) => {
    set({ selectedDate: dateStr });
    get().fetchHistory(true);
  },

  goToPreviousDay: () => {
    const current = get().selectedDate;
    const baseDate = current ? new Date(current) : new Date();
    baseDate.setDate(baseDate.getDate() - 1);
    const prevStr = formatDate(baseDate);
    set({ selectedDate: prevStr });
    get().fetchHistory(true);
  },

  goToNextDay: () => {
    const current = get().selectedDate;
    const baseDate = current ? new Date(current) : new Date();
    baseDate.setDate(baseDate.getDate() + 1);
    const nextStr = formatDate(baseDate);
    set({ selectedDate: nextStr });
    get().fetchHistory(true);
  },

  toggleOnlyFavorites: () => {
    set((s) => ({ onlyFavorites: !s.onlyFavorites }));
    get().fetchHistory(true);
  },

  setSelectedTag: (tag) => {
    set({ selectedTag: tag });
    get().fetchHistory(true);
  },

  toggleColumn: (col) =>
    set((state) => ({
      columns: {
        ...state.columns,
        [col]: !state.columns[col],
      },
    })),

  fetchHistory: async (reset = false) => {
    if (!reset && get().isLoading) return;

    const seq = ++fetchSequenceId;
    set({ isLoading: true });

    try {
      const state = get();
      const currentCursorTime = reset ? undefined : state.cursorTime || undefined;
      const currentCursorId = reset ? undefined : state.cursorId || undefined;

      let startTime: number | undefined;
      let endTime: number | undefined;

      if (state.selectedDate) {
        const [y, m, d] = state.selectedDate.split('-').map(Number);
        const dayStart = new Date(y, m - 1, d, 0, 0, 0, 0);
        const dayEnd = new Date(y, m - 1, d, 23, 59, 59, 999);
        startTime = dayStart.getTime();
        endTime = dayEnd.getTime();
      } else if (state.timeRange === 'custom') {
        startTime = state.customStartTime || undefined;
        endTime = state.customEndTime || undefined;
      } else {
        [startTime, endTime] = calculateTimeRange(state.timeRange);
      }

      const filter: HistoryFilter = {
        search: state.search.trim() || undefined,
        browsers: state.selectedBrowsers.length > 0 ? state.selectedBrowsers : undefined,
        profiles: state.selectedProfiles.length > 0 ? state.selectedProfiles : undefined,
        start_time: startTime,
        end_time: endTime,
        cursor_time: currentCursorTime,
        cursor_id: currentCursorId,
        limit: 100,
        only_favorites: state.onlyFavorites ? true : undefined,
        tag: state.selectedTag || undefined,
        sort_by: state.sortBy,
      };

      const result = await tauriApi.getHistoryPage(filter);

      // Discard stale responses if a newer fetch was started
      if (seq !== fetchSequenceId) return;

      set((prev) => ({
        items: reset ? result.items : [...prev.items, ...result.items],
        cursorTime: result.next_cursor_time,
        cursorId: result.next_cursor_id,
        hasMore: result.has_more,
        isLoading: false,
      }));
    } catch (err) {
      console.error('Failed to fetch history:', err);
      if (seq === fetchSequenceId) {
        set({ isLoading: false });
      }
    }
  },

  toggleSelectId: (id) =>
    set((state) => {
      const next = new Set(state.selectedIds);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return { selectedIds: next };
    }),

  selectAllVisible: () =>
    set((state) => {
      const next = new Set<number>();
      for (const item of state.items) {
        next.add(item.id);
      }
      return { selectedIds: next };
    }),

  clearSelection: () => set({ selectedIds: new Set<number>() }),

  openDetail: async (visitId) => {
    set({ detailVisitId: visitId, isLoadingDetail: true });
    try {
      const detail = await tauriApi.getVisitDetail(visitId);
      set({ detailData: detail, isLoadingDetail: false });
    } catch (e) {
      console.error('Failed to fetch detail:', e);
      set({ isLoadingDetail: false });
    }
  },

  closeDetail: () => set({ detailVisitId: null, detailData: null }),

  deleteSelected: async () => {
    const ids = Array.from(get().selectedIds);
    if (ids.length === 0) return 0;

    const count = await tauriApi.deleteVisits(ids);
    set((state) => ({
      items: state.items.filter((item) => !state.selectedIds.has(item.id)),
      selectedIds: new Set<number>(),
      detailVisitId: state.detailVisitId && state.selectedIds.has(state.detailVisitId) ? null : state.detailVisitId,
      detailData: state.detailVisitId && state.selectedIds.has(state.detailVisitId) ? null : state.detailData,
    }));
    return count;
  },

  toggleFavoriteUrl: async (urlId: number) => {
    const isFav = await tauriApi.toggleFavorite(urlId);
    set((state) => ({
      items: state.items.map((item) =>
        item.url_id === urlId ? { ...item, is_favorite: isFav } : item
      ),
      detailData:
        state.detailData && state.detailData.url_id === urlId
          ? { ...state.detailData, is_favorite: isFav }
          : state.detailData,
    }));
    return isFav;
  },

  addTagToUrl: async (urlId: number, tag: string) => {
    const trimmed = tag.trim();
    if (!trimmed) return;
    await tauriApi.addTagToUrl(urlId, trimmed);
    const detail = get().detailData;
    if (detail && detail.url_id === urlId) {
      if (!detail.tags.includes(trimmed)) {
        set({ detailData: { ...detail, tags: [...detail.tags, trimmed] } });
      }
    }
  },

  removeTagFromUrl: async (urlId: number, tag: string) => {
    await tauriApi.removeTagFromUrl(urlId, tag);
    const detail = get().detailData;
    if (detail && detail.url_id === urlId) {
      set({ detailData: { ...detail, tags: detail.tags.filter((t) => t !== tag) } });
    }
  },

  loadSmartCollections: async () => {
    try {
      const list = await tauriApi.listSmartCollections();
      set({ smartCollections: list });
    } catch (e) {
      console.error('Failed to load smart collections', e);
    }
  },

  saveCurrentAsCollection: async (name: string) => {
    const state = get();
    const query = state.search;
    const filterJson = JSON.stringify({
      browsers: state.selectedBrowsers,
      profiles: state.selectedProfiles,
      timeRange: state.timeRange,
      onlyFavorites: state.onlyFavorites,
      tag: state.selectedTag,
      sortBy: state.sortBy,
    });
    const col = await tauriApi.createSmartCollection(name, query, filterJson);
    set((s) => ({
      smartCollections: [col, ...s.smartCollections],
      activeCollectionId: col.id,
    }));
    return col;
  },

  deleteCollection: async (id: number) => {
    await tauriApi.deleteSmartCollection(id);
    set((s) => ({
      smartCollections: s.smartCollections.filter((c) => c.id !== id),
      activeCollectionId: s.activeCollectionId === id ? null : s.activeCollectionId,
    }));
  },

  applyCollection: (col: SmartCollection | null) => {
    if (!col) {
      set({ activeCollectionId: null });
      return;
    }
    let extraState: any = {};
    if (col.filter_json) {
      try {
        const parsed = JSON.parse(col.filter_json);
        extraState = {
          selectedBrowsers: parsed.browsers || [],
          selectedProfiles: parsed.profiles || [],
          timeRange: parsed.timeRange || 'all',
          onlyFavorites: !!parsed.onlyFavorites,
          selectedTag: parsed.tag || null,
          sortBy: parsed.sortBy || 'time_desc',
        };
      } catch (e) {
        console.error('Failed to parse filter_json', e);
      }
    }
    set({
      search: col.query || '',
      activeCollectionId: col.id,
      selectedDate: null,
      ...extraState,
    });
    get().fetchHistory(true);
  },
}));

function formatDate(d: Date): string {
  const year = d.getFullYear();
  const month = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

export function calculateTimeRange(key: TimeRangeKey): [number | undefined, number | undefined] {
  if (key === 'all' || key === 'custom') return [undefined, undefined];

  const now = new Date();
  const startOfDay = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const endOfDay = startOfDay + 24 * 60 * 60 * 1000 - 1;

  switch (key) {
    case 'today':
      return [startOfDay, endOfDay];
    case 'yesterday': {
      const startOfYesterday = startOfDay - 24 * 60 * 60 * 1000;
      const endOfYesterday = startOfDay - 1;
      return [startOfYesterday, endOfYesterday];
    }
    case 'last7Days':
      return [startOfDay - 6 * 24 * 60 * 60 * 1000, endOfDay];
    case 'last30Days':
      return [startOfDay - 29 * 24 * 60 * 60 * 1000, endOfDay];
    case 'last90Days':
      return [startOfDay - 89 * 24 * 60 * 60 * 1000, endOfDay];
    case 'thisYear': {
      const startOfYear = new Date(now.getFullYear(), 0, 1).getTime();
      return [startOfYear, endOfDay];
    }
    default:
      return [undefined, undefined];
  }
}
