import { invoke, isTauri } from '@tauri-apps/api/core';
import type { Segment } from './transcript/types';

export type Note = {
  id: string; title: string; createdAt: number; durationSeconds: number;
  folder: string; tags: string[];
  status: 'ready' | 'processing' | 'done' | 'failed'; segments: Segment[]; error: string | null;
};
export type RecordingStatus = { elapsedSeconds: number; stopped: boolean; paused: boolean; error: string | null };
export type Setup = { modelReady: boolean; modelName: string | null; dataDirectory: string; diarizationReady: boolean };
export type TextExportFormat = 'txt' | 'srt' | 'vtt' | 'md' | 'html' | 'csv' | 'json';
export type Language = { code: string; name: string };
export type Job = { id: number; noteId: string; title: string; durationSeconds: number; language: string; status: 'queued' | 'running' | 'done' | 'failed' | 'cancelled'; error: string | null };
export const nativeAvailable = isTauri();
export const api = {
  setup: () => invoke<Setup>('setup_status'),
  languages: () => invoke<Language[]>('transcription_languages'),
  list: (filters: { search?: string; status?: string; folder?: string; tag?: string; offset?: number } = {}) => invoke<Note[]>('list_notes', filters),
  get: (id: string) => invoke<Note>('get_note', { id }),
  organize: (id: string, title: string, folder: string, tags: string[]) => invoke<void>('organize_note', { id, title, folder, tags }),
  startRecording: (title: string, source = 'microphone', application = 0) => invoke<void>('start_recording', { title, source, application }),
  captureInfo: (applications = false) => invoke<{ system: boolean; mixed: boolean } | { pid: number; name: string }[]>('capture_info', { applications }),
  pauseRecording: (paused: boolean) => invoke<void>('pause_recording', { paused }),
  recordingStatus: () => invoke<RecordingStatus>('recording_status'),
  stopRecording: (discard: boolean) => invoke<{ note: Note; warning: string | null } | null>('stop_recording', { discard }),
  importAudio: () => invoke<Note | null>('import_audio'),
  importBatch: (language: string) => invoke<{ notes: Note[]; failures: string[]; cancelled: boolean }>('import_batch', { language }),
  jobs: () => invoke<Job[]>('list_jobs'),
  queue: (id: string, language: string) => invoke<void>('queue_note', { id, language }),
  jobAction: (id: number, action: 'retry' | 'remove') => invoke<void>('job_action', { id, action }),
  runQueue: () => invoke<void>('run_queue'),
  downloadModels: (speakers: boolean) => invoke<void>('download_models', { speakers }),
  importModel: () => invoke<boolean>('import_model'),
  importSpeakerModels: () => invoke<boolean>('import_diarization_models'),
  detectSpeakers: (id: string, speakerCount: number | null) => invoke<Segment[]>('detect_speakers', { id, speakerCount }),
  transcribe: (id: string, language: string) => invoke<void>('transcribe', { id, language }),
  cancel: () => invoke<void>('cancel_transcription'),
  save: (id: string, segments: Segment[]) => invoke<void>('save_note', { id, segments }),
  delete: (id: string) => invoke<void>('delete_note', { id }),
  export: (format: TextExportFormat, content: string) => invoke<boolean>('export_transcript', { format, content }),
  exportSaved: (id: string, format: 'docx' | 'wav' | 'golos') => invoke<boolean>('export_saved', { id, format }),
  importProject: () => invoke<Note | null>('import_project'),
  // Returns the local path of the recording's normalized audio after Rust has
  // checked that the note exists and authorized only that file for playback.
  audioSource: (id: string) => invoke<string>('audio_source', { id }),
};
