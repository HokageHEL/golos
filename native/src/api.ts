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
export type AutomationView = {
  settings: { paused: boolean; exportEnabled: boolean; exportDirectory: string | null; exportFormats: string[] };
  folders: { id: string; path: string; enabled: boolean; autoTranscribe: boolean; language: string; error: string | null }[];
  files: { folderId: string; name: string; status: string; error: string | null }[];
  exports: { noteId: string; title: string; status: string; error: string | null }[];
  label: string | null;
};
export const nativeAvailable = isTauri();
export const api = {
  aiStatus: (noteId: string, offset = 0) => invoke<{ settings: AiSettings; results: AiSummary[] }>('ai_status', { noteId, offset }),
  // Reads the shared local model settings without opening a recording. This is
  // needed when the library is empty but automatic rules still need setup.
  aiConfiguration: () => invoke<{ settings: AiSettings; results: AiSummary[] }>('ai_status', { noteId: null, offset: 0 }),
  aiConfigure: (settings: AiSettings) => invoke<void>('ai_configure', { settings }),
  aiModels: (port: number) => invoke<{ name: string; size: number; digest: string; context: number }[]>('ai_models', { port }),
  aiGet: (id: string) => invoke<AiWork>('ai_get', { id }),
  aiGenerate: (request: AiRequest) => invoke<AiWork>('ai_generate', { request }),
  aiRetry: (id: string) => invoke<AiWork>('ai_retry', { id }),
  aiDelete: (id: string) => invoke<void>('ai_delete', { id }),
  aiSegments: (id: string) => invoke<Segment[]>('ai_segments', { id }),
  aiRecording: (id: string) => invoke<Note>('ai_recording', { id }),
  aiDocx: (id: string) => invoke<boolean>('ai_docx', { id }),
  aiAutomation: () => invoke<AiAutomationView>('ai_automation_status'),
  aiAutomationConfigure: (rules: AiAutomationRule[]) => invoke<void>('ai_automation_configure', { rules }),
  aiAutomationAction: (id: string, action: 'retry' | 'skip' | 'remove') => invoke<void>('ai_automation_action', { id, action }),
  automation: () => invoke<AutomationView>('automation_status'),
  addWatch: (language: string, autoTranscribe: boolean, includeExisting: boolean) => invoke<boolean>('add_watch_folder', { language, autoTranscribe, includeExisting }),
  watchAction: (id: string, action: 'pause' | 'resume' | 'auto' | 'manual' | 'remove' | 'retry') => invoke<void>('watch_folder_action', { id, action }),
  pauseAutomation: (paused: boolean) => invoke<void>('pause_automation', { paused }),
  chooseExportFolder: () => invoke<boolean>('choose_export_folder'),
  configureExports: (enabled: boolean, formats: string[]) => invoke<void>('configure_exports', { enabled, formats }),
  exportQueue: () => invoke<number>('export_queue_results'),
  retryExports: () => invoke<void>('retry_exports'),
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

export type AiSettings = { port: number; model: string; contextWindow: number };
export type AiAutomationSource = 'single' | 'batch' | 'watch';
export type AiAutomationKind = 'cleanup' | 'summary' | 'actions' | 'custom';
export type AiAutomationRule = {
  id: string; name: string; enabled: boolean; kind: AiAutomationKind;
  target: string; prompt: string; sources: AiAutomationSource[];
};
export type AiAutomationTaskStatus = 'queued' | 'running' | 'done' | 'failed' | 'cancelled' | 'skipped';
export type AiAutomationTask = {
  id: string; noteId: string; title: string; kind: string; ruleName: string;
  status: AiAutomationTaskStatus; error: string | null; resultId: string | null;
  model: string; source: string;
};
export type AiAutomationView = { rules: AiAutomationRule[]; tasks: AiAutomationTask[]; model: string; label: string | null };
export type AiSummary = { prompt: string; id: string; kind: string; target: string; model: string; status: string; createdAt: number; phase: string; error: string | null; stale: boolean; conversation: string };
export type AiWork = Omit<AiSummary, 'stale'> & { noteId: string; prompt: string; sourceLanguage: string; contextWindow: number; source: Segment[]; sourceHash: string; content: string; segments: Segment[] };
export type AiRequest = { noteId: string; kind: string; target: string; prompt: string; conversation: string; sourceLanguage: string };
