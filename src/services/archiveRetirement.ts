import { invoke } from '@tauri-apps/api/core';
import { sensitiveRequest } from './vaultSession';
export interface LegacySourcePage {
  page_uuid: string; file_count: number; total_bytes: number;
  can_retire: boolean; resuming: boolean; reason: string | null;
}
export const archiveRetirement = {
  list: (offset=0) => sensitiveRequest(() => invoke<LegacySourcePage[]>('list_legacy_archive_sources',{offset})),
  // Only a commit receipt; a later lock must not relabel completed removal.
  retire: (pageUuid: string, password: string, confirmation: string) => invoke<{page_uuid:string;recovery_relative_path:string}>(
    'retire_legacy_archive_source', { pageUuid, password, confirmation }),
};
