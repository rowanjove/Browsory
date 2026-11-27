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
  availableProfiles: string[];
  availableBrowserIds: string[];
  hasLoadedFilterOptions: boolean;

  // Visible Columns (Default strictly follows PRD: time, title, url only)
  columns: ColumnVisibility;

  // Pagination & Data
  items: VisitListItem[];
  cursorTime: number | null;
  cursorId: number | null;
  cursorSortValue: number | null;
  hasMore: boolean;
  isLoading: boolean;
  historyError: string | null;

  // Selection & Detail Drawer
  selectedIds: Set<number>;
  detailVisitId: number | null;
  detailData: VisitDetail | null;
  isLoadingDetail: boolean;
  detailError: string | null;
  isDeleting: boolean;

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
  getCurrentFilter: () => HistoryFilter;

  fetchHistory: (reset?: boolean) => Promise<void>;
  toggleSelectId: (id: number) => void;
  selectAllVisible: () => void;
  clearSelection: () => void;
  clearMemoryCache: () => void;
  invalidateSession: () => void;

  openDetail: (visitId: number) => Promise<void>;
  closeDetail: () => void;
  deleteSelected: () => Promise<number>;
  toggleFavoriteUrl: (urlId: number) => Promise<boolean>;
  addTagToUrl: (urlId: number, tag: string) => Promise<void>;
  removeTagFromUrl: (urlId: number, tag: string) => Promise<void>;

  loadSmartCollections: () => Promise<void>;
  loadFilterOptions: (force?: boolean) => Promise<void>;
  saveCurrentAsCollection: (name: string) => Promise<SmartCollection>;
  deleteCollection: (id: number) => Promise<void>;
  applyCollection: (col: SmartCollection | null) => void;
}

