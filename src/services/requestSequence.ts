/**
 * Small request epoch helper for views that can issue overlapping IPC calls.
 * A response may update state only when it belongs to the latest epoch.
 */
export interface RequestSequence {
  next: () => number;
  isCurrent: (sequence: number) => boolean;
}

export const createRequestSequence = (): RequestSequence => {
  let latest = 0;

  return {
    next: () => {
      latest += 1;
      return latest;
    },
    isCurrent: (sequence) => sequence === latest,
  };
};
