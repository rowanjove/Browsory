import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import vm from 'node:vm';
import test from 'node:test';
import ts from 'typescript';
import { webcrypto } from 'node:crypto';

const require = createRequire(import.meta.url);
const root = new URL('../', import.meta.url);

test('vault passphrases use secure random bytes with uniform 64-character mapping', () => {
  let requests = 0;
  const { generateVaultPassphrase } = loadModule('src/services/vaultPassphrase.ts', {
    crypto: { getRandomValues(bytes) {
      requests++;
      assert.equal(bytes.length, 20);
      bytes.set(Array.from({ length: 20 }, (_, i) => 240 + i % 16));
      return bytes;
    } },
  });
  assert.equal(generateVaultPassphrase(), 'wxyz0123456789-_wxyz');
  assert.equal(requests, 1);
  const real = loadModule('src/services/vaultPassphrase.ts', { crypto: webcrypto });
  assert.match(real.generateVaultPassphrase(), /^[A-Za-z0-9_-]{20}$/);
  const unavailable = loadModule('src/services/vaultPassphrase.ts', {
    crypto: { getRandomValues() { throw new Error('unavailable'); } },
  });
  assert.throws(() => unavailable.generateVaultPassphrase(), /unavailable/);
});

function loadModule(relativePath, extraGlobals = {}) {
  const source = readFileSync(new URL(relativePath, root), 'utf8');
  const { outputText } = ts.transpileModule(source, {
    fileName: relativePath,
    compilerOptions: {
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2022,
      esModuleInterop: true,
    },
  });
  const module = { exports: {} };
  vm.runInNewContext(
    outputText,
    {
      module,
      exports: module.exports,
      require,
      Promise,
      URL,
      console: { info() {}, warn() {}, error() {} },
      setTimeout,
      clearTimeout,
      ...extraGlobals,
    },
    { filename: relativePath }
  );
  return module.exports;
}

test('privacyFilter blocks non-public IPs, local domains, wildcards, and non-HTTP schemes', () => {
  const { isNonPublicUrl, filterRecordsForAi } = loadModule('src/services/privacyFilter.ts');

  assert.equal(isNonPublicUrl('http://127.0.0.1:8080/dashboard'), true);
  assert.equal(isNonPublicUrl('http://10.0.0.5/intranet'), true);
  assert.equal(isNonPublicUrl('http://172.16.4.2/admin'), true);
  assert.equal(isNonPublicUrl('http://192.168.1.1/router'), true);
  assert.equal(isNonPublicUrl('http://169.254.169.254/latest/meta-data'), true);
  assert.equal(isNonPublicUrl('http://100.64.0.1/cgnat'), true);
  assert.equal(isNonPublicUrl('http://[::1]:3000/'), true);
  assert.equal(isNonPublicUrl('http://[fd12:3456:789a::1]/'), true);
  assert.equal(isNonPublicUrl('http://[fe80::1]/'), true);
  assert.equal(isNonPublicUrl('http://[::ffff:192.168.1.1]/'), true);
  assert.equal(isNonPublicUrl('https://localhost:5173/'), true);
  assert.equal(isNonPublicUrl('https://dev.localhost/'), true);
  assert.equal(isNonPublicUrl('http://nas.local/'), true);
  assert.equal(isNonPublicUrl('http://wiki.internal/'), true);
  assert.equal(isNonPublicUrl('file:///C:/secret.txt'), true);
  assert.equal(isNonPublicUrl('https://docs.rs/tauri/latest'), false);
  assert.equal(isNonPublicUrl('https://[2606:4700:4700::1111]/'), false);

  const items = [
    { url: 'https://docs.rs/tauri', domain: 'docs.rs' },
    { url: 'https://mail.corp.example/inbox', domain: 'mail.corp.example' },
    { url: 'http://192.168.1.1/login', domain: '192.168.1.1' },
    { url: 'file:///C:/Users/demo/notes.md', domain: 'local-file' },
    { url: 'https://public.disabled-rule.example/page', domain: 'public.disabled-rule.example' },
  ];

  const rules = [
    { id: 1, pattern: 'ip:non-public', rule_type: 'private', enabled: true, created_at: 1 },
    { id: 2, pattern: '*.corp.example', rule_type: 'private', enabled: true, created_at: 2 },
    { id: 3, pattern: 'disabled-rule.example', rule_type: 'private', enabled: false, created_at: 3 },
  ];

  const filtered = filterRecordsForAi(items, rules);
  assert.deepEqual(
    filtered.map((item) => item.url),
    ['https://docs.rs/tauri', 'https://public.disabled-rule.example/page']
  );
});

