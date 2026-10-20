// Development-only fixture; Vite's production entry does not import this module.
// No file access, recording hardware or inference is exercised by this fixture.
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import type { AiAutomationRule, AiAutomationTask, AiWork, AutomationView, Note } from '../src/api';
const notes = ['Research interview', 'Weekly planning'].map<Note>((title, i) => ({
  id: String(i), title, createdAt: 1792400000, durationSeconds: 60, status: 'done',
  segments: [{ startMs: 0, endMs: 10000, speaker: 'Alice', text: 'A saved transcript for library search and editing.' }, { startMs: 10000, endMs: 20000, speaker: 'Alice', text: 'The next paragraph keeps its timestamp.' }],
  error: null, folder: 'Work', tags: ['research'],
}));
const automation: AutomationView = { settings: { paused: false, exportEnabled: false, exportDirectory: null, exportFormats: ['txt'] }, folders: [], files: [], exports: [], label: null };
let aiSettings = {port:11434,model:'qwen2.5:1.5b',contextWindow:8192};
let aiResults: AiWork[] = [];
let aiAutomationRules: AiAutomationRule[] = [];
let aiAutomationTasks: AiAutomationTask[] = [{
  id: 'auto-1', noteId: '0', title: 'Research interview', kind: 'summary', ruleName: 'Meeting summary',
  status: 'failed', error: 'Mocked local model unavailable. Start local Ollama and retry.',
  resultId: null, model: 'qwen2.5:1.5b', source: 'single',
}];
let queueJobs: { id: number; noteId: string; title: string; durationSeconds: number; language: string; status: string; error: string | null }[] = [];
let paused = false;
let recordingTitle = 'Mock recording';
mockWindows('main');
mockIPC((command, payload = {}) => {
  if (Array.isArray(payload) || payload instanceof ArrayBuffer || payload instanceof Uint8Array) return;
  const args = payload;
  switch (command) {
    case 'ai_status': return {settings:{...aiSettings},results:aiResults.filter(result=>result.noteId===args.noteId).map(result=>({...structuredClone(result),stale:false}))};
    case 'ai_automation_status': return {rules:structuredClone(aiAutomationRules),tasks:structuredClone(aiAutomationTasks),model:aiSettings.model,label:aiAutomationTasks.some(task=>task.status==='running')?'Running an automatic AI rule':null};
    case 'ai_automation_configure': aiAutomationRules=structuredClone(args.rules as AiAutomationRule[]); return;
    case 'ai_automation_action': {
      const task=aiAutomationTasks.find(item=>item.id===args.id);
      if(!task)return;
      if(args.action==='remove')aiAutomationTasks=aiAutomationTasks.filter(item=>item.id!==args.id);
      else if(args.action==='skip'){task.status='skipped';task.error=null;}
      else if(args.action==='retry'){task.status='queued';task.error=null;}
      return;
    }
    case 'ai_models': return [{name:'qwen2.5:1.5b',size:986061892,digest:'mock',context:32768}];
    case 'ai_configure': aiSettings={...args.settings as typeof aiSettings}; return;
    case 'ai_get': return structuredClone(aiResults.find(result=>result.id===args.id));
    case 'ai_generate': {
      const request=args.request as {noteId:string;kind:string;target:string;prompt:string;conversation:string;sourceLanguage:string};
      const note=notes.find(note=>note.id===request.noteId)!;
      const structured=['cleanup','translate','speech_english'].includes(request.kind);
      const work:AiWork={...request,id:`ai-${aiResults.length}`,conversation:request.kind==='chat'?(request.conversation||'chat-1'):'',model:aiSettings.model,contextWindow:aiSettings.contextWindow,status:'done',createdAt:1792400000,phase:'Complete',error:null,sourceHash:'fixture',source:structuredClone(note.segments),content:structured?'Cleaned browser transcript.':'Local summary [s1].',segments:structured?structuredClone(note.segments).map(segment=>({...segment,text:'Cleaned browser transcript.'})):[]};
      aiResults.unshift(work);return structuredClone(work);
    }
    case 'ai_recording': {
      const result=aiResults.find(item=>item.id===args.id)!;const source=notes.find(item=>item.id===result.noteId)!;
      const note={...structuredClone(source),id:`copy-${notes.length}`,title:`[${result.target||'Cleaned'}] ${source.title}`,segments:structuredClone(result.segments)};notes.unshift(note);return structuredClone(note);
    }
    case 'ai_retry': {const old=aiResults.find(result=>result.id===args.id)!;const next={...structuredClone(old),id:`ai-${aiResults.length}`};aiResults.unshift(next);return structuredClone(next);}
    case 'ai_segments': return structuredClone(aiResults.find(result=>result.id===args.id)!.segments);
    case 'ai_delete': aiResults=aiResults.filter(result=>result.id!==args.id);return;
    case 'ai_docx': return true;
    case 'automation_status': return structuredClone(automation);
    case 'add_watch_folder': automation.folders.push({ id: 'watch-1', path: '/mock/interviews', enabled: true, autoTranscribe: Boolean(args.autoTranscribe), language: String(args.language), error: null }); return true;
    case 'watch_folder_action': {
      const folder = automation.folders.find(item => item.id === args.id);
      if (args.action === 'remove') automation.folders = automation.folders.filter(item => item.id !== args.id);
      else if (folder && (args.action === 'pause' || args.action === 'resume')) folder.enabled = args.action === 'resume';
      else if (folder) folder.autoTranscribe = args.action === 'auto';
      return;
    }
    case 'pause_automation':
      automation.settings.paused = Boolean(args.paused);
      if (args.paused) aiAutomationTasks = aiAutomationTasks.map(task => task.status === 'running'
        ? { ...task, status: 'cancelled', error: 'Cancelled by pause automation.' }
        : task);
      return;
    case 'choose_export_folder': automation.settings.exportDirectory = '/mock/exports'; return true;
    case 'configure_exports': automation.settings.exportEnabled = Boolean(args.enabled); automation.settings.exportFormats = args.formats as string[]; return;
    case 'export_queue_results': automation.exports = [{ noteId: '0', title: 'Research interview', status: 'failed', error: 'Mocked export permission error. Restore access and retry.' }]; return 1;
    case 'retry_exports': automation.exports = automation.exports.map(task => ({ ...task, status: 'pending', error: null })); return;
    case 'list_jobs': return queueJobs;
    case 'import_batch': {
      const created = ['Mock interview A', 'Mock interview B'].map((title, index) => ({ ...notes[0], id: `batch-${index}`, title, status: 'ready' as const, segments: [] }));
      notes.push(...created);
      queueJobs.push(...created.map((note, index) => ({ id: index + 1, noteId: note.id, title: note.title, durationSeconds: note.durationSeconds, language: String(args.language), status: 'queued', error: null })));
      return { notes: created, failures: ['broken.mp3: Mocked decode failure'], cancelled: false };
    }
    case 'job_action': {
      if (args.action === 'remove') queueJobs = queueJobs.filter(job => job.id !== args.id);
      else queueJobs = queueJobs.map(job => job.id === args.id ? { ...job, status: 'queued', error: null } : job);
      return;
    }
    case 'run_queue': for (const job of queueJobs) job.status = 'done'; return;
    case 'cancel_transcription': return;
    case 'transcription_languages': return [{ code: 'en', name: 'english' }, { code: 'uk', name: 'ukrainian' }, { code: 'ja', name: 'japanese' }];
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
      const note = { ...notes[0], id: String(notes.length), title: recordingTitle, durationSeconds: 7, status: 'ready' as const, segments: [] };
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
