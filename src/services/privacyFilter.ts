import { PrivacyRule } from '../types';

function parseIpv4(host: string): [number, number, number, number] | null {
  const parts = host.split('.');
  if (parts.length !== 4) return null;
  const octets: number[] = [];
  for (const part of parts) {
    if (!/^\d{1,3}$/.test(part)) return null;
    const n = Number(part);
    if (!Number.isInteger(n) || n < 0 || n > 255) return null;
    octets.push(n);
  }
  return [octets[0], octets[1], octets[2], octets[3]];
}

function isRestrictedIpv4(octets: [number, number, number, number]): boolean {
  const [a, b, c] = octets;
  return (
    a === 0 || // 0.0.0.0/8
    a === 10 || // 10.0.0.0/8
    a === 127 || // 127.0.0.0/8
    (a === 100 && (b & 0xc0) === 64) || // 100.64.0.0/10 CGNAT
    (a === 169 && b === 254) || // 169.254.0.0/16 Link-local
    (a === 172 && b >= 16 && b <= 31) || // 172.16.0.0/12
    (a === 192 && b === 0 && (c === 0 || c === 2)) || // 192.0.0.0/24, 192.0.2.0/24
    (a === 192 && b === 168) || // 192.168.0.0/16
    (a === 198 && (b === 18 || b === 19)) || // 198.18.0.0/15
    (a === 198 && b === 51 && c === 100) || // 198.51.100.0/24
    (a === 203 && b === 0 && c === 113) || // 203.0.113.0/24
    a >= 224 // Multicast / Reserved
  );
}

function isRestrictedIpv6(rawHost: string): boolean {
  const unbracketed = rawHost.startsWith('[') && rawHost.endsWith(']')
    ? rawHost.slice(1, -1)
    : rawHost;
  const zoneIndex = unbracketed.indexOf('%');
  const addr = (zoneIndex >= 0 ? unbracketed.slice(0, zoneIndex) : unbracketed).toLowerCase();
  if (!addr.includes(':')) return false;
  if (addr === '::' || addr === '::1') return true;

  if (addr.startsWith('::ffff:')) {
    const mapped = addr.slice('::ffff:'.length);
    const v4 = parseIpv4(mapped);
    if (v4) return isRestrictedIpv4(v4);
    const hexParts = mapped.split(':');
    if (hexParts.length === 2 && /^[0-9a-f]{1,4}$/.test(hexParts[0]) && /^[0-9a-f]{1,4}$/.test(hexParts[1])) {
      const hi = parseInt(hexParts[0], 16);
      const lo = parseInt(hexParts[1], 16);
      return isRestrictedIpv4([(hi >> 8) & 0xff, hi & 0xff, (lo >> 8) & 0xff, lo & 0xff]);
    }
  }

  const firstColon = addr.indexOf(':');
  const firstHextetStr = firstColon === 0 ? '0' : addr.slice(0, firstColon);
  if (!/^[0-9a-f]{1,4}$/.test(firstHextetStr)) return true;
  const firstWord = parseInt(firstHextetStr, 16);

  // Unique local (fc00::/7) and link-local (fe80::/10)
  if ((firstWord & 0xfe00) === 0xfc00) return true;
  if ((firstWord & 0xffc0) === 0xfe80) return true;
  // Non global unicast outside 2000::/3
  if ((firstWord & 0xe000) !== 0x2000) return true;

  // Documentation 2001:db8::/32 and special-use 2001:0002/001x
  const parts = addr.split(':');
  const secondWord = parts[1] && /^[0-9a-f]{1,4}$/.test(parts[1]) ? parseInt(parts[1], 16) : 0;
  if (firstWord === 0x2001 && (secondWord === 0x0db8 || secondWord === 0x0002 || (secondWord & 0xfff0) === 0x0010)) {
    return true;
  }
  return false;
}

export function isNonPublicUrl(rawUrl: string): boolean {
  let parsed: URL;
  try {
    parsed = new URL(rawUrl.trim());
  } catch {
    return true;
  }

  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    return true;
  }

  const host = parsed.hostname.toLowerCase();
  if (!host) return true;

  if (
    host === 'localhost' ||
    host.endsWith('.localhost') ||
    host.endsWith('.local') ||
    host.endsWith('.internal') ||
    host.endsWith('.onion')
  ) {
    return true;
  }

  const ipv4 = parseIpv4(host);
  if (ipv4) return isRestrictedIpv4(ipv4);
  if (host.includes(':') || host.startsWith('[')) return isRestrictedIpv6(host);

  return !host.includes('.');
}

export function matchesPrivacyRule(pattern: string, domain: string, url: string): boolean {
  const trimmedPattern = pattern.trim().toLowerCase();
  if (!trimmedPattern) return false;

  if (trimmedPattern === 'ip:non-public') {
    return isNonPublicUrl(url);
  }

  const dLower = (domain || '').toLowerCase();
  const uLower = (url || '').toLowerCase();

  if (trimmedPattern.startsWith('*.')) {
    const suffix = trimmedPattern.slice(2);
    return Boolean(suffix) && (dLower === suffix || dLower.endsWith(`.${suffix}`));
  }

  return dLower.includes(trimmedPattern) || uLower.includes(trimmedPattern);
}

export function filterRecordsForAi<T extends { url: string; domain: string }>(
  items: T[],
  rules: PrivacyRule[]
): T[] {
  const activeRules = rules.filter((r) => r.enabled);
  return items.filter((item) => {
    const lowerUrl = (item.url || '').trim().toLowerCase();
    if (!lowerUrl.startsWith('http://') && !lowerUrl.startsWith('https://')) {
      return false;
    }
    for (const rule of activeRules) {
      if (matchesPrivacyRule(rule.pattern, item.domain, item.url)) {
        return false;
      }
    }
    return true;
  });
}
