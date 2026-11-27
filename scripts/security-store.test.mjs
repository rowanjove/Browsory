import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import vm from 'node:vm';
import test from 'node:test';
import ts from 'typescript';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const require = createRequire(import.meta.url);
const root = new URL('../', import.meta.url);

test('all sensitive IPC results are rejected after locking and unlocking a new session', async () => {
  const session = loadModule('src/services/vaultSession.ts', {});
  session.openSensitiveSession();
  let resolve;
  let followupRequests = 0;
  const result = session.sensitiveRequest(() => new Promise((done) => { resolve = done; }))
    .then(() => { followupRequests++; });
  session.closeSensitiveSession();
  session.openSensitiveSession();
  resolve({ privateAiContext: 'old session' });
  await assert.rejects(result, /旧请求结果已丢弃/);
  assert.equal(followupRequests, 0);
  assert.equal(await session.sensitiveRequest(async () => 'new session'), 'new session');
});

test('locked IPC does not dispatch new sensitive work', async () => {
  const session = loadModule('src/services/vaultSession.ts', {});
  let dispatched = false;
  await assert.rejects(session.sensitiveRequest(async () => { dispatched = true; }), /已锁定/);
  assert.equal(dispatched, false);
});

test('an expired rejection cannot masquerade as a current backend lock event', async () => {
  const session = loadModule('src/services/vaultSession.ts', {});
  session.openSensitiveSession();
  let reject;
  const result = session.sensitiveRequest(() => new Promise((_, fail) => { reject = fail; }));
  session.closeSensitiveSession();
  session.openSensitiveSession();
  reject(new Error('VaultLocked'));
  await assert.rejects(result, /旧请求已取消/);
});

function loadModule(path, dependencies, globals = {}) {
  const source = readFileSync(new URL(path, root), 'utf8')
    .replaceAll('import.meta.env.DEV', String(globals.dev ?? false));
  const { outputText } = ts.transpileModule(source, {
    fileName: path,
    compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, esModuleInterop: true },
  });
  const exports = {};
  vm.runInNewContext(outputText, {
    exports,
    require: (id) =>
      dependencies[id] ??
      (id.endsWith('/vaultPassphrase')
        ? loadModule('src/services/vaultPassphrase.ts', {}, globals)
        : id.endsWith('/syncFeedback')
        ? loadModule('src/services/syncFeedback.ts', {})
        : id === './externalUrl'
          ? loadModule('src/services/externalUrl.ts', {}, { URL })
          : id === '../../hooks/useDialogFocus'
            ? loadModule('src/hooks/useDialogFocus.ts', {})
            : id === '../../stores/useAppStore'
              ? { useAppStore: () => ({ language: 'zh-CN', t: (key) => key }) }
              : require(id)),
    console: { info() {}, error() {}, warn() {} },
    setTimeout() {},
    URLSearchParams,
    window: { location: { search: '?preview' }, addEventListener() {} },
    ...globals,
  }, { filename: path });
  return exports;
}

test('vault setup generates matching fields, copies only on request and submits the generated passphrase', async () => {
  const states = [];
  let cursor = 0;
  let copied;
  let submitted;
  const generated = 'SecureGenerated_2026x';
  const { LockScreen } = loadModule('src/components/Layout/LockScreen.tsx', {
    react: { ...React,
      useState: (initial) => {
        const index = cursor++;
        if (!(index in states)) states[index] = initial;
        return [states[index], (value) => { states[index] = typeof value === 'function' ? value(states[index]) : value; }];
      },
      useEffect() {}, useRef: () => ({ current: null }),
    },
    '../../hooks/useDialogFocus': { useDialogFocus: () => ({ current: null }) },
    '../../services/vaultPassphrase': { generateVaultPassphrase: () => generated },
    '../../stores/useSecurityStore': { useSecurityStore: () => ({
      vaultStatus: 'unconfigured',
      createVault: async (password) => { submitted = password; return 'test-recovery-key'; },
    }) },
    'lucide-react': new Proxy({}, { get: () => () => null }),
  }, { navigator: { clipboard: { writeText: async (text) => { copied = text; } } } });
  const render = () => { cursor = 0; return LockScreen(); };
  const nodes = (node) => {
    if (!node || typeof node !== 'object') return [];
    return [node, ...React.Children.toArray(node.props?.children).flatMap(nodes)];
  };
  const button = (tree, text) => nodes(tree).find((node) => node.type === 'button' && React.Children.toArray(node.props.children).includes(text));
  let tree = render();
  button(tree, '随机生成口令').props.onClick();
  tree = render();
  const fields = nodes(tree).filter((node) => node.type === 'input');
  assert.deepEqual(fields.map((node) => node.props.value), [generated, generated]);
  assert.equal(fields[0].props.type, 'text');
  assert.equal(copied, undefined);
  await button(tree, '复制口令').props.onClick();
  assert.equal(copied, generated);
  await nodes(tree).find((node) => node.type === 'form').props.onSubmit({ preventDefault() {} });
  assert.equal(submitted, generated);
  tree = render();
  assert.ok(nodes(tree).filter((node) => node.type === 'input').every((node) => node.props.value === ''));
});

