import type { SyncResult } from '../types';

export function describeSyncResults(results: SyncResult[], language: 'zh-CN' | 'en-US' = 'zh-CN') {
  const failed = results.filter((r) => r.status === 'failed');
  const skipped = results.filter((r) => r.status === 'skipped').length;
  const succeeded = results.length - failed.length - skipped;
  const inserted = results.reduce((n, r) => n + r.inserted_count, 0);
  const duplicates = results.reduce((n, r) => n + r.duplicate_count, 0);
  const rejected = results.reduce((n, r) => n + r.failed_count, 0);
  const allFailed = failed.length > 0 && succeeded === 0;
  const en = language === 'en-US';
  const details = failed.map((r) => `${r.browser} / ${r.profile}: ${r.error || (en ? 'Sync failed' : '同步失败')}`).join('\n');
  let message: string;
  let type: 'success' | 'warning' | 'error' | 'info';
  if (failed.length) {
    type = allFailed ? 'error' : 'warning';
    message = en
      ? `${failed.length} source(s) failed; ${inserted} added, ${duplicates} duplicates. ${details}`
      : `${failed.length} 个数据源同步失败；新增 ${inserted} 条，去重 ${duplicates} 条。${details}`;
  } else if (rejected) {
    type = 'warning';
    message = en ? `${inserted} added; ${rejected} invalid records were skipped.` : `新增 ${inserted} 条；${rejected} 条无效记录未导入。`;
  } else if (!results.length || !succeeded) {
    type = 'info';
    message = en ? 'No sources need updating. Scan sources or import a file to add history.' : '没有需要更新的数据源。可重新扫描来源或导入历史文件。';
  } else {
    type = 'success';
    message = en ? `Sync complete: ${inserted} added, ${duplicates} duplicates.` : `同步完成：新增 ${inserted} 条，去重 ${duplicates} 条。`;
  }
  return { failed, skipped, succeeded, inserted, duplicates, rejected, allFailed, message, type };
}
