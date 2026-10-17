import { useCallback, useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { convertFileSrc } from '@tauri-apps/api/core';
import { exportWithSpeakers, displaySpeaker } from './speakerLabels';
import type { Segment } from '../../lib/transcript/types';
import AudioPlayer from './AudioPlayer';
import TranscriptText from './TranscriptText';
import RecordingMenu, { type RecordingMenuTarget } from './RecordingMenu';
import DeleteRecordingDialog from './DeleteRecordingDialog';
import { activeSegmentIndex, formatClock, type SeekRequest } from './playback';
import { api, nativeAvailable, type Note, type Setup } from './api';

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

export default function App() {
  const [notes, setNotes] = useState<Note[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setDraft] = useState<Segment[]>([]);
  const [setup, setSetup] = useState<Setup | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [loading, setLoading] = useState(nativeAvailable);
  const [dirty, setDirty] = useState(false);
  const [language, setLanguage] = useState('auto');
  const [progress, setProgress] = useState(0);
  const [speakerCount, setSpeakerCount] = useState('auto');
  const [cancelRequested, setCancelRequested] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<Note | null>(null);
  const [recordingMenu, setRecordingMenu] = useState<RecordingMenuTarget | null>(null);
  const [autoSpeakers, setAutoSpeakers] = useState(true);
  const closeMenu = useCallback(() => setRecordingMenu(null), []);
  const [audioUrl, setAudioUrl] = useState<string | null>(null);
  const [audioLoading, setAudioLoading] = useState(nativeAvailable);
  const [audioError, setAudioError] = useState<string | null>(null);
  const [playheadMs, setPlayheadMs] = useState(0);
  const [seekRequest, setSeekRequest] = useState<SeekRequest | null>(null);
  const active = notes.find(note => note.id === selected);
  const locked = Boolean(busy) || loading || !nativeAvailable;
  const activeIndex = activeSegmentIndex(draft, playheadMs);
  const hasSpeakerLabels = draft.some(segment => segment.speaker !== null);

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
  }, [selected]);

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
    if (note.id !== selected) resetPlayback();
    setSelected(note.id); setDraft(note.segments); setDirty(false); setConfirmDelete(null); setMessage(null); setError(null);
  }
  function seekTo(ms: number) {
    setSeekRequest(current => ({ ms, nonce: (current?.nonce ?? 0) + 1 }));
  }
  async function importAudio() {
    await perform('Importing recording', async () => {
      const note = await api.importAudio();
      if (note) { setNotes(await api.list()); select(note); }
    });
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
      <p className="tagline">Your words. Your device.</p>
      <button className="primary import" disabled={locked || dirty} onClick={importAudio}>＋ Import recording</button>
      <div className="library-heading"><h2>Library</h2><span>{notes.length}</span></div>
      <nav aria-label="Recordings" className="recordings">
        {loading && <p className="muted">Opening your library…</p>}
        {!loading && !notes.length && <p className="muted empty-library">Your recordings will appear here. Everything stays on this device.</p>}
        {notes.map(note => <button key={note.id} className={`recording ${selected === note.id ? 'selected' : ''}`}
          disabled={locked || dirty} onClick={() => select(note)}
          onContextMenu={event => { event.preventDefault(); if (!locked && !dirty) setRecordingMenu({ note, x: event.clientX, y: event.clientY }); }}
          onKeyDown={event => { if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) { event.preventDefault(); const bounds = event.currentTarget.getBoundingClientRect(); setRecordingMenu({ note, x: bounds.left + 12, y: bounds.bottom }); } }}>
          <span className="recording-title">{note.title}</span>
          <span className="recording-meta">{formatClock(note.durationSeconds * 1000)} · {note.status === 'done' ? 'Transcribed' : note.status === 'failed' ? 'Try again' : note.status === 'processing' ? 'Processing' : 'Ready'}</span>
        </button>)}
      </nav>
      <div className="sidebar-footer"><span className="status-dot"/> Local processing <span className="version">Preview 0.1</span></div>
    </aside>

    <main>
      <header className="topbar"><span>Private transcription</span><span className="pill">No account needed</span></header>
      <div className="workspace">
        {!nativeAvailable && <div className="notice">Browser preview. Open the Tauri app to import files, transcribe, and save to your local library.</div>}
        {error && <div className="notice error" role="alert">{error}</div>}
        {message && <div className="notice success" role="status">{message}</div>}
        {busy && !active && <div className="processing" role="status">{busy}…</div>}

        <details className="model-settings" open={!setup?.modelReady}>
          <summary>Models <span>{setup?.modelName ?? 'Set up transcription'} · {setup?.diarizationReady ? 'Speakers ready' : 'Speaker models not installed'}</span></summary>
        <section className="model-card" aria-label="Transcription model">
          <div><div className="eyebrow">OFFLINE ENGINE</div><h2>{setup?.modelReady ? `${setup.modelName} is ready` : 'Small model. Private transcription.'}</h2>
            <p>{setup?.modelReady ? 'Audio and transcripts are processed and stored on this device.' : 'Recommended: multilingual Whisper Small (ggml-small-q5_1.bin). Import once to transcribe offline. Tiny, Base, Medium, Large and Large V3 Turbo models (including quantized GGML) also work; larger models need more memory.'}</p></div>
          <button className="secondary" disabled={locked || dirty} onClick={() => perform('Loading model', async () => {
            if (await api.importModel()) { setSetup(await api.setup()); setMessage('Model imported. You’re ready to transcribe offline.'); }
          })}>{setup?.modelReady ? 'Replace model' : 'Import model'}</button>
        </section>

        <section className="speaker-card" aria-label="Speaker detection models">
          <div><h2>Offline speaker detection</h2><p>{setup?.diarizationReady ? 'Ready. Detection runs after transcription; existing transcripts can use Detect speakers.' : 'Import a pyannote segmentation model and a speaker embedding model to label recurring voices. Models and audio stay on this device.'}</p></div>
          <button className="secondary" disabled={locked || dirty} onClick={importSpeakerModels}>{setup?.diarizationReady ? 'Replace speaker models' : 'Import speaker models'}</button>
        </section>
        </details>

        {!active ? <section className="welcome">
          <div className="wave" aria-hidden="true">{[18, 36, 60, 42, 80, 54, 30, 60, 38, 18].map((height, i) => <span key={i} style={{ height }}/>)}</div>
          <div className="eyebrow">MAKE ROOM FOR THE CONVERSATION</div>
          <h1>From spoken words<br/>to something you can keep.</h1>
          <p>Import a recording, turn it into text, and make it your own.<br/>Your audio stays with you.</p>
          <button className="primary" disabled={locked} onClick={importAudio}>Import your first recording</button>
          <p className="format-note">WAV, MP3, M4A/AAC, FLAC, Ogg (Vorbis/Opus), AIFF · Up to 30 minutes / 512 MiB</p>
        </section> : <section className="document">
          <div className="document-heading"><div><div className="eyebrow">RECORDING</div><h1>{active.title}</h1>
            <p className="muted">{new Date(active.createdAt * 1000).toLocaleDateString()} · {formatClock(active.durationSeconds * 1000)} · Stored locally</p></div>
            <button className="text-button danger" disabled={locked || dirty} onClick={() => { setError(null); setConfirmDelete(active); }}>Delete</button></div>

          {nativeAvailable && confirmDelete?.id !== active.id && <AudioPlayer
            key={active.id}
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
            <div className="actions"><label>Audio language<select value={language} disabled={locked} onChange={e => setLanguage(e.target.value)}>
              <option value="auto">Detect automatically</option><option value="en">English</option><option value="uk">Ukrainian</option></select></label>
              {setup?.diarizationReady && <label className="auto-speakers"><input type="checkbox" checked={autoSpeakers} disabled={locked} onChange={e => setAutoSpeakers(e.target.checked)}/> Detect speakers after transcription</label>}
              <button className="primary" disabled={locked || !setup?.modelReady} onClick={transcribe}>Transcribe offline</button></div>
          </div>}

          {busy && <div className="processing" role="status"><span>{busy}{busy === 'Transcribing on your device' ? ` · ${progress}%` : '…'}</span>
            {busy === 'Detecting speakers' && <><span>No percentage estimate. Cancellation takes effect after the current processing pass.</span><button className="secondary" disabled={cancelRequested} onClick={() => { setCancelRequested(true); void api.cancel().catch(e => { setCancelRequested(false); setError(errorText(e)); }); }}>{cancelRequested ? 'Cancellation requested…' : 'Cancel after current pass'}</button></>}
            {busy === 'Transcribing on your device' && <><progress value={progress} max={100}/><button className="secondary" onClick={() => { void api.cancel().catch(e => setError(errorText(e))); }}>Cancel</button></>}</div>}

          {active.status === 'done' && <>
            <div className="speaker-actions"><label>Speakers<select value={speakerCount} disabled={locked || dirty} onChange={e => setSpeakerCount(e.target.value)}>
              <option value="auto">Detect automatically</option>{[1, 2, 3, 4, 5, 6, 8, 10, 20].map(n => <option key={n} value={n}>{n}</option>)}</select></label>
              <button className="secondary" disabled={locked || dirty || !draft.length || !setup?.diarizationReady} onClick={detectSpeakers}>Detect speakers</button>
              <p>{!setup?.diarizationReady ? 'Install separate speaker models in Models above. A Whisper model transcribes text only.' : !hasSpeakerLabels ? 'Speakers have not been detected for this transcript. Choose Detect speakers.' : 'Labels are per recording. Correct mixed-speaker paragraphs below.'}</p>
            </div>
            <div className="editor-toolbar"><span>{draft.length ? 'Transcript · Click any paragraph to edit' : 'No speech detected in this recording.'}</span><div className="actions">
              {dirty && <><button className="text-button" disabled={locked} onClick={() => { setDraft(active.segments); setDirty(false); }}>Discard edits</button><button className="primary" disabled={locked} onClick={save}>Save edits</button></>}
              <button className="secondary" disabled={locked || dirty || !draft.length} onClick={() => exportTranscript('txt')}>Export TXT</button>
              <button className="secondary" disabled={locked || dirty || !draft.length} onClick={() => exportTranscript('srt')}>Export SRT</button>
            </div></div>
            {dirty && <p className="unsaved" role="status">Unsaved edits — save or discard before opening another recording.</p>}
            <div className="transcript">{draft.map((segment, index) => {
              const isActive = index === activeIndex;
              return <div className={`segment ${isActive ? 'active' : ''}`} key={index} aria-current={isActive ? 'true' : undefined}>
                <button type="button" className="timecode" disabled={!audioUrl}
                  aria-label={`Seek to ${formatClock(segment.startMs)}`}
                  onClick={() => seekTo(segment.startMs)}>{formatClock(segment.startMs)}</button>
                <div className="segment-content"><label className="speaker-label"><span className="sr-only">Speaker</span>
                  <input type="text" aria-label={`Speaker at ${formatClock(segment.startMs)}`} maxLength={100} placeholder={hasSpeakerLabels ? 'Unassigned' : 'Not detected'} value={displaySpeaker(segment.speaker)} disabled={locked}
                    onChange={e => { const speaker = e.target.value.replace(/^Speaker (\d+)$/, 'speaker_$1') || null; setDraft(current => current.map((item, n) => n === index ? { ...item, speaker } : item)); setDirty(true); }}/>
                </label><TranscriptText label={`Transcript at ${formatClock(segment.startMs)}`} text={segment.text} disabled={locked}
                  onChange={text => { setDraft(current => current.map((item, n) => n === index ? { ...item, text } : item)); setDirty(true); }}/></div>
              </div>;
            })}</div>
          </>}
        </section>}
        <footer className="workspace-footer"><span>Audio is never uploaded for transcription.</span><span>Playback and speaker detection stay on your device.</span></footer>
      </div>
    </main>
    {recordingMenu && <RecordingMenu target={recordingMenu} onClose={closeMenu} onDelete={note => { closeMenu(); if (!locked && !dirty) { setError(null); setConfirmDelete(note); } }}/>}
    {confirmDelete && <DeleteRecordingDialog title={confirmDelete.title} busy={Boolean(busy)} error={error} onClose={() => setConfirmDelete(null)} onDelete={deleteRecording}/>}
  </div>;
}
