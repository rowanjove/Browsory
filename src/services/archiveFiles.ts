import { invoke as rawInvoke } from '@tauri-apps/api/core';
import { sensitiveRequest } from './vaultSession';

const isTauri = typeof window !== 'undefined' && Boolean((window as any).__TAURI_INTERNALS__);

export interface ArchiveFileInfo {
  relative_name: string;
  size_bytes: number;
  sha256: string;
  kind: 'image' | 'text' | 'document' | 'file' | string;
}

export interface ArchiveFilePreview {
  relative_name: string;
  mime_type: string;
  bytes: number[];
  sha256: string;
}

const request = <T>(command: string, args: Record<string, unknown>) =>
  sensitiveRequest(() => rawInvoke<T>(command, args));

export const archiveFiles = {
  list: (pageUuid: string): Promise<ArchiveFileInfo[]> =>
    isTauri ? request('list_archive_files', { pageUuid }) : Promise.resolve([]),
  preview: (pageUuid: string, relativeName: string): Promise<ArchiveFilePreview> =>
    request('preview_archive_file', { pageUuid, relativeName }),
  export: (pageUuid: string, relativeName: string, targetPath: string): Promise<void> =>
    request('export_archive_file', { pageUuid, relativeName, targetPath }),
};
