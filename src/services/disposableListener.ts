/**
 * Registers an async event subscription and guarantees immediate cleanup
 * if the caller disposes before the subscription promise resolves.
 */
export function bindDisposableListener(subscribe: () => Promise<() => void>): () => void {
  let disposed = false;
  let unlisten: (() => void) | undefined;

  try {
    Promise.resolve(subscribe())
      .then((fn) => {
        if (disposed) {
          fn?.();
        } else {
          unlisten = fn;
        }
      })
      .catch((error) => {
        console.warn('Disposable listener registration failed:', error);
      });
  } catch (error) {
    console.warn('Disposable listener initialization skipped:', error);
  }

  return () => {
    disposed = true;
    const fn = unlisten;
    unlisten = undefined;
    fn?.();
  };
}
