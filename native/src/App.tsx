import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { convertFileSrc } from '@tauri-apps/api/core';
import { exportWithSpeakers, displaySpeaker, speakerEntryIndexes, renameSpeakerEntries } from './speakerLabels';
import type { Segment } from './transcript/types';
import AudioPlayer from './AudioPlayer';
import TranscriptText from './TranscriptText';
import RecordingMenu, { type RecordingMenuTarget } from './RecordingMenu';
import DeleteRecordingDialog from './DeleteRecordingDialog';
import { activeSegmentIndex, formatClock, type SeekRequest } from './playback';
import { api, nativeAvailable, type Note, type Setup } from './api';
import Settings, { type SettingsSection } from './Settings';
import { applyTheme, readTheme, saveTheme, type Theme } from './theme';
import { findTranscriptMatches, type TranscriptMatch } from './review';
import { readPreferences, savePreferences } from './preferences';

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

export default function App() {
  const [notes, setNotes] = useState<Note[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setDraft] = useState<Segment[]>([]);
  const speakerEntries = useRef<number[]>([]);
  const [setup, setSetup] = useState<Setup | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [loading, setLoading] = useState(nativeAvailable);
  const [dirty, setDirty] = useState(false);
  const [preferences, setPreferences] = useState(readPreferences);
  const [progress, setProgress] = useState(0);
  const [cancelRequested, setCancelRequested] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<Note | null>(null);
  const [recordingMenu, setRecordingMenu] = useState<RecordingMenuTarget | null>(null);
  const { language, autoSpeakers, speakerCount } = preferences;
  const [editSpeakers, setEditSpeakers] = useState(false);
  const closeMenu = useCallback(() => setRecordingMenu(null), []);
  const [audioUrl, setAudioUrl] = useState<string | null>(null);
  const [audioLoading, setAudioLoading] = useState(nativeAvailable);
  const [audioError, setAudioError] = useState<string | null>(null);
  const [playheadMs, setPlayheadMs] = useState(0);
  const [seekRequest, setSeekRequest] = useState<SeekRequest | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsSection, setSettingsSection] = useState<SettingsSection>('appearance');
  const [theme, setTheme] = useState<Theme>(readTheme);
  const [query, setQuery] = useState('');
  const [matchIndex, setMatchIndex] = useState(-1);
  const [selection, setSelection] = useState<(TranscriptMatch & { nonce: number }) | null>(null);
  const [audioAttempt, setAudioAttempt] = useState(0);
  const matches = useMemo(() => findTranscriptMatches(draft, query), [draft, query]);
  const matchingSegments = useMemo(() => new Set(matches.map(match => match.segmentIndex)), [matches]);
  const selectedMatch = matchIndex < 0 ? -1 : Math.min(matchIndex, matches.length - 1);
  const active = notes.find(note => note.id === selected);
  const locked = Boolean(busy) || loading || !nativeAvailable;
  const activeIndex = activeSegmentIndex(draft, playheadMs);
  const hasSpeakerLabels = draft.some(segment => segment.speaker !== null);

  useEffect(() => {
    applyTheme(theme);
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const update = () => applyTheme(theme);
    if (theme === 'system') media.addEventListener('change', update);
    return () => media.removeEventListener('change', update);
  }, [theme]);

  useEffect(() => { savePreferences(preferences); }, [preferences]);

  function updatePreferences(changes: Partial<typeof preferences>) {
    setPreferences(current => ({ ...current, ...changes }));
  }

  useEffect(() => {
    if (!nativeAvailable) return;
    let disposed = false;
    Promise.all([api.setup(), api.list()]).then(([status, library]) => {
      if (disposed) return;
      setSetup(status); setNotes(library);
      if (library[0]) { setSelected(library[0].id); setDraft(library[0].segments); }
    }).catch(e => { if (!disposed) setError(errorText(e)); })
      .finally(() => { if (!disposed) setLoading(false); });
    const subscription = listen<{ noteId: string; percent: number }>('transcription-progress', event => {
      if (!disposed) setProgress(Math.max(0, Math.min(100, event.payload.percent)));
    });
    subscription.catch(e => { if (!disposed) setError(errorText(e)); });
    return () => { disposed = true; void subscription.then(unlisten => unlisten()).catch(() => {}); };
  }, []);

  useEffect(() => {
    if (!dirty && !busy) return;
    const warn = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [dirty, busy]);

  useEffect(() => {
    if (!nativeAvailable) return;
    const subscription = getCurrentWindow().onCloseRequested(event => {
      if (dirty || busy) {
        event.preventDefault();
        setError(dirty ? 'Save or discard your edits before closing Golos.' : 'Finish or cancel the current operation before closing Golos.');
      }
    });
    subscription.catch(e => setError(errorText(e)));
    return () => { void subscription.then(unlisten => unlisten()).catch(() => {}); };
  }, [dirty, busy]);

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

  async function perform(label: string, task: () => Promise<void>) {
    setBusy(label); setError(null); setMessage(null);
    try { await task(); } catch (e) { setError(errorText(e)); } finally { setBusy(null); }
  }
  function openSettings(section: SettingsSection = 'appearance') {
    setSettingsSection(section); setSettingsOpen(true);
  }
  // Reset playback when the selected recording changes or is deleted: the
  // player is keyed per recording, so stopping and clearing the playhead here
  // also drops any highlight.
  function resetPlayback() {
    setAudioUrl(null); setAudioError(null); setAudioLoading(nativeAvailable);
    setPlayheadMs(0); setSeekRequest(null);
  }
  function select(note: Note) {
    if (note.id !== selected) { resetPlayback(); setQuery(''); setMatchIndex(-1); setSelection(null); }
    setEditSpeakers(false); setSelected(note.id); setDraft(note.segments); setDirty(false); setConfirmDelete(null); setMessage(null); setError(null);
  }
  function seekTo(ms: number) {
    setSeekRequest(current => ({ ms, nonce: (current?.nonce ?? 0) + 1, play: true }));
  }
  async function importAudio() {
    await perform('Importing recording', async () => {
      const note = await api.importAudio();
      if (note) { setNotes(await api.list()); select(note); }
    });
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
    setDraft(segments); setDirty(true);
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
        if (updated) setDraft(updated.segments);
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
    const target = confirmDelete;
    await perform('Deleting recording', async () => {
      await api.delete(target.id);
      const library = await api.list(); setNotes(library);
      if (selected === target.id) {
        const index = notes.findIndex(note => note.id === target.id);
        const next = library[Math.min(index, library.length - 1)];
        if (next) select(next); else { setSelected(null); setDraft([]); resetPlayback(); }
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
      setDirty(false); setMessage('Transcript saved on this device.');
    });
  }
  async function exportTranscript(format: 'txt' | 'srt') {
    await perform('Exporting transcript', async () => {
      const content = exportWithSpeakers(draft, format);
      if (await api.export(format, content)) setMessage('Transcript exported.');
    });
  }

  return <div className="app">
    <aside className="sidebar">
      <a className="brand" href="#">golos<span className="brand-dot">●</span></a>
      <button className="primary import" disabled={locked || dirty} onClick={importAudio}>＋ Import recording</button>
      <div className="library-heading"><h2>Library</h2><span>{notes.length}</span></div>
      <nav aria-label="Recordings" className="recordings">
        {loading && <p className="muted">Opening your library…</p>}
        {!loading && !notes.length && <p className="muted empty-library">Your recordings will appear here.</p>}
        {notes.map(note => <div key={note.id} className={`recording-row ${selected === note.id ? 'selected' : ''}`}>
          <button className="recording" disabled={locked || dirty} onClick={() => select(note)}
            onContextMenu={event => { event.preventDefault(); if (!locked && !dirty) setRecordingMenu({ note, x: event.clientX, y: event.clientY, origin: event.currentTarget }); }}
            onKeyDown={event => { if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) { event.preventDefault(); const bounds = event.currentTarget.getBoundingClientRect(); setRecordingMenu({ note, x: bounds.left + 12, y: bounds.bottom, origin: event.currentTarget }); } }}>
            <span className="recording-title">{note.title}</span>
            <span className="recording-meta">{formatClock(note.durationSeconds * 1000)} · {note.status === 'done' ? 'Transcribed' : note.status === 'failed' ? 'Try again' : note.status === 'processing' ? 'Processing' : 'Ready'}</span>
          </button>
          <button className="recording-more" aria-label={`Actions for ${note.title}`} aria-haspopup="menu" aria-expanded={recordingMenu?.note.id === note.id && recordingMenu.origin.classList.contains('recording-more')} disabled={locked || dirty} onClick={event => { if (recordingMenu?.origin === event.currentTarget) { closeMenu(); return; } const bounds = event.currentTarget.getBoundingClientRect(); setRecordingMenu({ note, x: bounds.right - 216, y: bounds.bottom, origin: event.currentTarget }); }}>⋯</button>
        </div>)}
      </nav>
      <button className="settings-trigger" aria-haspopup="dialog" onClick={() => openSettings()}><span aria-hidden="true">⚙</span> Settings</button>
      <div className="sidebar-footer"><span className="status-dot"/> Local processing <span className="version">Preview 0.1</span></div>
    </aside>

    <main>
      <div className="workspace">
        {!nativeAvailable && <div className="notice">Browser preview. Open the Tauri app to import files, transcribe, and save to your local library.</div>}
        {error && !settingsOpen && <div className="notice error" role="alert">{error}</div>}
        {message && !settingsOpen && <div className="notice success" role="status">{message}</div>}
        {busy && !active && <div className="processing" role="status">{busy}…</div>}

        {!active ? <section className="welcome">
          <div className="wave" aria-hidden="true">{[18, 36, 60, 42, 80, 54, 30, 60, 38, 18].map((height, i) => <span key={i} style={{ height }}/>)}</div>
          <h1>Start with a recording</h1>
          <p>Transcribe and edit on your device.</p>
          <button className="primary" disabled={locked} onClick={importAudio}>Import your first recording</button>
          <p className="format-note">Up to 30 minutes · 512 MiB</p>
        </section> : <section className="document">
          <div className="document-heading"><div><h1>{active.title}</h1>
            <p className="muted">{new Date(active.createdAt * 1000).toLocaleDateString()} · {formatClock(active.durationSeconds * 1000)}</p></div>
            <button className="secondary document-menu-trigger" disabled={locked} aria-haspopup="menu" aria-expanded={recordingMenu?.workspace === true} onClick={event => { if (recordingMenu?.origin === event.currentTarget) { closeMenu(); return; } const bounds = event.currentTarget.getBoundingClientRect(); setRecordingMenu({ note: active, x: bounds.right - 216, y: bounds.bottom + 4, origin: event.currentTarget, workspace: true }); }}>Actions <span aria-hidden="true">▾</span></button></div>

          {nativeAvailable && confirmDelete?.id !== active.id && <AudioPlayer
            key={`${active.id}:${audioAttempt}`}
            onRetry={() => { resetPlayback(); setAudioAttempt(value => value + 1); }}
            src={audioUrl}
            title={active.title}
            durationMs={active.durationSeconds * 1000}
            currentMs={playheadMs}
            seekRequest={seekRequest}
            loading={audioLoading}
            error={audioError}
            onTimeUpdate={setPlayheadMs}
            onSeek={setPlayheadMs}
          />}

          {active.status !== 'done' && <div className="transcribe-card">
            <div><h2>{active.status === 'failed' ? 'Ready for another try' : 'Ready to transcribe'}</h2>
              {active.error && <p className="error-text">{active.error}</p>}</div>
            {!loading && !setup?.modelReady && <p>Choose a model in <button className="inline-button" onClick={() => openSettings('transcription')}>Settings</button> to get started.</p>}
            <div className="actions"><button className="primary" disabled={locked || !setup?.modelReady} onClick={transcribe}>Transcribe</button><button className="text-button" onClick={() => openSettings('transcription')}>Options</button></div>
          </div>}

          {busy && <div className="processing" role="status"><span>{busy}{busy === 'Transcribing on your device' ? ` · ${progress}%` : '…'}</span>
            {busy === 'Detecting speakers' && <><span>No percentage estimate. Cancellation takes effect after the current processing pass.</span><button className="secondary" disabled={cancelRequested} onClick={() => { setCancelRequested(true); void api.cancel().catch(e => { setCancelRequested(false); setError(errorText(e)); }); }}>{cancelRequested ? 'Cancellation requested…' : 'Cancel after current pass'}</button></>}
            {busy === 'Transcribing on your device' && <><progress value={progress} max={100}/><button className="secondary" onClick={() => { void api.cancel().catch(e => setError(errorText(e))); }}>Cancel</button></>}</div>}

          {active.status === 'done' && <>
            <div className="editor-toolbar"><span>{draft.length ? 'Transcript' : 'No speech detected in this recording.'}</span><div className="actions">
              {dirty && <><button className="text-button" disabled={locked} onClick={() => { setDraft(active.segments); setDirty(false); }}>Discard edits</button><button className="primary" disabled={locked} onClick={save}>Save edits</button></>}
            </div></div>
            {dirty && <p className="unsaved" role="status">Unsaved edits — save or discard before opening another recording.</p>}
            {!!draft.length && <div className="transcript-search" role="search" aria-label="Search transcript">
              <label className="sr-only" htmlFor="transcript-search">Find in transcript</label>
              <input id="transcript-search" type="search" placeholder="Find in transcript…" value={query} onChange={event => {
                setQuery(event.target.value); setMatchIndex(-1); setSelection(null);
              }} onKeyDown={event => {
                if (event.key === 'Enter') { event.preventDefault(); navigateMatch(event.shiftKey ? -1 : 1); }
                if (event.key === 'Escape') { setQuery(''); setMatchIndex(-1); setSelection(null); }
              }}/>
              <span className="search-count" role="status">{query.trim() ? matches.length
                ? selectedMatch < 0 ? `${matches.length} ${matches.length === 1 ? 'match' : 'matches'}` : `${selectedMatch + 1} of ${matches.length}`
                : 'No matches' : ''}</span>
              <button className="secondary" disabled={!matches.length} aria-label="Previous match" onClick={() => navigateMatch(-1)}>↑</button>
              <button className="secondary" disabled={!matches.length} aria-label="Next match" onClick={() => navigateMatch(1)}>↓</button>
            </div>}
            <div className="transcript">{draft.map((segment, index) => {
              const isActive = index === activeIndex;
              return <div className={`segment ${isActive ? 'active' : ''}${matchingSegments.has(index) ? ' search-match' : ''}${matches[selectedMatch]?.segmentIndex === index ? ' selected-match' : ''}`} key={index} aria-current={isActive ? 'true' : undefined}>
                <button type="button" className="timecode" disabled={!audioUrl}
                  aria-label={`Seek to ${formatClock(segment.startMs)}`}
                  onClick={() => seekTo(segment.startMs)}>{formatClock(segment.startMs)}</button>
                <div className={`segment-content${hasSpeakerLabels || editSpeakers ? ' with-speaker' : ''}`}>{(hasSpeakerLabels || editSpeakers) && <label className="speaker-label"><span className="sr-only">Speaker</span>
                  <input type="text" aria-label={`Speaker at ${formatClock(segment.startMs)}`} maxLength={100} placeholder={hasSpeakerLabels ? 'Unassigned' : 'Not detected'} value={displaySpeaker(segment.speaker)} disabled={locked}
                    title={segment.speaker ? 'Rename this speaker throughout the transcript' : 'Assign a speaker to this entry'}
                    onFocus={() => { setEditSpeakers(true); speakerEntries.current = speakerEntryIndexes(draft, index); }}
                    onChange={e => { const label = e.target.value; const entries = speakerEntries.current; setDraft(current => renameSpeakerEntries(current, entries, label)); setDirty(true); }}/>
                </label>}<TranscriptText selection={selection?.segmentIndex === index ? selection : null} label={`Transcript at ${formatClock(segment.startMs)}`} text={segment.text} disabled={locked}
                  onChange={text => { setDraft(current => current.map((item, n) => n === index ? { ...item, text } : item)); setDirty(true); }}/></div>
              </div>;
            })}</div>
          </>}
        </section>}
      </div>
    </main>
    {settingsOpen && <Settings initialSection={settingsSection} theme={theme} onThemeChange={value => { saveTheme(value); setTheme(value); }}
      setup={setup} modelDisabled={locked || dirty} loadingModel={busy === 'Loading model'} dirty={dirty}
      nativeAvailable={nativeAvailable} error={error} message={message} onImportModel={importModel}
      language={language} onLanguageChange={value => updatePreferences({ language: value as typeof language })}
      autoSpeakers={autoSpeakers} onAutoSpeakersChange={value => updatePreferences({ autoSpeakers: value })}
      speakerCount={speakerCount} onSpeakerCountChange={value => updatePreferences({ speakerCount: value === 'auto' ? value : Number(value) })}
      preferencesDisabled={Boolean(busy) || loading}
      onImportSpeakerModels={importSpeakerModels} loadingSpeakers={busy === 'Importing speaker models'}
      onClose={() => setSettingsOpen(false)}/>}
    {recordingMenu && <RecordingMenu target={recordingMenu} onClose={closeMenu}
      canExport={active?.status === 'done' && !!draft.length && !locked && !dirty} canDetect={active?.status === 'done' && !!draft.length && !locked && !dirty} speakerReady={Boolean(setup?.diarizationReady)}
      canEditSpeakers={active?.status === 'done' && !!draft.length && !locked} canDelete={!locked && !dirty}
      onExport={exportTranscript} onDetect={() => { if (setup?.diarizationReady) void detectSpeakers(); else openSettings('speakers'); }}
      onSpeakerSettings={() => openSettings('speakers')} onEditSpeakers={() => { setEditSpeakers(true); requestAnimationFrame(() => document.querySelector<HTMLInputElement>('.speaker-label input')?.focus()); }}
      onDelete={note => { closeMenu(); if (!locked && !dirty) { setError(null); setConfirmDelete(note); } }}/>}
    {confirmDelete && <DeleteRecordingDialog title={confirmDelete.title} busy={Boolean(busy)} error={error} onClose={() => setConfirmDelete(null)} onDelete={deleteRecording}/>}
  </div>;
}
