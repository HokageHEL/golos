import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { api, type Job, type Language } from './api';
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

export default function BatchQueue({ languages, modelReady, busy, backgroundBusy = false, progress, error, onOperation, onChanged, onClose }: Props) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [language, setLanguage] = useState('auto');
  const [localError, setLocalError] = useState<string | null>(null);
  const [report, setReport] = useState<{ text: string; failures: string[] } | null>(null);
  const [cancelled, setCancelled] = useState(false);
  const refresh = useCallback(async () => { setJobs(await api.jobs()); }, []);
  useEffect(() => {
    const element = dialog.current; element?.showModal();
    let disposed = false;
    const reload = () => { void api.jobs().then(rows => { if (!disposed) setJobs(rows); }).catch(e => { if (!disposed) setLocalError(String(e)); }); };
    reload();
    const subscription = listen('queue-changed', reload);
    subscription.catch(e => { if (!disposed) setLocalError(String(e)); });
    return () => { disposed = true; element?.close(); void subscription.then(unlisten => unlisten()).catch(() => {}); };
  }, []);

  async function operation(label: string, task: () => Promise<void>) {
    setLocalError(null); setReport(null); setCancelled(false);
    await onOperation(label, async () => { try { await task(); } finally { await refresh(); await onChanged(); } });
  }
  const pending = jobs.filter(job => job.status === 'queued');
  const running = jobs.find(job => job.status === 'running');
  return <dialog ref={dialog} className="settings-dialog batch-dialog" aria-labelledby="batch-title" onCancel={event => { event.preventDefault(); if (!busy) onClose(); }}>
    <div className="settings-content">
      <header className="settings-header"><div><div className="eyebrow">ON THIS DEVICE</div><h1 id="batch-title">Batch transcription</h1></div><button className="secondary" disabled={!!busy} onClick={onClose} aria-label="Close batch queue" autoFocus>✕</button></header>
      <p>Import up to 100 files at a time. Each recording keeps the 30-minute / 512-MiB limit. Jobs are saved locally and run one at a time. Existing transcripts are never replaced.</p>
      {(error || localError) && <div className="notice error" role="alert">{error || localError}</div>}
      {report && <div className="notice" role="status"><p>{report.text}</p>{report.failures.length > 0 && <ul>{report.failures.map((failure, index) => <li key={index}>{failure}</li>)}</ul>}</div>}
      <div className="editor-tools"><label>Batch audio language<select aria-label="Batch audio language" disabled={!!busy} value={language} onChange={event => setLanguage(event.target.value)}><option value="auto">Detect automatically</option>{languages.map(item => <option value={item.code} key={item.code}>{item.name}</option>)}</select></label>
        <button className="secondary" disabled={!!busy} onClick={() => { void operation('Importing batch', async () => { const result = await api.importBatch(language); setReport({ text: `${result.notes.length} recordings imported and queued.${result.cancelled ? ' Import cancelled; completed imports were kept.' : ''}`, failures: result.failures }); }); }}>Add files to queue</button>
        <button className="primary" disabled={!!busy || !pending.length || !modelReady} onClick={() => { void operation('Transcribing batch', async () => { await api.runQueue(); setReport({ text: 'Batch stopped. Results and remaining jobs are saved on this device. Retry failed or cancelled recordings individually.', failures: [] }); }); }}>Start / resume queue</button>
      </div>
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
