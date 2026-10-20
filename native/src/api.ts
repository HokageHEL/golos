import { invoke, isTauri } from '@tauri-apps/api/core';
import type { Segment } from './transcript/types';

export type Note = {
  id: string; title: string; createdAt: number; durationSeconds: number;
  status: 'ready' | 'processing' | 'done' | 'failed'; segments: Segment[]; error: string | null;
};
export type RecordingStatus = { elapsedSeconds: number; stopped: boolean; paused: boolean; error: string | null };
export type Setup = { modelReady: boolean; modelName: string | null; dataDirectory: string; diarizationReady: boolean };
export const nativeAvailable = isTauri();
export const api = {
  setup: () => invoke<Setup>('setup_status'),
  list: () => invoke<Note[]>('list_notes'),
  startRecording: (title: string) => invoke<void>('start_recording', { title }),
  pauseRecording: (paused: boolean) => invoke<void>('pause_recording', { paused }),
  recordingStatus: () => invoke<RecordingStatus>('recording_status'),
  stopRecording: (discard: boolean) => invoke<{ note: Note; warning: string | null } | null>('stop_recording', { discard }),
  importAudio: () => invoke<Note | null>('import_audio'),
  downloadModels: (speakers: boolean) => invoke<void>('download_models', { speakers }),
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
