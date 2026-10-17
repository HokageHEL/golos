import { invoke, isTauri } from '@tauri-apps/api/core';
import type { Segment } from '../../lib/transcript/types';

export type Note = {
  id: string; title: string; createdAt: number; durationSeconds: number;
  status: 'ready' | 'processing' | 'done' | 'failed'; segments: Segment[]; error: string | null;
};
export type Setup = { modelReady: boolean; modelName: string | null; dataDirectory: string; diarizationReady: boolean };
export const nativeAvailable = isTauri();
export const api = {
  setup: () => invoke<Setup>('setup_status'),
  list: () => invoke<Note[]>('list_notes'),
  importAudio: () => invoke<Note | null>('import_audio'),
  importModel: () => invoke<boolean>('import_model'),
  importSpeakerModels: () => invoke<boolean>('import_diarization_models'),
  detectSpeakers: (id: string, speakerCount: number | null) => invoke<Segment[]>('detect_speakers', { id, speakerCount }),
  transcribe: (id: string, language: string) => invoke<void>('transcribe', { id, language }),
  cancel: () => invoke<void>('cancel_transcription'),
  save: (id: string, segments: Segment[]) => invoke<void>('save_note', { id, segments }),
  delete: (id: string) => invoke<void>('delete_note', { id }),
  export: (format: 'txt' | 'srt', content: string) => invoke<boolean>('export_transcript', { format, content }),
  // Returns the local path of the recording's normalized audio after Rust has
  // checked that the note exists and authorized only that file for playback.
  audioSource: (id: string) => invoke<string>('audio_source', { id }),
};
