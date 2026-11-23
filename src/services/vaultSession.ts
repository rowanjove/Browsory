// UI response freshness only. Rust remains the authorization boundary.
let generation = 0;
let accepting = false;

/** A snapshot used to bind a user action to the session that started it. */
export type SensitiveSessionToken = number;

export function captureSensitiveSession(): SensitiveSessionToken {
  if (!accepting) throw new Error('保险库界面已锁定，敏感操作已取消');
  return generation;
}

export function isSensitiveSessionCurrent(token: SensitiveSessionToken): boolean {
  return accepting && generation === token;
}

export function assertSensitiveSessionCurrent(token: SensitiveSessionToken): void {
  if (!isSensitiveSessionCurrent(token)) {
    throw new Error('保险库会话已变化，旧操作已取消');
  }
}

export function closeSensitiveSession(): void {
  if (accepting) generation++;
  accepting = false;
}

export function openSensitiveSession(): void {
  if (!accepting) generation++;
  accepting = true;
}

export async function sensitiveRequest<T>(request: () => Promise<T>): Promise<T> {
  if (!accepting) throw new Error('保险库界面已锁定，敏感请求已取消');
  const issuedIn = generation;
  let value: T;
  try {
    value = await request();
  } catch (error) {
    if (!accepting || issuedIn !== generation) throw new Error('保险库会话已变化，旧请求已取消');
    throw error;
  }
  if (!accepting || issuedIn !== generation) {
    throw new Error('保险库会话已变化，旧请求结果已丢弃');
  }
  return value;
}