test('a committed plaintext-retirement status survives frontend session invalidation', async () => {
  const session = loadModule('src/services/vaultSession.ts', {});
  session.openSensitiveSession();
  let finish;
  const calls = [];
  const { tauriApi } = loadModule('src/services/tauri.ts', {
    '@tauri-apps/api/core': { invoke: (command, args) => {
      calls.push([command, args]);
      return new Promise((resolve) => { finish = resolve; });
    } },
    '@tauri-apps/plugin-opener': { openUrl() {} },
    './vaultSession': session, './mockData': {},
  }, { window: { __TAURI_INTERNALS__: {}, dispatchEvent() {} } });
  const result = tauriApi.retireLegacyPlaintextBackup('test password', '移除旧明文副本');
  session.closeSensitiveSession();
  finish('completed removal');
  assert.equal(await result, 'completed removal');
  assert.equal(calls[0][0], 'retire_legacy_plaintext_backup');
  assert.equal(calls[0][1].confirmation, '移除旧明文副本');
});

function setup(options = {}) {
  let pending = options.pending ?? false;
  let status = options.status ?? 'locked';
  let syncs = 0;
  let regenerated = 0;
  let passiveRegenerated = 0;
  const initial = 'BHA-INITIAL-TEST-KEY';
  const replacement = 'BHA-REISSUED-TEST-KEY';
  let currentKey = initial;
  let pinRecoveryKey = options.pinRecoveryKey ?? 'BHA-PIN-RECOVERY-KEY';
  const calls = [];
  const api = {
    async createVault() { status = 'unlocked'; pending = true; return initial; },
    async migrateLegacyDatabase() { status = 'unlocked'; pending = true; return initial; },
    async unlockVault() {
      calls.push('unlock-start');
      if (options.unlockPromise) {
        const result = await options.unlockPromise;
        if (result) status = 'unlocked';
        calls.push('unlock-finish');
        return result;
      }
      status = 'unlocked'; return true;
    },
    async unlockVaultWithRecoveryKey() { status = 'unlocked'; return true; },
    async lockVault() {
      calls.push('lock-start');
      if (options.lockPromise) await options.lockPromise;
      if (options.failLock) throw new Error('backend lock failed');
      status = 'locked';
    },
    async restoreDatabaseBackupFile(source, password, recoveryKey) {
      calls.push(['restore', source, password, recoveryKey]);
      if (options.restorePromise) await options.restorePromise;
      if (options.failRestore) throw new Error('backup validation failed');
      status = 'locked';
      return 'restored';
    },
    async restoreDatabaseBackup(id, password, recoveryKey) {
      return this.restoreDatabaseBackupFile(id, password, recoveryKey);
    },
    async getVaultStatus() {
      if (options.vaultStatusPromise) return options.vaultStatusPromise;
      return {
        status,
        recovery_key_pending: pending,
        has_envelope: true,
        has_dpapi_wrap: options.hasDpapiWrap ?? false,
        has_plaintext_db: false,
        is_portable: true,
      };
    },
    async resumePassiveVault(pin) {
      calls.push(pin ? `resumePassive:${pin}` : 'resumePassive');
      if (options.failPassiveResume) throw new Error(options.failPassiveResume);
      if (options.pinEnabled) {
        if (!pin) {
          return { unlocked: false, pin_required: true, is_locked_out: false, lockout_remaining_secs: 0, error_message: null };
        }
        if (pin !== (options.expectedPin ?? '2580')) {
          return { unlocked: false, pin_required: true, is_locked_out: false, lockout_remaining_secs: 0, error_message: 'PIN 码错误，还剩 4 次重试机会' };
        }
      }
      status = 'unlocked';
      return { unlocked: true, pin_required: false, is_locked_out: false, lockout_remaining_secs: 0, error_message: null };
    },
    async resetPinWithRecoveryKey(recoveryKey, newPin) {
      calls.push(['resetPinWithRecoveryKey', recoveryKey, newPin]);
      if (recoveryKey !== pinRecoveryKey) {
        throw new Error('安全恢复密钥校验失败，无法重置 PIN 码');
      }
      options.expectedPin = newPin;
      options.pinEnabled = true;
      status = 'unlocked';
    },
    async getSecurityState() {
      options.onSecurityState?.();
      if (options.securityStatePromise) return options.securityStatePromise;
      return { pin_enabled: options.pinEnabled ?? false, auto_lock_minutes: 15, lock_on_minimize: false, lock_on_sleep: true, is_locked_out: false, lockout_remaining_secs: 0 };
    },
    async regenerateVaultRecoveryKey(password) {
      assert.equal(password, 'verified-password');
      regenerated++;
      currentKey = replacement;
      return replacement;
    },
    async regeneratePassiveVaultRecoveryKey() {
      passiveRegenerated++;
      currentKey = replacement;
      return replacement;
    },
    async confirmVaultRecoveryKey(key) { assert.equal(key, currentKey); pending = false; },
    async syncAll() {
      syncs++;
      if (options.syncAllError) throw new Error(options.syncAllError);
      return [];
    },
    async getRecentImportJobs() { return []; },
  };
  let invalidations = 0;
  const history = { getState: () => ({ items: [], clearMemoryCache() {}, invalidateSession() { invalidations++; }, async fetchHistory() {} }) };
  const { useSecurityStore: store } = loadModule('src/stores/useSecurityStore.ts', {
    '../services/tauri': { tauriApi: api }, './useHistoryStore': { useHistoryStore: history },
    '../services/vaultSession': loadModule('src/services/vaultSession.ts', {}),
  }, {
    dev: options.dev ?? false,
    ...(options.runTimersImmediately ? { setTimeout: (fn) => { fn(); return 0; } } : {}),
  });
  return { store, initial, replacement, calls, getBackendStatus: () => status, getSyncs: () => syncs, getRegenerated: () => regenerated, getPassiveRegenerated: () => passiveRegenerated, getInvalidations: () => invalidations };
}

