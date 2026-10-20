import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { api, type BatchDropPreview, type BatchImportResult, type Job, type Language } from './api';
import { formatClock } from './playback';

type Props = {
  languages: Language[];
  modelReady: boolean;
  busy: string | null;
  backgroundBusy?: boolean;
  progress: number;
  error: string | null;
  onOperation: (label: string, task: () => Promise<void>) => Promise<void>;
  onChanged: () => Promise<void>;
  onClose: () => void;
};

function importReport(result: BatchImportResult) {
  const duplicates = result.duplicates
    ? ` ${result.duplicates} duplicate file${result.duplicates === 1 ? '' : 's'} skipped within this selection.`
    : '';
  return {
    text: `${result.notes.length} recordings imported and queued.${duplicates}${result.cancelled ? ' Import cancelled; completed imports were kept.' : ''}`,
    failures: result.failures,
  };
}

export default function BatchQueue({ languages, modelReady, busy, backgroundBusy = false, progress, error, onOperation, onChanged, onClose }: Props) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [language, setLanguage] = useState('auto');
  const [localError, setLocalError] = useState<string | null>(null);
  const [report, setReport] = useState<{ text: string; failures: string[] } | null>(null);
  const [cancelled, setCancelled] = useState(false);
  const [drop, setDrop] = useState<BatchDropPreview | null>(null);
  // The effect below owns native listeners for the dialog's lifetime. Keep the
  // current grant id in a ref so closing also invalidates an unconsumed drop.
  const dropId = useRef<string | null>(null);
  const refresh = useCallback(async () => { setJobs(await api.jobs()); }, []);
  useEffect(() => {
    const element = dialog.current; element?.showModal();
    let disposed = false;
    const reload = () => { void api.jobs().then(rows => { if (!disposed) setJobs(rows); }).catch(e => { if (!disposed) setLocalError(String(e)); }); };
    const pending: Promise<() => void>[] = [];
    const subscribe = <T,>(name: string, callback: (payload: T) => void) => {
      const subscription = listen<T>(name, event => { if (!disposed) callback(event.payload); });
      subscription.catch(e => { if (!disposed) setLocalError(String(e)); });
      pending.push(subscription);
    };
    reload();
    subscribe<unknown>('queue-changed', reload);
    subscribe<BatchDropPreview>('batch-drop', preview => {
      if (!preview.names.length || preview.names.length > 100) {
        setLocalError('Drop between 1 and 100 local audio files at a time. The queue can hold 1,000 pending recordings.');
        return;
      }
      dropId.current = preview.id;
      setDrop(preview);
      setLocalError(null);
      setReport(null);
    });
    subscribe<{ message: string }>('batch-drop-rejected', rejection => setLocalError(rejection.message));
    return () => {
      disposed = true;
      element?.close();
      pending.forEach(subscription => { void subscription.then(unlisten => unlisten()).catch(() => {}); });
      // Closing the dialog clears its preview and its native one-shot grant.
      if (dropId.current) void api.discardDroppedBatch(dropId.current).catch(() => {});
      dropId.current = null;
    };
  }, []);

  async function operation(label: string, task: () => Promise<void>) {
    setLocalError(null); setReport(null); setCancelled(false);
    await onOperation(label, async () => { try { await task(); } finally { await refresh(); await onChanged(); } });
  }

  async function discardDroppedSelection() {
    const id = dropId.current;
    dropId.current = null;
    setDrop(null); setLocalError(null); setReport(null);
    if (id) {
      try { await api.discardDroppedBatch(id); } catch (e) { setLocalError(String(e)); }
    }
  }

  function addDroppedSelection() {
    if (!drop) return;
    const selection = drop;
    void operation('Importing dropped batch', async () => {
      const result = await api.importDroppedBatch(selection.id, language);
      // Keep the preview if a busy claim failed before Rust consumed the grant.
      if (dropId.current === selection.id) {
        dropId.current = null;
        setDrop(null);
      }
      setReport(importReport(result));
    });
  }

  const pending = jobs.filter(job => job.status === 'queued');
  const running = jobs.find(job => job.status === 'running');
  return <dialog ref={dialog} className="settings-dialog batch-dialog" aria-labelledby="batch-title" onCancel={event => { event.preventDefault(); if (!busy) onClose(); }}>
    <div className="settings-content">
      <header className="settings-header"><div><div className="eyebrow">ON THIS DEVICE</div><h1 id="batch-title">Batch transcription</h1></div><button className="secondary" disabled={!!busy} onClick={onClose} aria-label="Close batch queue" autoFocus>✕</button></header>
      <p>Import up to 100 files at a time. Each recording keeps the 30-minute / 512-MiB limit. Jobs are saved locally and run one at a time. Existing transcripts are never replaced. Open this dialog first, then drop local audio files anywhere in this window. Dropped files are only previewed until you explicitly add them.</p>
      {(error || localError) && <div className="notice error" role="alert">{error || localError}</div>}
      {drop && <section className="notice batch-drop-preview" aria-label="Dropped files ready to add">
        <p><strong>{drop.names.length} dropped file{drop.names.length === 1 ? '' : 's'} ready.</strong> Nothing has been imported or transcribed yet.</p>
        <ul aria-label="Dropped file names">{drop.names.map((name, index) => <li key={`${name}-${index}`}>{name}</li>)}</ul>
        <p className="muted">The batch audio language below applies to all added files. Dropping again replaces this preview.</p>
        <div className="editor-tools">
          <button className="primary" disabled={!!busy} onClick={addDroppedSelection}>Add dropped files</button>
          <button className="secondary" onClick={() => { void discardDroppedSelection(); }}>Discard selection</button>
        </div>
      </section>}
      {report && <div className="notice" role="status"><p>{report.text}</p>{report.failures.length > 0 && <ul>{report.failures.map((failure, index) => <li key={index}>{failure}</li>)}</ul>}</div>}
      <div className="editor-tools"><label>Batch audio language<select aria-label="Batch audio language" disabled={!!busy} value={language} onChange={event => setLanguage(event.target.value)}><option value="auto">Detect automatically</option>{languages.map(item => <option value={item.code} key={item.code}>{item.name}</option>)}</select></label>
        <button className="secondary" disabled={!!busy} onClick={() => { void operation('Importing batch', async () => { const result = await api.importBatch(language); setReport(importReport(result)); }); }}>Add files to queue</button>
        <button className="primary" disabled={!!busy || !pending.length || !modelReady} onClick={() => { void operation('Transcribing batch', async () => { await api.runQueue(); setReport({ text: 'Batch stopped. Results and remaining jobs are saved on this device. Retry failed or cancelled recordings individually.', failures: [] }); }); }}>Start / resume queue</button>
      </div>
      <p className="muted">Drops are ignored while another operation is running; wait or cancel it, then drop again. Files are read only after you choose Add dropped files or Add files to queue.</p>
      {!modelReady && <p className="muted">Install a transcription model in Settings before starting. You can import and queue files now.</p>}
      <p role="status">{pending.length} queued · {formatClock(pending.reduce((sum, job) => sum + job.durationSeconds * 1000, 0))} audio{running ? ` · Processing ${running.title} (${progress}%)` : ''}</p>
      {busy && <div className="processing" role="status"><span>{busy}{cancelled ? ' · Cancellation requested…' : ''}</span>{running && <progress max={100} value={progress}/>}<button className="secondary" disabled={cancelled} onClick={() => { setCancelled(true); void (backgroundBusy ? api.pauseAutomation(true) : api.cancel()).catch(e => { setLocalError(String(e)); setCancelled(false); }); }}>Cancel current operation</button></div>}
      <p className="muted">Cancelling stops the current file and leaves later jobs queued. Retry the cancelled file to include it again. Interrupted work is marked for retry after restart. Manual jobs wait for Start / resume; opted-in watched-folder jobs can run automatically. Cancelling background work also pauses automation.</p>
      <ul className="batch-jobs" aria-label="Transcription jobs">{jobs.map(job => <li key={job.id}><div><strong>{job.title}</strong><span>{formatClock(job.durationSeconds * 1000)} · {job.language} · {job.status}</span>{job.error && <p className="error-text">{job.error}</p>}</div><div className="actions">{['failed', 'cancelled'].includes(job.status) && <button className="secondary" disabled={!!busy} onClick={() => { void operation('Retrying job', () => api.jobAction(job.id, 'retry')); }}>Retry {job.title}</button>}<button className="text-button" disabled={!!busy || job.status === 'running'} onClick={() => { void operation('Removing queue job', () => api.jobAction(job.id, 'remove')); }}>Remove {job.title} from queue</button></div></li>)}</ul>
      {!jobs.length && <p className="muted">No jobs yet. Add files here or queue a ready recording from the library.</p>}
      <p className="muted">Removing a job keeps its library recording and source file. Results appear in the library. Speaker detection can be run after transcription.</p>
    </div>
  </dialog>;
}
