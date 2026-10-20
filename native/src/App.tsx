import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { convertFileSrc } from '@tauri-apps/api/core';
import { displaySpeaker, speakerEntryIndexes, renameSpeakerEntries } from './speakerLabels';
import type { Segment } from './transcript/types';
import AudioPlayer from './AudioPlayer';
import TranscriptText from './TranscriptText';
import RecordingMenu, { type RecordingMenuTarget } from './RecordingMenu';
import DeleteRecordingDialog from './DeleteRecordingDialog';
import Disclosure from './Disclosure';
import { activeSegmentIndex, formatClock, type SeekRequest } from './playback';
import { api, nativeAvailable, type Note, type Setup, type RecordingStatus, type Language, type TextExportFormat } from './api';
import Settings from './Settings';
import Organization from './Organization';
import BatchQueue from './BatchQueue';
import AutomationSettings from './AutomationSettings';
import LocalAi from './LocalAi';
import { change, undo, redo, replaceAll, splitParagraph, mergeParagraph, type History } from './editing';
import { exportDocument, type DocumentExportOptions } from './export/document';
import { DEFAULT_EXPORT_OPTIONS, validateSubtitleOptions, validateTranscriptLayout, type ExportOptionsState } from './export/options';
import ExportSettings from './ExportSettings';
import { applyTheme, readTheme, saveTheme, type Theme } from './theme';
import { findTranscriptMatches, type TranscriptMatch } from './review';

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