test('restore protects the UI immediately, invalidates history and finishes locked without syncing', async () => {
  let finish;
  const restorePromise = new Promise((resolve) => { finish = resolve; });
  const { store, getInvalidations, getSyncs, calls } = setup({ status: 'unlocked', restorePromise });
  store.setState({ vaultStatus: 'unlocked', isLocked: false });
  const restoring = store.getState().restoreVaultBackup('imported.browsory-backup', 'source-password');
  assert.equal(store.getState().isLocked, true);
  assert.equal(store.getState().isVaultRestoring, true);
  assert.equal(getInvalidations(), 1);
  assert.equal(await store.getState().unlockVault('must-not-dispatch'), false);
  const { LockScreen } = loadModule('src/components/Layout/LockScreen.tsx', {
    '../../stores/useSecurityStore': { useSecurityStore: () => store.getState() },
    'lucide-react': new Proxy({}, { get: () => () => null }),
  });
  assert.match(renderToStaticMarkup(React.createElement(LockScreen)), /正在校验并恢复备份/);
  finish();
  assert.equal(await restoring, 'restored');
  assert.equal(store.getState().isVaultRestoring, false);
  assert.equal(store.getState().vaultStatus, 'locked');
  assert.equal(store.getState().isLocked, true);
  assert.equal(getSyncs(), 0);
  assert.deepEqual(calls, [['restore', 'imported.browsory-backup', 'source-password', undefined]]);
});