let fetchSequenceId = 0;
let sessionEpoch = 0;
let detailRequestId = 0;
let collectionRequestId = 0;

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
  availableProfiles: [],
  availableBrowserIds: [],
  hasLoadedFilterOptions: false,

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
  cursorSortValue: null,
  hasMore: true,
  isLoading: false,
  historyError: null,

  selectedIds: new Set<number>(),
  detailVisitId: null,
  detailData: null,
  isLoadingDetail: false,
  detailError: null,
  isDeleting: false,

  setSearch: (search) => {
    set({ search, selectedIds: new Set<number>() });
    get().fetchHistory(true);
  },

  setSelectedBrowsers: (browsers) => {
    set({ selectedBrowsers: browsers, selectedIds: new Set<number>() });
    get().fetchHistory(true);
  },

  setSelectedProfiles: (profiles) => {
    set({ selectedProfiles: profiles, selectedIds: new Set<number>() });
    get().fetchHistory(true);
  },

  setTimeRange: (timeRange) => {
    set({ timeRange, selectedDate: null, selectedIds: new Set<number>() });
    get().fetchHistory(true);
  },

  setCustomTimeRange: (start, end) => {
    set({ timeRange: 'custom', customStartTime: start, customEndTime: end, selectedDate: null, selectedIds: new Set<number>() });
    get().fetchHistory(true);
  },

  setSortBy: (sortBy) => {
    set({ sortBy, selectedIds: new Set<number>() });
    get().fetchHistory(true);
  },

  jumpToDate: (dateStr) => {
    set({ selectedDate: dateStr, selectedIds: new Set<number>() });
    get().fetchHistory(true);
  },

  goToPreviousDay: () => {
    const current = get().selectedDate;
    const baseDate = current ? parseLocalDate(current) || new Date() : new Date();
    baseDate.setDate(baseDate.getDate() - 1);
    const prevStr = formatDate(baseDate);
    set({ selectedDate: prevStr, selectedIds: new Set<number>() });
    get().fetchHistory(true);
  },

  goToNextDay: () => {
    const current = get().selectedDate;
    const baseDate = current ? parseLocalDate(current) || new Date() : new Date();
    baseDate.setDate(baseDate.getDate() + 1);
    const nextStr = formatDate(baseDate);
    set({ selectedDate: nextStr, selectedIds: new Set<number>() });
    get().fetchHistory(true);
  },

  toggleOnlyFavorites: () => {
    set((s) => ({ onlyFavorites: !s.onlyFavorites, selectedIds: new Set<number>() }));
    get().fetchHistory(true);
  },

  setSelectedTag: (tag) => {
    set({ selectedTag: tag, selectedIds: new Set<number>() });
    get().fetchHistory(true);
  },

  toggleColumn: (col) =>
    set((state) => ({
      columns: {
        ...state.columns,
        [col]: !state.columns[col],
      },
    })),

  getCurrentFilter: () => {
    const state = get();
    let startTime: number | undefined;
    let endTime: number | undefined;

    if (state.selectedDate) {
      const selected = parseLocalDate(state.selectedDate);
      if (selected) {
        startTime = startOfLocalDay(selected).getTime();
        endTime = endOfLocalDay(selected).getTime();
      }
    } else if (state.timeRange === 'custom') {
      startTime = state.customStartTime ?? undefined;
      endTime = state.customEndTime ?? undefined;
    } else {
      [startTime, endTime] = calculateTimeRange(state.timeRange);
    }

    return {
      search: state.search.trim() || undefined,
      browsers: state.selectedBrowsers.length > 0 ? state.selectedBrowsers : undefined,
      profiles: state.selectedProfiles.length > 0 ? state.selectedProfiles : undefined,
      start_time: startTime,
      end_time: endTime,
      only_favorites: state.onlyFavorites ? true : undefined,
      tag: state.selectedTag || undefined,
      sort_by: state.sortBy,
    };
  },

  fetchHistory: async (reset = false) => {
    if (!reset && get().isLoading) return;

    const seq = ++fetchSequenceId;
    const requestEpoch = sessionEpoch;
    set({ isLoading: true, historyError: null, ...(reset ? { hasMore: true } : {}) });

    try {
      const state = get();
      const currentCursorTime = reset ? undefined : state.cursorTime ?? undefined;
      const currentCursorId = reset ? undefined : state.cursorId ?? undefined;
      const currentCursorSortValue = reset ? undefined : state.cursorSortValue ?? undefined;

      let startTime: number | undefined;
      let endTime: number | undefined;

      if (state.selectedDate) {
        const selected = parseLocalDate(state.selectedDate);
        if (selected) {
          startTime = startOfLocalDay(selected).getTime();
          endTime = endOfLocalDay(selected).getTime();
        }
      } else if (state.timeRange === 'custom') {
        startTime = state.customStartTime ?? undefined;
        endTime = state.customEndTime ?? undefined;
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
        cursor_sort_value: currentCursorSortValue,
        limit: 100,
        only_favorites: state.onlyFavorites ? true : undefined,
        tag: state.selectedTag || undefined,
        sort_by: state.sortBy,
      };

      const result = await tauriApi.getHistoryPage(filter);

      // Discard stale responses if a newer fetch was started
      if (requestEpoch !== sessionEpoch || seq !== fetchSequenceId) return;

      set((prev) => ({
        items: reset ? result.items : [...prev.items, ...result.items],
        cursorTime: result.next_cursor_time,
        cursorId: result.next_cursor_id,
        cursorSortValue: result.next_cursor_sort_value ?? null,
        hasMore: result.has_more,
        isLoading: false,
        historyError: null,
      }));
    } catch (err) {
      console.error('Failed to fetch history:', err);
      if (requestEpoch === sessionEpoch && seq === fetchSequenceId) {
        set({
          isLoading: false,
          hasMore: false,
          historyError: getErrorMessage(err, '加载历史记录失败'),
        });
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

  clearMemoryCache: () => {
    fetchSequenceId++;
    detailRequestId++;
    collectionRequestId++;
    set({
      items: [],
      selectedIds: new Set<number>(),
      detailVisitId: null,
      detailData: null,
      smartCollections: [],
      availableProfiles: [],
      availableBrowserIds: [],
      hasLoadedFilterOptions: false,
      cursorTime: null,
      cursorId: null,
      cursorSortValue: null,
      hasMore: true,
      isLoading: false,
      historyError: null,
      isLoadingDetail: false,
      detailError: null,
      isDeleting: false,
      search: '',
      selectedBrowsers: [],
      selectedProfiles: [],
      selectedTag: null,
      activeCollectionId: null,
      selectedDate: null,
      timeRange: 'all',
      customStartTime: null,
      customEndTime: null,
      onlyFavorites: false,
    });
  },

  invalidateSession: () => {
    sessionEpoch++;
    get().clearMemoryCache();
  },

  openDetail: async (visitId) => {
    const requestEpoch = sessionEpoch;
    const requestId = ++detailRequestId;
    set({ detailVisitId: visitId, detailData: null, detailError: null, isLoadingDetail: true });
    try {
      const detail = await tauriApi.getVisitDetail(visitId);
      if (requestEpoch !== sessionEpoch || requestId !== detailRequestId) return;
      set({
        detailData: detail,
        detailError: detail ? null : '未找到该历史记录',
        isLoadingDetail: false,
      });
    } catch (e) {
      console.error('Failed to fetch detail:', e);
      if (requestEpoch === sessionEpoch && requestId === detailRequestId) {
        set({ isLoadingDetail: false, detailData: null, detailError: getErrorMessage(e, '加载详情失败') });
      }
    }
  },

  closeDetail: () => {
    detailRequestId++;
    set({ detailVisitId: null, detailData: null, isLoadingDetail: false, detailError: null });
  },

  deleteSelected: async () => {
    const ids = Array.from(get().selectedIds);
    if (ids.length === 0 || get().isDeleting) return 0;
    const requestEpoch = sessionEpoch;
    const deletedIds = new Set(ids);
    set({ isDeleting: true });
    try {
      const count = await tauriApi.deleteVisits(ids);
      if (requestEpoch !== sessionEpoch) return count;
      set((state) => {
        const nextSelection = new Set(state.selectedIds);
        ids.forEach((id) => nextSelection.delete(id));
        const detailWasDeleted = state.detailVisitId !== null && deletedIds.has(state.detailVisitId);
        return {
          items: state.items.filter((item) => !deletedIds.has(item.id)),
          selectedIds: nextSelection,
          detailVisitId: detailWasDeleted ? null : state.detailVisitId,
          detailData: detailWasDeleted ? null : state.detailData,
        };
      });
      return count;
    } finally {
      if (requestEpoch === sessionEpoch) set({ isDeleting: false });
    }
  },

  toggleFavoriteUrl: async (urlId: number) => {
    const requestEpoch = sessionEpoch;
    const isFav = await tauriApi.toggleFavorite(urlId);
    if (requestEpoch !== sessionEpoch) return isFav;
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
    const requestEpoch = sessionEpoch;
    await tauriApi.addTagToUrl(urlId, trimmed);
    if (requestEpoch !== sessionEpoch) return;
    const detail = get().detailData;
    if (detail && detail.url_id === urlId) {
      if (!detail.tags.includes(trimmed)) {
        set({ detailData: { ...detail, tags: [...detail.tags, trimmed] } });
      }
    }
  },

  removeTagFromUrl: async (urlId: number, tag: string) => {
    const requestEpoch = sessionEpoch;
    await tauriApi.removeTagFromUrl(urlId, tag);
    if (requestEpoch !== sessionEpoch) return;
    const detail = get().detailData;
    if (detail && detail.url_id === urlId) {
      set({ detailData: { ...detail, tags: detail.tags.filter((t) => t !== tag) } });
    }
  },

  loadSmartCollections: async () => {
    const requestEpoch = sessionEpoch;
    const requestId = ++collectionRequestId;
    try {
      const list = await tauriApi.listSmartCollections();
      if (requestEpoch !== sessionEpoch || requestId !== collectionRequestId) return;
      set({ smartCollections: list });
    } catch (e) {
      console.error('Failed to load smart collections', e);
    }
  },

  loadFilterOptions: async (force = false) => {
    if (!force && get().hasLoadedFilterOptions) return;
    const requestEpoch = sessionEpoch;
    try {
      const profiles = await tauriApi.scanBrowsers();
      if (requestEpoch !== sessionEpoch) return;
      const names = Array.from(new Set(profiles.map((p) => p.profile_name))).filter(Boolean);
      const browserIds = Array.from(new Set(profiles.map((profile) => profile.browser)));
      set({
        availableProfiles: names,
        availableBrowserIds: browserIds,
        hasLoadedFilterOptions: true,
      });
    } catch (e) {
      console.error('Failed to load browser filter options', e);
    }
  },

  saveCurrentAsCollection: async (name: string) => {
    const requestEpoch = sessionEpoch;
    const state = get();
    const query = state.search;
    const filterJson = JSON.stringify({
      version: 2,
      browsers: state.selectedBrowsers,
      profiles: state.selectedProfiles,
      timeRange: state.timeRange,
      customStartTime: state.customStartTime,
      customEndTime: state.customEndTime,
      selectedDate: state.selectedDate,
      onlyFavorites: state.onlyFavorites,
      tag: state.selectedTag,
      sortBy: state.sortBy,
    });
    const col = await tauriApi.createSmartCollection(name, query, filterJson);
    if (requestEpoch !== sessionEpoch) return col;
    set((s) => ({
      smartCollections: [col, ...s.smartCollections],
      activeCollectionId: col.id,
    }));
    return col;
  },

  deleteCollection: async (id: number) => {
    const requestEpoch = sessionEpoch;
    await tauriApi.deleteSmartCollection(id);
    if (requestEpoch !== sessionEpoch) return;
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
        const validTimeRanges: TimeRangeKey[] = [
          'all', 'today', 'yesterday', 'last7Days', 'last30Days', 'last90Days', 'thisYear', 'custom',
        ];
        const parsedTimeRange = validTimeRanges.includes(parsed.timeRange) ? parsed.timeRange : 'all';
        const customStart = toFiniteNumber(parsed.customStartTime);
        const customEnd = toFiniteNumber(parsed.customEndTime);
        const hasValidCustomRange = customStart !== null && customEnd !== null && customStart <= customEnd;
        const selectedDate = typeof parsed.selectedDate === 'string' && parseLocalDate(parsed.selectedDate)
          ? parsed.selectedDate
          : null;
        extraState = {
          selectedBrowsers: Array.isArray(parsed.browsers) ? parsed.browsers.filter((v: unknown) => typeof v === 'string') : [],
          selectedProfiles: Array.isArray(parsed.profiles) ? parsed.profiles.filter((v: unknown) => typeof v === 'string') : [],
          timeRange: parsedTimeRange === 'custom' && !hasValidCustomRange ? 'all' : parsedTimeRange,
          customStartTime: hasValidCustomRange ? customStart : null,
          customEndTime: hasValidCustomRange ? customEnd : null,
          selectedDate,
          onlyFavorites: !!parsed.onlyFavorites,
          selectedTag: typeof parsed.tag === 'string' && parsed.tag.trim() ? parsed.tag : null,
          sortBy: ['time_desc', 'time_asc', 'visit_count', 'duration'].includes(parsed.sortBy)
            ? parsed.sortBy
            : 'time_desc',
        };
      } catch (e) {
        console.error('Failed to parse filter_json', e);
        extraState = {
          selectedBrowsers: [],
          selectedProfiles: [],
          timeRange: 'all',
          customStartTime: null,
          customEndTime: null,
          selectedDate: null,
          onlyFavorites: false,
          selectedTag: null,
          sortBy: 'time_desc',
        };
      }
    }
    set({
      search: col.query || '',
      activeCollectionId: col.id,
      selectedIds: new Set<number>(),
      customStartTime: null,
      customEndTime: null,
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

function parseLocalDate(value: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const date = new Date(year, month - 1, day);
  return date.getFullYear() === year && date.getMonth() === month - 1 && date.getDate() === day
    ? date
    : null;
}

function startOfLocalDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate(), 0, 0, 0, 0);
}

function endOfLocalDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + 1, 0, 0, 0, -1);
}

