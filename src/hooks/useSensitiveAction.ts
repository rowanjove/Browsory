import { useLayoutEffect, useRef } from 'react';
import { assertSensitiveSessionCurrent, captureSensitiveSession } from '../services/vaultSession';

/** Bind native-dialog continuations to both the session and this mounted view. */
export function useSensitiveAction(active = true): () => () => void {
  const epoch = useRef(0);
  useLayoutEffect(() => {
    epoch.current++;
    return () => { epoch.current++; };
  }, [active]);

  return () => {
    if (!active) throw new Error('操作界面已关闭');
    const issuedAt = epoch.current;
    const session = captureSensitiveSession();
    return () => {
      assertSensitiveSessionCurrent(session);
      if (epoch.current !== issuedAt) throw new Error('操作界面已变化，旧操作已取消');
    };
  };
}