test('restore validation failure refreshes the still-open backend without claiming it is locked', async () => {
  const { store, getBackendStatus } = setup({ status: 'unlocked', failRestore: true });
  store.setState({ vaultStatus: 'unlocked', isLocked: false, hasSyncedAfterStart: true });
  await assert.rejects(store.getState().restoreVaultBackup(7), /backup validation failed/);
  assert.equal(getBackendStatus(), 'unlocked');
  assert.equal(store.getState().vaultStatus, 'unlocked');
  assert.equal(store.getState().isLocked, false);
  assert.equal(store.getState().isVaultRestoring, false);
});

test('a lock requested during restore runs afterward and cannot be undone by its status refresh', async () => {
  let finish;
  const { store, calls } = setup({ status: 'unlocked', restorePromise: new Promise((resolve) => { finish = resolve; }) });
  store.setState({ vaultStatus: 'unlocked', isLocked: false });
  const restoring = store.getState().restoreVaultBackup(7);
  const locking = store.getState().lockVault();
  finish();
  await Promise.all([restoring, locking]);
  assert.deepEqual(calls, [['restore', 7, undefined, undefined], 'lock-start']);
  assert.equal(store.getState().vaultStatus, 'locked');
  assert.equal(store.getState().isLocked, true);
});

test('a pending unlock is followed by the actual backend lock in FIFO order', async () => {
  let resolveUnlock;
  const unlockPromise = new Promise((resolve) => { resolveUnlock = resolve; });
  const { store, calls, getBackendStatus } = setup({ unlockPromise });
  const unlocking = store.getState().unlockVault('verified-password');
  await Promise.resolve();
  assert.deepEqual(calls, ['unlock-start']);
  const locking = store.getState().lockVault();
  assert.equal(store.getState().isLocked, true);
  await Promise.resolve();
  assert.deepEqual(calls, ['unlock-start']);
  resolveUnlock(true);
  assert.equal(await unlocking, false);
  await locking;
  assert.deepEqual(calls, ['unlock-start', 'unlock-finish', 'lock-start']);
  assert.equal(getBackendStatus(), 'locked');
  assert.equal(store.getState().vaultStatus, 'locked');
});

for (const method of ['createVault', 'migrateLegacyDatabase']) {
  test(`${method} keeps recovery screen mounted until backend confirmation`, async () => {
    const { store, initial, getSyncs } = setup();
    assert.equal(await store.getState()[method]('verified-password'), initial);
    assert.equal(store.getState().isLocked, true);
    assert.equal(store.getState().pendingRecoveryKey, initial);
    await store.getState().checkSecurityState();
    assert.equal(store.getState().isLocked, true);
    assert.equal(store.getState().hasSyncedAfterStart, false);
    assert.equal(getSyncs(), 0);
    await store.getState().confirmVaultRecoveryKey(initial);
    assert.equal(store.getState().isLocked, false);
    assert.equal(store.getState().pendingRecoveryKey, null);
    assert.equal(store.getState().recoveryKeyPending, false);
  });
}

test('password unlock after interrupted delivery reissues and renders the new key', async () => {
  const { store, replacement, getRegenerated } = setup({ pending: true });
  assert.equal(await store.getState().unlockVault('verified-password'), true);
  assert.equal(getRegenerated(), 1);
  assert.equal(store.getState().isLocked, true);
  assert.equal(store.getState().pendingRecoveryKey, replacement);
  const { LockScreen } = loadModule('src/components/Layout/LockScreen.tsx', {
    '../../stores/useSecurityStore': { useSecurityStore: () => store.getState() },
    'lucide-react': new Proxy({}, { get: () => () => null }),
  });
  assert.match(renderToStaticMarkup(React.createElement(LockScreen)), /BHA-REISSUED-TEST-KEY/);
  await store.getState().confirmVaultRecoveryKey(replacement);
  assert.equal(store.getState().isLocked, false);
});

test('recovery-key unlock can confirm the possessed key without password rotation', async () => {
  const { store, initial, getRegenerated } = setup({ pending: true });
  await store.getState().unlockVaultWithRecoveryKey(initial);
  assert.equal(getRegenerated(), 0);
  assert.equal(store.getState().pendingRecoveryKey, initial);
  await store.getState().confirmVaultRecoveryKey(initial);
  assert.equal(store.getState().isLocked, false);
});