function toFiniteNumber(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

function getErrorMessage(error: unknown, fallback: string): string {
  if (typeof error === 'string' && error.trim()) return error;
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === 'string' && message.trim()) return message;
  }
  return fallback;
}

export function calculateTimeRange(key: TimeRangeKey): [number | undefined, number | undefined] {
  if (key === 'all' || key === 'custom') return [undefined, undefined];

  const now = new Date();
  const startOfDayDate = startOfLocalDay(now);
  const endOfDayDate = endOfLocalDay(now);
  const startOfDay = startOfDayDate.getTime();
  const endOfDay = endOfDayDate.getTime();

  switch (key) {
    case 'today':
      return [startOfDay, endOfDay];
    case 'yesterday': {
      const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1);
      return [startOfLocalDay(yesterday).getTime(), endOfLocalDay(yesterday).getTime()];
    }
    case 'last7Days':
      return [new Date(now.getFullYear(), now.getMonth(), now.getDate() - 6).getTime(), endOfDay];
    case 'last30Days':
      return [new Date(now.getFullYear(), now.getMonth(), now.getDate() - 29).getTime(), endOfDay];
    case 'last90Days':
      return [new Date(now.getFullYear(), now.getMonth(), now.getDate() - 89).getTime(), endOfDay];
    case 'thisYear': {
      const startOfYear = new Date(now.getFullYear(), 0, 1).getTime();
      return [startOfYear, endOfDay];
    }
    default:
      return [undefined, undefined];
  }
}
