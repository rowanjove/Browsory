import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import vm from 'node:vm';
import test from 'node:test';
import ts from 'typescript';

const require = createRequire(import.meta.url);
const root = new URL('../', import.meta.url);

function loadStore(api) {
  const source = readFileSync(new URL('src/stores/useHistoryStore.ts', root), 'utf8');
  const { outputText } = ts.transpileModule(source, {
    fileName: 'src/stores/useHistoryStore.ts',
    compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, esModuleInterop: true },
  });
  const exports = {};
  vm.runInNewContext(outputText, {
    exports,
    require: (id) => id === '../services/tauri' ? { tauriApi: api } : require(id),
    console: { info() {}, error() {}, warn() {} },
    setTimeout,
  }, { filename: 'src/stores/useHistoryStore.ts' });
  return exports.useHistoryStore;
}

const item = (id) => ({
  id,
  url_id: id,
  url: `https://example.test/${id}`,
  title: `item ${id}`,
  domain: 'example.test',
  visit_time: id,
  browser: 'test',
  profile: 'default',
  visit_duration: 0,
  visit_count: 1,
  is_favorite: false,
});

test('deletion uses the start snapshot and preserves a selection made while waiting', async () => {
  let requestedIds;
  let resolveDelete;
  const store = loadStore({
    deleteVisits(ids) {
      requestedIds = ids;
      return new Promise((resolve) => { resolveDelete = resolve; });
    },
  });
  store.setState({ items: [item(1), item(2)], selectedIds: new Set([1]) });
  const pending = store.getState().deleteSelected();
  store.getState().toggleSelectId(2);
  resolveDelete(1);
  assert.equal(await pending, 1);
  assert.deepEqual(Array.from(requestedIds), [1]);
  assert.deepEqual(store.getState().items.map((entry) => entry.id), [2]);
  assert.deepEqual([...store.getState().selectedIds], [2]);
  assert.equal(store.getState().isDeleting, false);
});

test('a failed page load pauses pagination until an explicit retry', async () => {
  let calls = 0;
  const store = loadStore({
    getHistoryPage: async () => {
      calls += 1;
      if (calls === 1) throw new Error('temporary backend failure');
      return { items: [item(2)], next_cursor_time: null, next_cursor_id: null, has_more: false };
    },
  });
  await store.getState().fetchHistory(true);
  assert.equal(store.getState().isLoading, false);
  assert.equal(store.getState().hasMore, false);
  assert.equal(store.getState().historyError, 'temporary backend failure');
  await store.getState().fetchHistory(true);
  assert.deepEqual(store.getState().items.map((entry) => entry.id), [2]);
  assert.equal(store.getState().historyError, null);
});

test('detail null and errors clear old content and settle loading state', async () => {
  const resolvers = [];
  const store = loadStore({
    getVisitDetail: () => new Promise((resolve, reject) => resolvers.push({ resolve, reject })),
  });
  const first = store.getState().openDetail(1);
  const second = store.getState().openDetail(2);
  resolvers[0].resolve({ ...item(1), nearby_visits: [], tags: [] });
  await first;
  assert.equal(store.getState().detailData, null);
  assert.equal(store.getState().isLoadingDetail, true);
  resolvers[1].resolve(null);
  await second;
  assert.equal(store.getState().isLoadingDetail, false);
  assert.equal(store.getState().detailData, null);
  assert.equal(store.getState().detailError, '未找到该历史记录');

  const failed = store.getState().openDetail(3);
  resolvers[2].reject(new Error('detail unavailable'));
  await failed;
  assert.equal(store.getState().isLoadingDetail, false);
  assert.equal(store.getState().detailData, null);
  assert.equal(store.getState().detailError, 'detail unavailable');
});

test('smart collections persist date state and reject invalid legacy custom ranges', async () => {
  let created;
  const store = loadStore({
    createSmartCollection: async (name, query, filterJson) => {
      created = { id: 7, name, query, filter_json: filterJson, created_at: 1 };
      return created;
    },
    getHistoryPage: async () => ({ items: [], next_cursor_time: null, next_cursor_id: null, has_more: false }),
  });
  store.setState({
    search: 'docs',
    timeRange: 'custom',
    customStartTime: 1000,
    customEndTime: 2000,
    selectedDate: '2025-03-08',
    selectedBrowsers: ['chrome'],
  });
  const saved = await store.getState().saveCurrentAsCollection('saved dates');
  const savedFilter = JSON.parse(created.filter_json);
  assert.equal(savedFilter.version, 2);
  assert.equal(savedFilter.customStartTime, 1000);
  assert.equal(savedFilter.customEndTime, 2000);
  assert.equal(savedFilter.selectedDate, '2025-03-08');

  store.setState({ customStartTime: 9000, customEndTime: 9999, selectedDate: null, timeRange: 'all' });
  store.getState().applyCollection(saved);
  assert.equal(store.getState().customStartTime, 1000);
  assert.equal(store.getState().customEndTime, 2000);
  assert.equal(store.getState().selectedDate, '2025-03-08');

  store.getState().applyCollection({
    id: 8,
    name: 'old invalid',
    query: '',
    filter_json: JSON.stringify({ version: 1, timeRange: 'custom' }),
    created_at: 1,
  });
  assert.equal(store.getState().timeRange, 'all');
  assert.equal(store.getState().customStartTime, null);
  assert.equal(store.getState().customEndTime, null);
});

test('pagination forwards and clears the optional sort cursor', async () => {
  const filters = [];
  let page = 0;
  const store = loadStore({
    getHistoryPage: async (filter) => {
      filters.push(filter);
      page += 1;
      return page === 1
        ? { items: [item(1)], next_cursor_time: 10, next_cursor_id: 1, next_cursor_sort_value: 12, has_more: true }
        : { items: [item(2)], next_cursor_time: null, next_cursor_id: null, has_more: false };
    },
  });
  await store.getState().fetchHistory(true);
  await store.getState().fetchHistory(false);
  assert.equal(filters[1].cursor_sort_value, 12);
  assert.equal(store.getState().cursorSortValue, null);
});

test('selected dates use local calendar boundaries and calendar stepping', async () => {
  const filters = [];
  const store = loadStore({
    getHistoryPage: async (filter) => {
      filters.push(filter);
      return { items: [], next_cursor_time: null, next_cursor_id: null, has_more: false };
    },
  });
  store.setState({ selectedDate: '2024-03-10' });
  await store.getState().fetchHistory(true);
  const start = new Date(2024, 2, 10, 0, 0, 0, 0).getTime();
  const end = new Date(2024, 2, 11, 0, 0, 0, -1).getTime();
  assert.equal(filters[0].start_time, start);
  assert.equal(filters[0].end_time, end);

  store.getState().goToNextDay();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(store.getState().selectedDate, '2024-03-11');
});