export default function App() {
  const [source, setSource] = useState('microphone');
  const [application, setApplication] = useState(0);
  const [applications, setApplications] = useState<{ pid: number; name: string }[]>([]);
  const [captureSupport, setCaptureSupport] = useState({ system: false, mixed: false });
  const [capture, setCapture] = useState<RecordingStatus | null>(null);
  const [capturePhase, setCapturePhase] = useState<'starting' | 'recording' | 'saving' | null>(null);
  const [discardCapture, setDiscardCapture] = useState(false);
  const [librarySearch, setLibrarySearch] = useState('');
  const [statusFilter, setStatusFilter] = useState('');
  const [folderFilter, setFolderFilter] = useState('');
  const [tagFilter, setTagFilter] = useState('');
  const [activeNote, setActiveNote] = useState<Note | null>(null);
  const [draftCache, setDraftCache] = useState<Record<string, Segment[]>>({});
  const [notes, setNotes] = useState<Note[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setDraft] = useState<Segment[]>([]);
  const [history, setHistory] = useState<History>({ past: [], present: [], future: [] });
  const [replacement, setReplacement] = useState('');
  const [historyRevision, setHistoryRevision] = useState(0);
  function editDraft(next: Segment[]) {
    setHistory(change({ ...history, present: draft }, next));
    setDraft(next); setDirty(true); setHistoryRevision(v => v + 1);
  }
  function travelHistory(direction: 'undo' | 'redo') {
    const next = (direction === 'undo' ? undo : redo)({ ...history, present: draft });
    setHistory(next); setDraft(next.present); setDirty(true); setHistoryRevision(v => v + 1);
  }
  const speakerEntries = useRef<number[]>([]);
  const [setup, setSetup] = useState<Setup | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [loading, setLoading] = useState(nativeAvailable);
  const [dirty, setDirty] = useState(false);
  const [language, setLanguage] = useState('auto');
  const [languages, setLanguages] = useState<Language[]>([]);
  const [extraFormat, setExtraFormat] = useState<TextExportFormat>('md');
  const [exportOptions, setExportOptions] = useState<ExportOptionsState>(DEFAULT_EXPORT_OPTIONS);
  const [downloadProgress, setDownloadProgress] = useState<{ received: number; total: number; phase: string } | null>(null);
  const [progress, setProgress] = useState(0);
  const [speakerCount, setSpeakerCount] = useState('auto');
  const [cancelRequested, setCancelRequested] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<Note | null>(null);
  const [recordingMenu, setRecordingMenu] = useState<RecordingMenuTarget | null>(null);
  const menuTrigger = useRef<HTMLElement | null>(null);
  const [autoSpeakers, setAutoSpeakers] = useState(true);
  const closeMenu = useCallback(() => { setRecordingMenu(null); menuTrigger.current?.focus(); }, []);
  const [audioUrl, setAudioUrl] = useState<string | null>(null);
  const [audioLoading, setAudioLoading] = useState(nativeAvailable);
  const [audioError, setAudioError] = useState<string | null>(null);
  const [playheadMs, setPlayheadMs] = useState(0);
  const [seekRequest, setSeekRequest] = useState<SeekRequest | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [batchOpen, setBatchOpen] = useState(false);
  const [automationOpen, setAutomationOpen] = useState(false);
  const [aiOpen, setAiOpen] = useState(false);
  const [automationLabel, setAutomationLabel] = useState<string | null>(null);
  const [theme, setTheme] = useState<Theme>(readTheme);
  const [query, setQuery] = useState('');
  const [matchIndex, setMatchIndex] = useState(-1);
  const [selection, setSelection] = useState<(TranscriptMatch & { nonce: number }) | null>(null);
  const [audioAttempt, setAudioAttempt] = useState(0);
  const matches = useMemo(() => findTranscriptMatches(draft, query), [draft, query]);
  const matchingSegments = useMemo(() => new Set(matches.map(match => match.segmentIndex)), [matches]);
  const selectedMatch = matchIndex < 0 ? -1 : Math.min(matchIndex, matches.length - 1);
  const active = activeNote;
  const hasUnsaved = dirty || Object.keys(draftCache).length > 0;
  const locked = Boolean(busy || automationLabel) || loading || !nativeAvailable;
  const readLocked = Boolean(busy) || loading || !nativeAvailable;
  const activeIndex = activeSegmentIndex(draft, playheadMs);
  const hasSpeakerLabels = draft.some(segment => segment.speaker !== null);
  // Transcript exports need a finished transcript and no pending edits; audio
  // and project backups stay available for every recording.
  const transcriptExportDisabled = locked || dirty || !draft.length || active?.status !== 'done';

  useEffect(() => {
    if (!nativeAvailable) return;
    let stale = false;
    const timer = setTimeout(() => {
      api.list({ search: librarySearch, status: statusFilter, folder: folderFilter, tag: tagFilter })
        .then(rows => { if (!stale) setNotes(rows); }).catch(e => { if (!stale) setError(errorText(e)); });
    }, 200);
    return () => { stale = true; clearTimeout(timer); };
  }, [librarySearch, statusFilter, folderFilter, tagFilter]);

  useEffect(() => {
    applyTheme(theme);
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const update = () => applyTheme(theme);
    if (theme === 'system') media.addEventListener('change', update);
    return () => media.removeEventListener('change', update);
  }, [theme]);

  useEffect(() => {
    if (!nativeAvailable) return;
    let disposed = false;
    void api.languages().then(rows => { if (!disposed) setLanguages(rows); }).catch(e => { if (!disposed) setError(errorText(e)); });
    void api.captureInfo().then(info => { if (!disposed && !Array.isArray(info)) setCaptureSupport(info); }).catch(e => { if (!disposed) setError(errorText(e)); });
    Promise.all([api.setup(), api.list()]).then(async ([status, library]) => {
      if (disposed) return;
      setSetup(status); setNotes(library);
      if (library[0]) { const note = await api.get(library[0].id); if (!disposed) { setActiveNote(note); setSelected(note.id); setDraft(note.segments); } }
    }).catch(e => { if (!disposed) setError(errorText(e)); })
      .finally(() => { if (!disposed) setLoading(false); });
    const subscription = listen<{ noteId: string; percent: number }>('transcription-progress', event => {
      if (!disposed) setProgress(Math.max(0, Math.min(100, event.payload.percent)));
    });
    subscription.catch(e => { if (!disposed) setError(errorText(e)); });
    return () => { disposed = true; void subscription.then(unlisten => unlisten()).catch(() => {}); };
  }, []);

  useEffect(() => {
    if (!hasUnsaved && !busy && !automationLabel) return;
    const warn = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [hasUnsaved, busy, automationLabel]);

  useEffect(() => {
    if (!nativeAvailable) return;
    const subscription = getCurrentWindow().onCloseRequested(event => {
      if (hasUnsaved || busy || automationLabel) {
        event.preventDefault();
        setError(hasUnsaved ? 'Save or discard your edits before closing Golos.' : 'Finish or cancel the current operation before closing Golos.');
      }
    });
    subscription.catch(e => setError(errorText(e)));
    return () => { void subscription.then(unlisten => unlisten()).catch(() => {}); };
  }, [hasUnsaved, busy, automationLabel]);

  useEffect(() => {
    if (!nativeAvailable || !selected) return;
    // Rust checks that the note exists and authorizes only that note's
    // normalized WAV for the asset protocol; the webview never names a path.
    let disposed = false;
    api.audioSource(selected)
      .then(path => { if (!disposed) setAudioUrl(convertFileSrc(path)); })
      .catch(e => { if (!disposed) setAudioError(errorText(e)); })
      .finally(() => { if (!disposed) setAudioLoading(false); });
    return () => { disposed = true; };
  }, [selected, audioAttempt]);

  useEffect(() => {
    if (capturePhase !== 'recording') return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const status = await api.recordingStatus();
        if (!disposed) setCapture(status);
      } catch (e) { if (!disposed) setError(errorText(e)); }
      if (!disposed) timer = setTimeout(() => { void poll(); }, 250);
    };
    void poll();
    return () => { disposed = true; clearTimeout(timer); };
  }, [capturePhase]);

  async function startRecording() {
    setBusy('Opening recording source'); setError(null); setMessage(null);
    setCapturePhase('starting'); setDiscardCapture(false);
    setCapture({ elapsedSeconds: 0, stopped: false, paused: false, error: null });
    try {
      await api.startRecording(`Recording · ${new Date().toLocaleString()}`, source, application);
      setCapturePhase('recording'); setBusy('Recording');
    } catch (e) {
      setError(errorText(e)); setCapturePhase(null); setCapture(null); setBusy(null);
    }
  }
  async function stopRecording(discard = false) {
    setCapturePhase('saving'); setBusy(discard ? 'Discarding recording' : 'Saving recording'); setError(null);
    try {
      const saved = await api.stopRecording(discard);
      // Clear capture only after native saving succeeds; failures retain the
      // source audio and let the user retry Stop and save.
      setCapturePhase(null); setCapture(null); setDiscardCapture(false); setBusy(null);
      if (saved) {
        setNotes(current => [saved.note, ...current.filter(note => note.id !== saved.note.id)]);
        select(saved.note);
        setMessage(saved.warning ?? 'Recording saved on this device. Ready to transcribe.');
      } else setMessage('Recording discarded.');
    } catch (e) {
      setError(errorText(e)); setCapturePhase('recording'); setBusy('Recording stopped');
    }
  }

  useEffect(() => {
    if (!nativeAvailable) return;
    let disposed = false;
    let revision = 0;
    const state = listen<string | null>('automation-state', event => { revision++; if (!disposed) setAutomationLabel(event.payload); });
    void state.then(async () => { const expected = revision; const view = await api.automation(); if (!disposed && expected === revision) setAutomationLabel(view.label); }).catch(e => { if (!disposed) setError(errorText(e)); });
    const errors = listen<string>('automation-error', event => { if (!disposed) setError(event.payload); });
    return () => { disposed = true; for (const subscription of [state, errors]) void subscription.then(unlisten => unlisten()).catch(() => {}); };
  }, []);
  useEffect(() => {
    if (!nativeAvailable) return;
    let disposed = false;
    const subscription = listen('queue-changed', () => {
      void (async () => {
        try {
          const rows = await api.list({ search: librarySearch, status: statusFilter, folder: folderFilter, tag: tagFilter });
          if (!disposed) setNotes(rows);
          if (active && active.status !== 'done' && !dirty) { const full = await api.get(active.id); if (!disposed) { setActiveNote(full); setDraft(full.segments); } }
        } catch (e) { if (!disposed) setError(errorText(e)); }
      })();
    });
    return () => { disposed = true; void subscription.then(unlisten => unlisten()).catch(() => {}); };
  }, [active, dirty, librarySearch, statusFilter, folderFilter, tagFilter]);

  async function perform(label: string, task: () => Promise<void>) {
    setBusy(label); setError(null); setMessage(null);
    try { await task(); } catch (e) { setError(errorText(e)); } finally { setBusy(null); }
  }
  // Reset playback when the selected recording changes or is deleted: the
  // player is keyed per recording, so stopping and clearing the playhead here
  // also drops any highlight.
  function resetPlayback() {
    setAudioUrl(null); setAudioError(null); setAudioLoading(nativeAvailable);
    setPlayheadMs(0); setSeekRequest(null);
  }
  function select(note: Note) {
    if (note.id === selected) return;
    if (selected && dirty) setDraftCache(current => ({ ...current, [selected]: draft }));
    if (note.id !== selected) { resetPlayback(); setQuery(''); setMatchIndex(-1); setSelection(null); }
    setSelected(note.id); setActiveNote(note);
    setHistory({ past: [], present: draftCache[note.id] ?? note.segments, future: [] }); setHistoryRevision(v => v + 1);
    setDraft(draftCache[note.id] ?? note.segments); setDirty(Boolean(draftCache[note.id]));
    setDraftCache(current => { const next = { ...current }; delete next[note.id]; return next; });
    setConfirmDelete(null); setMessage(null); setError(null);
  }
  async function openNote(note: Note) {
    await perform('Opening recording', async () => { select(await api.get(note.id)); });
  }
  function seekTo(ms: number) {
    setSeekRequest(current => ({ ms, nonce: (current?.nonce ?? 0) + 1, play: true }));
  }
  async function importAudio() {
    await perform('Importing recording', async () => {
      const note = await api.importAudio();
      if (note) { setNotes(await api.list({ search: librarySearch, status: statusFilter, folder: folderFilter, tag: tagFilter })); select(note); }
    });
  }
  async function downloadModels(speakers = false) {
    setDownloadProgress({ received: 0, total: speakers ? 47215727 : 190085487, phase: 'Connecting to trusted source' });
    const unlisten = await listen<{ received: number; total: number; phase: string }>('model-download', event => setDownloadProgress(event.payload));
    try {
      await perform('Downloading models', async () => {
        await api.downloadModels(speakers);
        setSetup(await api.setup());
        setMessage('Models installed. Recording and transcription are ready offline.');
      });
    } finally { unlisten(); setDownloadProgress(null); }
  }
  async function importModel() {
    await perform('Loading model', async () => {
      if (await api.importModel()) { setSetup(await api.setup()); setMessage('Model imported. You’re ready to transcribe offline.'); }
    });
  }
  function navigateMatch(direction: 1 | -1) {
    if (!matches.length) return;
    const index = selectedMatch < 0 ? (direction === 1 ? 0 : matches.length - 1)
      : (selectedMatch + direction + matches.length) % matches.length;
    setMatchIndex(index);
    setSelection(current => ({ ...matches[index], nonce: (current?.nonce ?? 0) + 1 }));
  }
  async function prepareSpeakerDraft(id: string) {
    setBusy('Detecting speakers'); setCancelRequested(false);
    const segments = await api.detectSpeakers(id, speakerCount === 'auto' ? null : Number(speakerCount));
    if (!segments.some(segment => segment.speaker)) {
      throw new Error('No speakers could be assigned. Try a known speaker count or different speaker models. Your saved transcript is unchanged.');
    }
    editDraft(segments);
    const count = new Set(segments.flatMap(segment => segment.speaker ? [segment.speaker] : [])).size;
    setMessage(`${count} ${count === 1 ? 'speaker' : 'speakers'} detected. Review the labels, then save your edits.`);
  }
  async function transcribe() {
    if (!active) return;
    setProgress(0);
    await perform('Transcribing on your device', async () => {
      setNotes(current => current.map(note => note.id === active.id ? { ...note, status: 'processing', error: null } : note));
      try { await api.transcribe(active.id, language); }
      finally {
        const library = await api.list(); setNotes(library);
        const updated = library.find(note => note.id === active.id);
        if (updated) { const full = await api.get(updated.id); setActiveNote(full); setDraft(full.segments); }
      }
      if (autoSpeakers && setup?.diarizationReady) await prepareSpeakerDraft(active.id);
    });
  }
  async function detectSpeakers() {
    if (!active || dirty) return;
    await perform('Detecting speakers', () => prepareSpeakerDraft(active.id));
  }
  async function importSpeakerModels() {
    await perform('Importing speaker models', async () => {
      if (!await api.importSpeakerModels()) return;
      setSetup(await api.setup());
      if (active?.status === 'done' && active.segments.length) await prepareSpeakerDraft(active.id);
      else setMessage('Speaker models installed. Speakers will be detected after transcription.');
    });
  }
  async function deleteRecording() {
    if (!confirmDelete || locked || dirty) return;
    if (draftCache[confirmDelete.id]) { setError('Open this recording and save or discard its draft before deleting it.'); return; }
    const target = confirmDelete;
    await perform('Deleting recording', async () => {
      await api.delete(target.id);
      const library = await api.list(); setNotes(library);
      if (selected === target.id) {
        const index = notes.findIndex(note => note.id === target.id);
        const next = library[Math.min(index, library.length - 1)];
        if (next) await openNote(next); else { setActiveNote(null); setSelected(null); setDraft([]); resetPlayback(); }
      }
      setConfirmDelete(null);
      setMessage(`Deleted “${target.title}”.`);
    });
  }
  async function save() {
    if (!active) return;
    await perform('Saving transcript', async () => {
      await api.save(active.id, draft);
      setNotes(current => current.map(note => note.id === active.id ? { ...note, segments: draft } : note));
      setActiveNote({ ...active, segments: draft }); setDirty(false); setMessage('Transcript saved on this device.');
    });
  }
  async function exportSaved(format: 'docx' | 'wav' | 'golos') {
    if (!active) return;
    await perform('Exporting recording', async () => {
      // Capture, audio and project backups never receive layout options.
      const options = format === 'docx' && exportOptions.custom
        ? validateTranscriptLayout(exportOptions)
        : undefined;
      const exported = options
        ? await api.exportSaved(active.id, format, options)
        : await api.exportSaved(active.id, format);
      if (exported) setMessage('Exported.');
    });
  }
  async function importProject() {
    await perform('Importing project', async () => {
      const note = await api.importProject();
      if (note) { setNotes(await api.list({ search: librarySearch, status: statusFilter, folder: folderFilter, tag: tagFilter })); select(note); setMessage('Project imported as a new recording.'); }
    });
  }
  async function exportTranscript(format: TextExportFormat) {
    await perform('Exporting transcript', async () => {
      const options: DocumentExportOptions | undefined = exportOptions.custom && format !== 'csv' && format !== 'json'
        ? format === 'srt' || format === 'vtt'
          ? { subtitles: validateSubtitleOptions(exportOptions) }
          : { layout: validateTranscriptLayout(exportOptions) }
        : undefined;
      const content = exportDocument(active?.title ?? 'Transcript', draft, format, options);
      if (await api.export(format, content)) setMessage('Transcript exported.');
    });
  }

  async function refreshAfterBatch() {
    setNotes(await api.list({ search: librarySearch, status: statusFilter, folder: folderFilter, tag: tagFilter }));
    if (active && active.status !== 'done') {
      const note = await api.get(active.id);
      setActiveNote(note); setDraft(note.segments);
    }
  }

  return <div className="app">
    {aiOpen && active && <LocalAi key={active.id} note={active} dirty={dirty} modelReady={!!setup?.modelReady} languages={languages} busy={busy} background={automationLabel} audioAvailable={!!audioUrl} onOperation={perform} onSeek={seekTo} onClose={()=>setAiOpen(false)} onApply={segments=>{editDraft(segments);setAiOpen(false);setMessage('Generated draft applied — unsaved. Undo is available.');}} onCreated={async note=>{select(note);setNotes(await api.list({search:librarySearch,status:statusFilter,folder:folderFilter,tag:tagFilter}));setAiOpen(false);setMessage(`Created separate recording: ${note.title}`);}}/>}
    {automationOpen && <AutomationSettings languages={languages} busy={busy} background={automationLabel} onOperation={perform} onClose={() => setAutomationOpen(false)}/>}
    {batchOpen && <BatchQueue languages={languages} modelReady={!!setup?.modelReady} busy={busy || automationLabel} backgroundBusy={!!automationLabel} progress={progress} error={error} onOperation={perform} onChanged={refreshAfterBatch} onClose={() => setBatchOpen(false)}/>}
    <aside className="sidebar">
      <a className="brand" href="#">golos<span className="brand-dot">●</span></a>
      <p className="tagline">Your words. Your device.</p>
      <div className="sidebar-actions">
        <button className="primary record-trigger"  disabled={locked || dirty} onClick={startRecording}>● Record audio</button>
        <button className="secondary" disabled={locked || dirty} onClick={importAudio}>＋ Import recording</button>
      </div>
      <label className="sidebar-source">Recording source<select aria-label="Recording source" value={source} disabled={locked} onChange={e => setSource(e.target.value)}>
        <option value="microphone">Microphone</option><option value="system" disabled={!captureSupport.system}>Application / system audio (macOS 13+)</option><option value="mixed" disabled={!captureSupport.mixed}>Microphone + application audio (macOS 15+)</option>
      </select></label>
      {source !== 'microphone' && <Disclosure key={source} className="sidebar-panel" summary="System audio & permissions" open>
        <select aria-label="Application to capture" value={application} disabled={locked} onChange={e => setApplication(Number(e.target.value))}><option value={0}>All system audio (excluding Golos)</option>{applications.map(app => <option key={app.pid} value={app.pid}>{app.name}</option>)}</select>
        <button className="secondary" disabled={locked} onClick={() => { void perform('Loading applications', async () => { const info = await api.captureInfo(true); if (Array.isArray(info)) setApplications(info); }); }}>Refresh applications / grant access</button>
        <p className="muted">Allow Golos Audio Capture in System Settings → Privacy &amp; Security → Screen &amp; System Audio Recording. Mixed mode also needs Microphone access. Restart Golos after changing permission. Use headphones to avoid microphone echo.</p>
      </Disclosure>}
      <Disclosure className="sidebar-panel" summary="More tools">
        <button className="secondary" disabled={locked} onClick={() => setBatchOpen(true)}>Batch transcription</button>
        <button className="secondary" disabled={!!busy || loading || !nativeAvailable} onClick={() => setAutomationOpen(true)}>Folders &amp; automation</button>
        <button className="secondary" disabled={locked} onClick={importProject}>Import project backup</button>
      </Disclosure>
      <div className="library-heading"><h2>Library</h2><span title="Loaded matching recordings">{notes.length}{notes.length >= 100 ? '＋' : ''}</span></div>
      <div className="sidebar-search">
        <input aria-label="Search library" type="search" placeholder="Search titles & saved transcripts" title="Search word prefixes across saved titles and transcripts" value={librarySearch} onChange={e => setLibrarySearch(e.target.value)}/>
      </div>
      <Disclosure className="sidebar-panel" summary="Filters">
        <select aria-label="Recording status" value={statusFilter} onChange={e => setStatusFilter(e.target.value)}><option value="">All statuses</option><option value="ready">Ready to transcribe</option><option value="done">Transcribed</option><option value="failed">Needs retry</option><option value="processing">Processing</option></select>
        <input aria-label="Filter folder" placeholder="Filter by folder" value={folderFilter} onChange={e => setFolderFilter(e.target.value)}/>
        <input aria-label="Filter tag" placeholder="Filter by tag" value={tagFilter} onChange={e => setTagFilter(e.target.value)}/>
      </Disclosure>
      <nav aria-label="Recordings"  className="recordings">
        {loading && <p className="muted">Opening your library…</p>}
        {!loading && !notes.length && <p className="muted empty-library">Your recordings will appear here. Everything stays on this device.</p>}
        {notes.map(note => <div key={note.id} className={`recording-row ${selected === note.id ? 'selected' : ''}`}>
          <button className="recording"
            disabled={readLocked} onClick={() => { void openNote(note); }}
            onContextMenu={event => { event.preventDefault(); if (!locked && !dirty) { menuTrigger.current = event.currentTarget; setRecordingMenu({ note, trigger: event.currentTarget }); } }}
            onKeyDown={event => { if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) { event.preventDefault(); menuTrigger.current = event.currentTarget; if (!locked && !dirty) setRecordingMenu({ note, trigger: event.currentTarget }); } }}>
            <span className="recording-title">{note.title}</span>
            <span className="recording-meta">{formatClock(note.durationSeconds * 1000)} · {note.status === 'done' ? 'Transcribed' : note.status === 'failed' ? 'Try again' : note.status === 'processing' ? 'Processing' : 'Ready'}</span>
          </button>
          <button className="recording-actions" aria-haspopup="menu" aria-expanded={recordingMenu?.note.id === note.id}
            aria-label={`Actions for ${note.title}`} disabled={locked || dirty}
            onClick={event => { menuTrigger.current = event.currentTarget; setRecordingMenu(current => current?.note.id === note.id ? null : { note, trigger: event.currentTarget }); }}>
            <span aria-hidden="true">⋯</span>
          </button>
          {recordingMenu?.note.id === note.id && <RecordingMenu target={recordingMenu} onClose={closeMenu} onDelete={note => { closeMenu(); if (!locked && !dirty) { if (draftCache[note.id]) { setError('Open this recording and save or discard its draft before deleting it.'); return; } setError(null); setConfirmDelete(note); } }}/>}
        </div>)}
        {notes.length >= 100 && <button className="secondary" onClick={async () => { try { const more = await api.list({ search: librarySearch, status: statusFilter, folder: folderFilter, tag: tagFilter, offset: notes.length }); setNotes(current => [...current, ...more]); } catch (e) { setError(errorText(e)); } }}>Load more</button>}
      </nav>
      <button className="settings-trigger" aria-haspopup="dialog" onClick={() => setSettingsOpen(true)}><span aria-hidden="true">⚙</span> Settings</button>
      <div className="sidebar-footer"><span className="status-dot"/> Local processing <span className="version">Preview 0.1</span></div>
    </aside>

    <main>
      <header className="topbar"><span>Private transcription</span><span className="pill">No account needed</span></header>
      <div className="workspace">
        {automationLabel && <div className="notice" role="status"><span>{automationLabel} · Runs on this device.</span><button className="secondary" onClick={() => { void api.pauseAutomation(true).catch(e => setError(errorText(e))); }}>Pause automation and cancel current work</button></div>}
        {!nativeAvailable && <div className="notice">Browser preview. Open the Tauri app to record audio, import files, transcribe, and save to your local library.</div>}
        {error && !settingsOpen && <div className="notice error" role="alert">{error}</div>}
        {message && !settingsOpen && <div className="notice success" role="status">{message}</div>}
        {capture && <section className="capture-card" aria-label="Audio recording">
          <div className="capture-heading"><span className={`capture-dot ${capture.stopped || capture.paused ? 'stopped' : ''}`} aria-hidden="true"/><h2>{capturePhase === 'starting' ? 'Opening recording source…' : capturePhase === 'saving' ? 'Saving recording…' : capture.stopped ? 'Recording stopped' : capture.paused ? 'Recording paused' : source === 'microphone' ? 'Recording from your microphone' : source === 'mixed' ? 'Recording microphone + application audio' : 'Recording application audio'}</h2></div>
          <p className="capture-timer" role="timer" aria-label="Recorded duration">{formatClock(capture.elapsedSeconds * 1000)}</p>
          <p className="muted">{capture.error ?? (capture.stopped ? 'Save the captured audio to your library, then transcribe it.' : 'Audio stays on this device. Recording stops at 30 minutes.')}</p>
          {discardCapture ? <div role="group" aria-label="Discard this recording?">
            <p>Discard this recording? The captured audio will be deleted.</p>
            <div className="actions"><button className="secondary" disabled={capturePhase !== 'recording'} onClick={() => setDiscardCapture(false)}>{capture.stopped || capture.paused ? 'Keep captured audio' : 'Keep recording'}</button><button className="danger-button" disabled={capturePhase !== 'recording'} onClick={() => stopRecording(true)}>Discard recording</button></div>
          </div> : <div className="actions"><button className="secondary" disabled={capturePhase !== 'recording' || capture.stopped} onClick={async () => { try { await api.pauseRecording(!capture.paused); setCapture(await api.recordingStatus()); } catch (e) { setError(errorText(e)); } }}>{capture.paused ? 'Resume' : 'Pause'}</button><button className="primary" disabled={capturePhase !== 'recording'} onClick={() => stopRecording()}>Stop and save</button><button className="secondary" disabled={capturePhase !== 'recording'} onClick={() => setDiscardCapture(true)}>Discard…</button></div>}
        </section>}
        {busy && !active && !capture && <div className="processing" role="status">{busy}…</div>}

        {!capture && (!active ? <section className="welcome">
          <div className="wave" aria-hidden="true">{[18, 36, 60, 42, 80, 54, 30, 60, 38, 18].map((height, i) => <span key={i} style={{ height }}/>)}</div>
          <div className="eyebrow">MAKE ROOM FOR THE CONVERSATION</div>
          <h1>From spoken words<br/>to something you can keep.</h1>
          <p>Record or import audio, turn it into text, and make it your own.<br/>Your audio stays with you.</p>
          <div className="actions welcome-actions"><button className="primary" disabled={locked} onClick={startRecording}>Record your first conversation</button><button className="secondary" disabled={locked} onClick={importAudio}>Import recording</button></div>
          <button className="secondary" disabled={locked} onClick={() => { setSettingsOpen(true); void downloadModels(); }}>Download recommended model</button>
          <p className="format-note">WAV, MP3, M4A/AAC, FLAC, Ogg (Vorbis/Opus), AIFF · Up to 30 minutes / 512 MiB</p>
        </section> : <section className="document">
          <div className="document-heading"><div><div className="eyebrow">RECORDING</div><h1>{active.title}</h1>
            <Organization key={active.id} note={active} disabled={locked} onSave={async (title, folder, tags) => { await perform('Saving organization', async () => { await api.organize(active.id, title, folder, tags); setActiveNote({ ...active, title, folder, tags }); setNotes(await api.list({ search: librarySearch, status: statusFilter, folder: folderFilter, tag: tagFilter })); setMessage('Organization saved.'); }); }}/>
            <p className="muted">{new Date(active.createdAt * 1000).toLocaleDateString()} · {formatClock(active.durationSeconds * 1000)} · Stored locally</p></div>
            <button className="text-button danger" disabled={locked || dirty} onClick={() => { setError(null); setConfirmDelete(active); }}>Delete</button></div>

          <div className="document-actions">
            <button className="secondary" disabled={readLocked} onClick={()=>setAiOpen(true)}>Local AI &amp; translation</button>
            <Disclosure className="workspace-panel export-panel" summary="Export">
              <div className="export-panel-body">
                <div className="export-group">
                  <h3>Transcript</h3>
                  <p className="muted">Transcript files use the saved transcript. Save your edits first.</p>
                  <div className="export-buttons">
                    <button className="secondary" disabled={transcriptExportDisabled} onClick={() => exportTranscript('txt')}>Export TXT</button>
                    <button className="secondary" disabled={transcriptExportDisabled} onClick={() => exportTranscript('srt')}>Export SRT</button>
                    <button className="secondary" disabled={transcriptExportDisabled} onClick={() => exportTranscript('vtt')}>Export VTT</button>
                    <button className="secondary" disabled={transcriptExportDisabled} onClick={() => exportSaved('docx')}>Export DOCX</button>
                  </div>
                  <label>Other formats<select aria-label="Other export format" value={extraFormat} disabled={locked} onChange={event => setExtraFormat(event.target.value as TextExportFormat)}><option value="md">Markdown (.md)</option><option value="html">HTML (print ready)</option><option value="csv">CSV (spreadsheet)</option><option value="json">JSON (structured)</option></select></label>
                  <button className="secondary" disabled={transcriptExportDisabled} onClick={() => exportTranscript(extraFormat)}>Export {extraFormat.toUpperCase()}</button>
                </div>
                <div className="export-group">
                  <h3>Recording</h3>
                  <button className="secondary" disabled={locked} onClick={() => exportSaved('wav')}>Export audio</button>
                  <button className="secondary" disabled={locked || dirty} onClick={() => exportSaved('golos')}>Export project backup</button>
                  <button className="secondary" disabled={locked || !draft.length} onClick={() => { void perform('Copying transcript', async () => { await navigator.clipboard.writeText(exportDocument(active.title, draft, 'txt')); setMessage(dirty ? 'Draft copied to clipboard. Your edits are still unsaved.' : 'Transcript copied to clipboard.'); }); }}>Copy transcript</button>
                  <p className="muted">Audio and project backups keep the recording exactly as it is stored on this device.</p>
                </div>
              </div>
            </Disclosure>
          </div>
          {nativeAvailable && confirmDelete?.id !== active.id && <AudioPlayer
            key={`${active.id}:${audioAttempt}`}
            onRetry={() => { resetPlayback(); setAudioAttempt(value => value + 1); }}
            src={audioUrl}
            title={active.title}
            durationMs={active.durationSeconds * 1000}
            currentMs={playheadMs}
            activeSegment={activeIndex === null ? null : draft[activeIndex]}
            seekRequest={seekRequest}
            loading={audioLoading}
            error={audioError}
            onTimeUpdate={setPlayheadMs}
            onSeek={setPlayheadMs}
          />}

          {active.status !== 'done' && <div className="transcribe-card">
            <div><h2>{active.status === 'failed' ? 'Ready for another try' : 'Ready when you are'}</h2><p>Transcription runs locally. Audio is automatically converted to the format Whisper needs.</p>
              {active.error && <p className="error-text">{active.error}</p>}</div>
            {!loading && !setup?.modelReady && <p>Choose a model in <button className="inline-button" onClick={() => setSettingsOpen(true)}>Settings</button> to get started.</p>}
            <div className="actions"><label>Audio language<select value={language} disabled={locked} onChange={e => setLanguage(e.target.value)}>
              <option value="auto">Detect automatically</option>{languages.map(item => <option key={item.code} value={item.code}>{item.name.charAt(0).toUpperCase() + item.name.slice(1)}</option>)}</select></label>
              {setup?.diarizationReady && <label className="auto-speakers"><input type="checkbox" checked={autoSpeakers} disabled={locked} onChange={e => setAutoSpeakers(e.target.checked)}/> Detect speakers after transcription</label>}
              <button className="primary" disabled={locked || !setup?.modelReady} onClick={transcribe}>Transcribe offline</button>
              <button className="secondary" disabled={locked} onClick={() => { void perform('Queueing recording', async () => { await api.queue(active.id, language); setBatchOpen(true); }); }}>Add to batch queue</button></div>
          </div>}

          {busy && <div className="processing" role="status"><span>{busy}{busy === 'Transcribing on your device' ? ` · ${progress}%` : '…'}</span>
            {busy === 'Detecting speakers' && <><span>No percentage estimate. Cancellation takes effect after the current processing pass.</span><button className="secondary" disabled={cancelRequested} onClick={() => { setCancelRequested(true); void api.cancel().catch(e => { setCancelRequested(false); setError(errorText(e)); }); }}>{cancelRequested ? 'Cancellation requested…' : 'Cancel after current pass'}</button></>}
            {busy === 'Transcribing on your device' && <><progress value={progress} max={100}/><button className="secondary" onClick={() => { void api.cancel().catch(e => setError(errorText(e))); }}>Cancel</button></>}</div>}

          {active.status === 'done' && <>
            <Disclosure className="workspace-panel speaker-panel" summary="Speaker detection">
              <div className="speaker-actions"><label>Speakers<select value={speakerCount} disabled={locked || dirty} onChange={e => setSpeakerCount(e.target.value)}>
                <option value="auto">Detect automatically</option>{[1, 2, 3, 4, 5, 6, 8, 10, 20].map(n => <option key={n} value={n}>{n}</option>)}</select></label>
                <button className="secondary" disabled={locked || dirty || !draft.length || !setup?.diarizationReady} onClick={detectSpeakers}>Detect speakers</button>
                <p>{!setup?.diarizationReady ? 'Install separate speaker models in Settings. A Whisper model transcribes text only.' : !hasSpeakerLabels ? 'Speakers have not been detected for this transcript. Choose Detect speakers.' : 'Labels are per recording. Correct mixed-speaker paragraphs below.'}</p>
              </div>
            </Disclosure>
            <div className="editor-tools" data-history-revision={historyRevision}>
              <button className="secondary" disabled={readLocked || !history.past.length} onClick={() => travelHistory('undo')}>Undo</button>
              <button className="secondary" disabled={readLocked || !history.future.length} onClick={() => travelHistory('redo')}>Redo</button>
            </div>
            {!!draft.length && <Disclosure className="workspace-panel find-panel" summary="Find &amp; replace">
              <div className="transcript-search" role="search" aria-label="Search transcript">
                <label className="sr-only" htmlFor="transcript-search">Find in transcript</label>
                <input id="transcript-search" type="search" placeholder="Find in transcript…" value={query} onChange={event => {
                  setQuery(event.target.value); setMatchIndex(-1); setSelection(null);
                }} onKeyDown={event => {
                  if (event.key === 'Enter') { event.preventDefault(); navigateMatch(event.shiftKey ? -1 : 1); }
                  if (event.key === 'Escape') { setQuery(''); setMatchIndex(-1); setSelection(null); }
                }}/>
                <span className="search-count" role="status">{query.trim() ? matches.length
                  ? selectedMatch < 0 ? `${matches.length} ${matches.length === 1 ? 'match' : 'matches'}` : `${selectedMatch + 1} of ${matches.length}`
                  : 'No matches' : 'Search your edits too'}</span>
                <button className="secondary" disabled={!matches.length} aria-label="Previous match" onClick={() => navigateMatch(-1)}>↑</button>
                <button className="secondary" disabled={!matches.length} aria-label="Next match" onClick={() => navigateMatch(1)}>↓</button>
              </div>
              <div className="editor-tools">
                <input aria-label="Replacement text" placeholder="Replace with…" value={replacement} onChange={e => setReplacement(e.target.value)}/>
                <button className="secondary" disabled={readLocked || !query} onClick={() => editDraft(replaceAll(draft, query, replacement))}>Replace all (case sensitive)</button>
              </div>
            </Disclosure>}
            <div className="editor-toolbar"><span>{draft.length ? 'Transcript · Click any paragraph to edit' : 'No speech detected in this recording.'}</span><div className="actions">
              {dirty && <><button className="text-button" disabled={locked} onClick={() => { setDraft(active.segments); setHistory({ past: [], present: active.segments, future: [] }); setHistoryRevision(v => v + 1); setDirty(false); }}>Discard edits</button><button className="primary" disabled={locked} onClick={save}>Save edits</button></>}
            </div></div>
            <ExportSettings value={exportOptions} disabled={locked} onChange={patch => setExportOptions(current => ({ ...current, ...patch }))}/>
            {dirty && <p className="unsaved" role="status">Unsaved edits — kept while you browse. Save to include them in library search and exports.</p>}
            <div className="transcript">{draft.map((segment, index) => {
              const isActive = index === activeIndex;
              return <div className={`segment ${isActive ? 'active' : ''}${matchingSegments.has(index) ? ' search-match' : ''}${matches[selectedMatch]?.segmentIndex === index ? ' selected-match' : ''}`} key={index} aria-current={isActive ? 'true' : undefined}>
                <button type="button" className="timecode" disabled={!audioUrl}
                  aria-label={`Seek to ${formatClock(segment.startMs)}`}
                  onClick={() => seekTo(segment.startMs)}>{formatClock(segment.startMs)}</button>
                <div className="segment-content"><label className="speaker-label"><span className="sr-only">Speaker</span>
                  <input type="text" aria-label={`Speaker at ${formatClock(segment.startMs)}`} maxLength={100} placeholder={hasSpeakerLabels ? 'Unassigned' : 'Not detected'} value={displaySpeaker(segment.speaker)} disabled={readLocked}
                    title={segment.speaker ? 'Rename this speaker throughout the transcript' : 'Assign a speaker to this entry'}
                    onFocus={() => { speakerEntries.current = speakerEntryIndexes(draft, index); }}
                    onChange={e => { const label = e.target.value; const entries = speakerEntries.current; editDraft(renameSpeakerEntries(draft, entries, label)); }}/>
                </label><TranscriptText onSplit={offset => editDraft(splitParagraph(draft, index, offset))} selection={selection?.segmentIndex === index ? selection : null} label={`Transcript at ${formatClock(segment.startMs)}`} text={segment.text} disabled={readLocked}
                  onChange={text => { editDraft(draft.map((item, n) => n === index ? { ...item, text } : item)); }}/>
                  <div className="editor-tools"><button className="text-button" disabled={readLocked || segment.text.length < 2 || segment.endMs - segment.startMs < 2} onClick={() => editDraft(splitParagraph(draft, index, Math.floor(segment.text.length / 2)))}>Split paragraph in half</button>
                  <button className="text-button" disabled={readLocked || !draft[index + 1] || segment.speaker !== draft[index + 1]?.speaker} title="Merge requires matching speaker labels" onClick={() => editDraft(mergeParagraph(draft, index))}>Merge with next</button></div>
                </div>
              </div>;
            })}</div>
          </>}
        </section>)}
        {Object.keys(draftCache).length > 0 && <p role="status">Unsaved drafts in {Object.keys(draftCache).length} other recording(s). Open them to save or discard before closing.</p>}
        <footer className="workspace-footer"><span>Audio is never uploaded for transcription.</span><span>Playback and speaker detection stay on your device.</span></footer>
      </div>
    </main>
    {settingsOpen && <Settings theme={theme} onThemeChange={value => { saveTheme(value); setTheme(value); }}
      onDownloadModels={downloadModels} downloadProgress={downloadProgress} onCancelDownload={() => { void api.cancel(); }}
      setup={setup} modelDisabled={locked || dirty} loadingModel={busy === 'Loading model'} dirty={dirty}
      nativeAvailable={nativeAvailable} error={error} message={message} onImportModel={importModel}
      onImportSpeakerModels={importSpeakerModels} loadingSpeakers={busy === 'Importing speaker models'}
      onClose={() => setSettingsOpen(false)}/>}
    {confirmDelete && <DeleteRecordingDialog title={confirmDelete.title} busy={Boolean(busy)} error={error} onClose={() => setConfirmDelete(null)} onDelete={deleteRecording}/>}
  </div>;
}
