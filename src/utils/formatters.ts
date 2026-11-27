export type SupportedLocale = 'zh-CN' | 'en-US';

export function formatRelativeTimestamp(
  ms: number,
  language: SupportedLocale,
  unknownLabel = 'Unknown'
): string {
  if (!Number.isFinite(ms) || ms <= 0) {
    return unknownLabel;
  }

  const diffSeconds = Math.round((ms - Date.now()) / 1000);
  const absSeconds = Math.abs(diffSeconds);

  if (absSeconds < 60) {
    return language === 'en-US' ? 'just now' : '刚刚';
  }

  const rtf = new Intl.RelativeTimeFormat(language, { numeric: 'auto' });

  if (absSeconds < 3600) {
    return rtf.format(Math.trunc(diffSeconds / 60), 'minute');
  }
  if (absSeconds < 86400) {
    return rtf.format(Math.trunc(diffSeconds / 3600), 'hour');
  }
  const diffDays = Math.trunc(diffSeconds / 86400);
  if (Math.abs(diffDays) < 30) {
    return rtf.format(diffDays, 'day');
  }

  return new Intl.DateTimeFormat(language, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  }).format(new Date(ms));
}

export function formatDurationCompact(seconds: number, language: SupportedLocale): string {
  if (!Number.isFinite(seconds) || seconds <= 0) {
    return '-';
  }
  const rounded = Math.round(seconds);
  if (rounded < 60) {
    return language === 'en-US' ? `${rounded}s` : `${rounded} 秒`;
  }
  const minutes = Math.floor(rounded / 60);
  const remSeconds = rounded % 60;
  if (minutes < 60) {
    if (remSeconds === 0) {
      return language === 'en-US' ? `${minutes}m` : `${minutes} 分`;
    }
    return language === 'en-US' ? `${minutes}m ${remSeconds}s` : `${minutes} 分 ${remSeconds} 秒`;
  }
  const hours = Math.floor(minutes / 60);
  const remMinutes = minutes % 60;
  return language === 'en-US' ? `${hours}h ${remMinutes}m` : `${hours} 小时 ${remMinutes} 分`;
}