test('production preview query cannot skip security initialization', () => {
  const { store } = setup();
  assert.equal(store.getState().isInitialized, false);
  assert.equal(store.getState().isLocked, true);
});

test('development preview remains available', () => {
  const { store } = setup({ dev: true });
  assert.equal(store.getState().isInitialized, true);
  assert.equal(store.getState().isLocked, false);
});

test('lock invalidates history before IPC and records a successful backend lock', async () => {
  const { store, getInvalidations } = setup({ status: 'unlocked' });
  store.setState({ vaultStatus: 'unlocked', isLocked: false, pinEnabled: true });
  await store.getState().lockVault();
  assert.equal(getInvalidations(), 1);
  assert.equal(store.getState().isLocked, true);
  assert.equal(store.getState().vaultStatus, 'locked');
});

test('lock failure keeps the backend status truthful and leaves the UI usable', async () => {
  const { store, getInvalidations } = setup({ status: 'unlocked', failLock: true });
  store.setState({ vaultStatus: 'unlocked', isLocked: false, pinEnabled: true });
  await store.getState().lockVault();
  assert.equal(getInvalidations(), 1);
  assert.equal(store.getState().isLocked, true);
  assert.equal(store.getState().isUiLocked, false);
  assert.equal(store.getState().vaultStatus, 'unlocked');
  assert.match(store.getState().vaultLockError, /backend lock failed/);
  const { LockScreen } = loadModule('src/components/Layout/LockScreen.tsx', {
    '../../stores/useSecurityStore': { useSecurityStore: () => store.getState() },
    'lucide-react': new Proxy({}, { get: () => () => null }),
  });
  const markup = renderToStaticMarkup(React.createElement(LockScreen));
  assert.match(markup, /保险库锁定未完成/);
  assert.match(markup, /backend lock failed/);
  assert.doesNotMatch(markup, /返回继续使用/);
  assert.match(markup, /重试锁定保险库/);
});

test('pending lock immediately renders a protected overlay before IPC completes', async () => {
  let resolveLock;
  const lockPromise = new Promise((resolve) => { resolveLock = resolve; });
  const { store } = setup({ status: 'unlocked', lockPromise });
  store.setState({ vaultStatus: 'unlocked', isLocked: false, pinEnabled: true });
  const lockAttempt = store.getState().lockVault();
  assert.equal(store.getState().isLocked, true);
  assert.equal(store.getState().vaultStatus, 'unlocked');
  const { LockScreen } = loadModule('src/components/Layout/LockScreen.tsx', {
    '../../stores/useSecurityStore': { useSecurityStore: () => store.getState() },
    'lucide-react': new Proxy({}, { get: () => () => null }),
  });
  assert.match(renderToStaticMarkup(React.createElement(LockScreen)), /正在锁定保险库/);
  resolveLock();
  await lockAttempt;
  assert.equal(store.getState().vaultStatus, 'locked');
});

test('late failed-lock status responses cannot clear the protected overlay', async () => {
  let resolveStatus;
  const vaultStatusPromise = new Promise((resolve) => { resolveStatus = resolve; });
  const { store } = setup({ status: 'unlocked', failLock: true, vaultStatusPromise });
  store.setState({ vaultStatus: 'unlocked', isLocked: false, pinEnabled: true });
  const lockAttempt = store.getState().lockVault();
  assert.equal(store.getState().isLocked, true);
  resolveStatus({ status: 'unlocked', recovery_key_pending: false, has_envelope: true, has_plaintext_db: false, is_portable: true });
  await lockAttempt;
  assert.equal(store.getState().isLocked, true);
  assert.equal(store.getState().vaultStatus, 'unlocked');
  assert.match(store.getState().vaultLockError, /backend lock failed/);
});

