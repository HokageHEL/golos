import { convertFileSrc, invoke, isTauri } from '@tauri-apps/api/core';
import type { Segment } from '../../lib/transcript/types';

export type Note = {
  id: string; title: string; createdAt: number; durationSeconds: number;
  status: 'ready' | 'processing' | 'done' | 'failed'; segments: Segment[]; error: string | null;
};
export type Setup = { modelReady: boolean; modelName: string | null; dataDirectory: string };
export const nativeAvailable = isTauri();
export const api = {
  setup: () => invoke<Setup>('setup_status'),
  list: () => invoke<Note[]>('list_notes'),
  audioSource: async (id: string) => convertFileSrc(await invoke<string>('recording_audio', { id })),
  importAudio: () => invoke<Note | null>('import_audio'),
  importModel: () => invoke<boolean>('import_model'),
  transcribe: (id: string, language: string) => invoke<void>('transcribe', { id, language }),
  cancel: () => invoke<void>('cancel_transcription'),
  save: (id: string, segments: Segment[]) => invoke<void>('save_note', { id, segments }),
  delete: (id: string) => invoke<void>('delete_note', { id }),
  export: (format: 'txt' | 'srt', content: string) => invoke<boolean>('export_transcript', { format, content }),
};