test('validateExternalHttpUrl strictly permits HTTP and HTTPS URLs only', () => {
  const { validateExternalHttpUrl } = loadModule('src/services/externalUrl.ts');

  assert.equal(validateExternalHttpUrl(' https://github.com/rowanjove/browsory '), 'https://github.com/rowanjove/browsory');
  assert.equal(validateExternalHttpUrl('http://example.com/path?a=1'), 'http://example.com/path?a=1');

  assert.throws(() => validateExternalHttpUrl(''), /Empty URL/);
  assert.throws(() => validateExternalHttpUrl('file:///C:/Windows/win.ini'), /Blocked unsupported external URL scheme/);
  assert.throws(() => validateExternalHttpUrl('javascript:alert(1)'), /Blocked unsupported external URL scheme/);
  assert.throws(() => validateExternalHttpUrl('custom://open-me'), /Blocked unsupported external URL scheme/);
  assert.throws(() => validateExternalHttpUrl('not a valid url'), /Invalid external URL/);
});

test('bindDisposableListener cleans up subscriptions that resolve after unmount', async () => {
  const { bindDisposableListener } = loadModule('src/services/disposableListener.ts');

  let resolveSubscribe;
  let unlistenCalls = 0;

  const cleanup = bindDisposableListener(
    () =>
      new Promise((resolve) => {
        resolveSubscribe = resolve;
      })
  );

  // Unmount runs before async registration settles.
  cleanup();
  resolveSubscribe(() => {
    unlistenCalls += 1;
  });

  await Promise.resolve();
  await Promise.resolve();
  assert.equal(unlistenCalls, 1);

  // Normal mount -> resolve -> unmount only calls unlisten once.
  let normalUnlisten = 0;
  const cleanupNormal = bindDisposableListener(async () => () => {
    normalUnlisten += 1;
  });
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(normalUnlisten, 0);
  cleanupNormal();
  cleanupNormal();
  assert.equal(normalUnlisten, 1);
});

test('vault session tokens invalidate native-dialog continuations after lock or re-open', () => {
  const {
    assertSensitiveSessionCurrent,
    captureSensitiveSession,
    closeSensitiveSession,
    openSensitiveSession,
  } = loadModule('src/services/vaultSession.ts');

  assert.throws(() => captureSensitiveSession(), /保险库界面已锁定/);

  openSensitiveSession();
  const token = captureSensitiveSession();
  assert.doesNotThrow(() => assertSensitiveSessionCurrent(token));

  closeSensitiveSession();
  assert.throws(() => assertSensitiveSessionCurrent(token), /保险库会话已变化/);

  openSensitiveSession();
  assert.throws(() => assertSensitiveSessionCurrent(token), /保险库会话已变化/);
});

test('createRequestSequence rejects out-of-order stale responses', () => {
  const { createRequestSequence } = loadModule('src/services/requestSequence.ts');

  const seq = createRequestSequence();
  const reqA = seq.next();
  assert.equal(seq.isCurrent(reqA), true);

  const reqB = seq.next();
  assert.equal(seq.isCurrent(reqA), false);
  assert.equal(seq.isCurrent(reqB), true);
});

test('native-dialog action guards cancel deferred work on close, unmount and session change', async () => {
  const session = loadModule('src/services/vaultSession.ts');
  session.openSensitiveSession();
  const epoch = { current: 0 };
  let cleanup;
  let lastActive;
  const { useSensitiveAction } = loadModule('src/hooks/useSensitiveAction.ts', {
    require: (name) => name === 'react' ? {
      useRef: () => epoch,
      useLayoutEffect: (setup, [active]) => {
        if (lastActive !== active) {
          cleanup?.();
          cleanup = setup();
          lastActive = active;
        }
      },
    } : session,
  });
  let writes = 0;
  const deferredDialog = (capture) => {
    const assertCurrent = capture();
    let resolve;
    const selection = new Promise((done) => { resolve = done; });
    const completed = selection.then(() => { assertCurrent(); writes++; });
    return { resolve, completed };
  };
  const closed = deferredDialog(useSensitiveAction(true));
  useSensitiveAction(false);
  useSensitiveAction(true);
  closed.resolve('/synthetic/export.json');
  await assert.rejects(closed.completed, /操作界面已变化/);

  const unmounted = deferredDialog(useSensitiveAction(true));
  cleanup();
  unmounted.resolve('/synthetic/export.json');
  await assert.rejects(unmounted.completed, /操作界面已变化/);

  const locked = deferredDialog(useSensitiveAction(true));
  session.closeSensitiveSession();
  session.openSensitiveSession();
  locked.resolve('/synthetic/export.json');
  await assert.rejects(locked.completed, /保险库会话已变化/);

  const current = deferredDialog(useSensitiveAction(true));
  current.resolve('/synthetic/export.json');
  await current.completed;
  assert.equal(writes, 1);
});
