export function validateExternalHttpUrl(rawUrl: string): string {
  const trimmed = rawUrl.trim();
  if (!trimmed) {
    throw new Error('Empty URL cannot be opened.');
  }
  let parsed: URL;
  try {
    parsed = new URL(trimmed);
  } catch {
    throw new Error(`Invalid external URL: ${trimmed}`);
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    throw new Error(`Blocked unsupported external URL scheme: ${parsed.protocol}`);
  }
  if (!parsed.hostname) {
    throw new Error('External URL must include a valid host.');
  }
  return parsed.toString();
}