test('history requests cannot repopulate list, details, or collections after session invalidation', async () => {
  let resolveHistory;
  let resolveDetail;
  let resolveCollections;
  const api = {
    getHistoryPage: () => new Promise((resolve) => { resolveHistory = resolve; }),
    getVisitDetail: () => new Promise((resolve) => { resolveDetail = resolve; }),
    listSmartCollections: () => new Promise((resolve) => { resolveCollections = resolve; }),
  };
  const { useHistoryStore: history } = loadModule('src/stores/useHistoryStore.ts', {
    '../services/tauri': { tauriApi: api },
  });
  const listRequest = history.getState().fetchHistory(true);
  const detailRequest = history.getState().openDetail(7);
  const collectionsRequest = history.getState().loadSmartCollections();
  history.getState().invalidateSession();
  resolveHistory({ items: [{ id: 1 }], next_cursor_time: 1, next_cursor_id: 1, has_more: false });
  resolveDetail({ id: 7, title: 'secret' });
  resolveCollections([{ id: 1, name: 'secret collection' }]);
  await Promise.all([listRequest, detailRequest, collectionsRequest]);
  assert.equal(history.getState().items.length, 0);
  assert.equal(history.getState().detailData, null);
  assert.equal(history.getState().smartCollections.length, 0);
});

test('stale security status and unlock responses cannot reopen a locked session', async () => {
  let resolveStatus;
  let resolveUnlock;
  const statusPromise = new Promise((resolve) => { resolveStatus = resolve; });
  const unlockPromise = new Promise((resolve) => { resolveUnlock = resolve; });
  const { store } = setup({ status: 'unlocked', vaultStatusPromise: statusPromise, unlockPromise });
  store.setState({ vaultStatus: 'unlocked', isLocked: false, pinEnabled: true });
  const staleStatus = store.getState().checkSecurityState({ relockIfPinEnabled: false });
  store.getState().lock();
  resolveStatus({ status: 'unlocked', recovery_key_pending: false, has_envelope: true, has_plaintext_db: false, is_portable: true });
  await staleStatus;
  assert.equal(store.getState().isLocked, true);
  const staleUnlock = store.getState().unlockVault('verified-password');
  store.getState().lock();
  resolveUnlock(true);
  await staleUnlock;
  assert.equal(store.getState().isLocked, true);
  assert.equal(store.getState().isUiLocked, true);
});

test('a stale failed PIN-state query cannot reopen the view after a lock transition', async () => {
  let rejectState;
  let started;
  const ready = new Promise((resolve) => { started = resolve; });
  const securityStatePromise = new Promise((_, reject) => { rejectState = reject; });
  const { store } = setup({ status: 'unlocked', securityStatePromise, onSecurityState: started });
  store.setState({ vaultStatus: 'unlocked', isLocked: false, pinEnabled: true });
  const pending = store.getState().checkSecurityState({ relockIfPinEnabled: false });
  await ready;
  store.getState().lock();
  rejectState(new Error('old security-state response'));
  await pending;
  assert.equal(store.getState().isLocked, true);
  assert.equal(store.getState().isUiLocked, true);
});

test('passive DPAPI startup requires recovery-key confirmation before opening the archive', async () => {
  const { store, calls, replacement, getPassiveRegenerated, getSyncs } = setup({ status: 'locked', hasDpapiWrap: true, pending: true, pinEnabled: false });
  await store.getState().checkSecurityState();
  assert.equal(store.getState().isLocked, true);
  assert.equal(store.getState().isUiLocked, false);
  assert.equal(store.getState().hasDpapiWrap, true);
  assert.equal(store.getState().pinRequiredOnResume, false);
  assert.equal(store.getState().pendingRecoveryKey, replacement);
  assert.equal(store.getState().recoveryKeyPending, true);
  assert.equal(getPassiveRegenerated(), 1);
  assert.equal(calls.includes('resumePassive'), true);
  assert.equal(store.getState().hasSyncedAfterStart, false);
  assert.equal(getSyncs(), 0);
  await store.getState().confirmVaultRecoveryKey(replacement);
  assert.equal(store.getState().isLocked, false);
  assert.equal(getSyncs(), 0);
});

test('DPAPI resume failure keeps recovery-key unlock available and reports the failure', async () => {
  const { store } = setup({ status: 'locked', hasDpapiWrap: true, failPassiveResume: 'DPAPI key unavailable' });
  await store.getState().checkSecurityState();
  assert.equal(store.getState().isLocked, true);
  assert.match(store.getState().passiveResumeError, /DPAPI key unavailable/);
  const { LockScreen } = loadModule('src/components/Layout/LockScreen.tsx', {
    '../../stores/useSecurityStore': { useSecurityStore: () => store.getState() },
    'lucide-react': new Proxy({}, { get: () => () => null }),
  });
  const markup = renderToStaticMarkup(React.createElement(LockScreen));
  assert.match(markup, /DPAPI key unavailable/);
  assert.match(markup, /BHA-XXXX-XXXX-XXXX/);
  assert.doesNotMatch(markup, /本地加密工作区已休眠/);
});

