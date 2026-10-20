// Development-only fixture; Vite's production entry does not import this module.
// No file access, recording hardware or inference is exercised by this fixture.
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
const notes = ['Research interview', 'Weekly planning'].map((title, i) => ({
  id: String(i), title, createdAt: 1792400000, durationSeconds: 60, status: 'done',
  segments: [{ startMs: 0, endMs: 10000, speaker: 'Alice', text: 'A saved transcript for library search and editing.' }, { startMs: 10000, endMs: 20000, speaker: 'Alice', text: 'The next paragraph keeps its timestamp.' }],
  error: null, folder: 'Work', tags: ['research'],
}));
let paused = false;
let recordingTitle = 'Mock recording';
mockWindows('main');
mockIPC((command, payload = {}) => {
  if (Array.isArray(payload) || payload instanceof ArrayBuffer || payload instanceof Uint8Array) return;
  const args = payload;
  switch (command) {
    case 'setup_status': return { modelReady: false, modelName: null, dataDirectory: '/mock/local', diarizationReady: false };
    case 'capture_info': return args.applications ? [{ pid: 123, name: 'Browser (mock)' }] : { system: true, mixed: true };
    case 'list_notes': return notes.filter(n => n.title.toLowerCase().includes(String(args.search ?? '').toLowerCase()));
    case 'get_note': return notes.find(n => n.id === args.id);
    case 'audio_source': throw new Error('Mocked audio: native playback is not tested here.');
    case 'start_recording': recordingTitle = String(args.title); paused = false; return;
    case 'pause_recording': paused = Boolean(args.paused); return;
    case 'recording_status': return { elapsedSeconds: 7, stopped: false, paused, error: null };
    case 'stop_recording': {
      if (args.discard) return null;
      const note = { ...notes[0], id: String(notes.length), title: recordingTitle, durationSeconds: 7, status: 'ready', segments: [] };
      notes.unshift(note);
      return { note, warning: null };
    }
    case 'download_models': throw new Error('Mocked network failure. Retry or import a model.');
    case 'organize_note': Object.assign(notes.find(n => n.id === args.id)!, { title: args.title, folder: args.folder, tags: args.tags }); return;
    case 'save_note': Object.assign(notes.find(n => n.id === args.id)!, { segments: args.segments }); return;
    case 'export_transcript': case 'export_saved': return true;
    default: return;
  }
}, { shouldMockEvents: true });
Object.defineProperty(window, 'isTauri', { value: true });
await import('../src/main');
