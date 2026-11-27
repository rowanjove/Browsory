const supportedBrowserNames: Record<string, string> = {
  chrome: 'Google Chrome',
  edge: 'Microsoft Edge',
  brave: 'Brave Browser',
  vivaldi: 'Vivaldi',
  opera: 'Opera',
  arc: 'Arc',
  firefox: 'Mozilla Firefox',
  zen: 'Zen Browser',
};

// Retain imported/custom sources and selections even when they are not currently
// discovered on this device. Known adapters remain discoverable in an empty DB.
export const browserFilterOptions = (sourceIds: Iterable<string>) =>
  Array.from(new Set([...Object.keys(supportedBrowserNames), ...sourceIds].filter(Boolean)))
    .map((id) => ({ id, label: supportedBrowserNames[id] || id }));