test('passive DPAPI startup with 4-digit PIN pauses on PIN lock and resumes on valid PIN', async () => {
  const { store, calls } = setup({ status: 'locked', hasDpapiWrap: true, pinEnabled: true, expectedPin: '2580' });
  await store.getState().checkSecurityState();
  assert.equal(store.getState().isLocked, true);
  assert.equal(store.getState().isUiLocked, true);
  assert.equal(store.getState().pinRequiredOnResume, true);
  assert.equal(store.getState().hasSyncedAfterStart, false);

  await assert.rejects(() => store.getState().unlock('0000'), /PIN 码错误/);
  assert.equal(store.getState().isLocked, true);

  assert.equal(await store.getState().unlock('2580'), true);
  assert.equal(store.getState().isLocked, false);
  assert.equal(store.getState().isUiLocked, false);
  assert.equal(store.getState().pinRequiredOnResume, false);
  assert.equal(calls.includes('resumePassive:2580'), true);
  assert.equal(store.getState().hasSyncedAfterStart, true);
});

test('passive DPAPI startup with 4-digit PIN renders the PIN keypad and PIN recovery link instead of legacy passphrase form', async () => {
  const { store } = setup({ status: 'locked', hasDpapiWrap: true, pinEnabled: true, expectedPin: '2580' });
  await store.getState().checkSecurityState();
  const { LockScreen } = loadModule('src/components/Layout/LockScreen.tsx', {
    '../../stores/useSecurityStore': { useSecurityStore: () => store.getState() },
    'lucide-react': new Proxy({}, { get: () => () => null }),
  });
  const markup = renderToStaticMarkup(React.createElement(LockScreen));
  assert.match(markup, /请输入 4 位 PIN 码以继续访问/);
  assert.match(markup, /忘记 PIN 码？使用安全恢复密钥重置/);
  assert.doesNotMatch(markup, /请输入数据库主加密口令/);
  assert.doesNotMatch(markup, /数据库保险库已锁定/);
});

test('passive DPAPI locked with 4-digit PIN supports resetting PIN with recovery key and unlocking', async () => {
  const { store, calls } = setup({
    status: 'locked',
    hasDpapiWrap: true,
    pinEnabled: true,
    expectedPin: '2580',
    pinRecoveryKey: 'BHA-PIN-RECOVERY-KEY',
  });
  await store.getState().checkSecurityState();
  assert.equal(store.getState().pinRequiredOnResume, true);
  assert.equal(store.getState().isLocked, true);

  await store.getState().resetPinWithRecoveryKey('BHA-PIN-RECOVERY-KEY', '1357');
  assert.equal(store.getState().vaultStatus, 'unlocked');
  assert.equal(store.getState().pinRequiredOnResume, false);
  assert.equal(store.getState().isUiLocked, false);
  assert.equal(store.getState().isLocked, false);
  assert.deepEqual(
    calls.filter((c) => Array.isArray(c) && c[0] === 'resetPinWithRecoveryKey'),
    [['resetPinWithRecoveryKey', 'BHA-PIN-RECOVERY-KEY', '1357']],
  );

  await store.getState().lockVault();
  assert.equal(await store.getState().unlock('1357'), true);
  assert.equal(store.getState().isLocked, false);
});

test('first-unlock auto-sync ignores coordinator contention error instead of showing sync error banner', async () => {
  const { store, getSyncs } = setup({
    status: 'unlocked',
    hasDpapiWrap: true,
    pinEnabled: false,
    syncAllError: '已有同步任务正在执行 (startup)，请稍候',
    runTimersImmediately: true,
  });
  store.setState({ vaultStatus: 'unlocked', isLocked: false, hasSyncedAfterStart: false });
  await store.getState().triggerFirstUnlockSyncIfNeeded();
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(getSyncs(), 1);
  assert.equal(store.getState().isSyncing, false);
  assert.equal(store.getState().lastSyncError, null);
});
