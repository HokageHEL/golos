import { useCallback, useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { api, dictationPhaseLabel, type DictationStatus } from './api';
import './dictation.css';

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

function emptyStatus(): DictationStatus {
  return {
    enabled: false, shortcut: '', shortcutDisplay: '', mode: 'toggle', language: 'auto',
    phase: 'idle', active: false, overlayVisible: false, showOverlay: true, overlayAlwaysOnTop: true,
    restoreClipboard: true, aiEnabled: false, aiKind: 'cleanup', aiPrompt: '', appOverrides: [],
    shortcutError: null, startupFailed: false, insertionSupported: false, permissions: null,
    target: null, noteId: null, originalText: '', processedText: null, previewText: '',
    processedModel: null, processedPrompt: null, error: null, warning: null, pasteMessage: null,
    clipboard: null, recovered: false, canStart: false, canPause: false, canDiscard: false,
    canRetrySave: false, canRetryTranscribe: false, canRetryProcess: false, canInsert: false,
    canDismiss: false,
  };
}

/**
 * The recording overlay is button-only and never takes keyboard focus, so the
 * editable review lives in the main window instead.
 */
export default function Overlay() {
  const [status, setStatus] = useState<DictationStatus>(emptyStatus);
  const [error, setError] = useState<string | null>(null);
  const [confirmDiscard, setConfirmDiscard] = useState(false);

  useEffect(() => {
    let disposed = false;
    void api.dictationStatus().then(value => { if (!disposed) setStatus(value); })
      .catch(e => { if (!disposed) setError(errorText(e)); });
    const subscription = listen<DictationStatus>('dictation-state', event => {
      if (!disposed) setStatus(event.payload);
    });
    subscription.catch(e => { if (!disposed) setError(errorText(e)); });
    return () => { disposed = true; void subscription.then(unlisten => unlisten()).catch(() => {}); };
  }, []);

  const run = useCallback(async (work: () => Promise<unknown>) => {
    setError(null);
    try { await work(); } catch (e) { setError(errorText(e)); }
  }, []);

  const phase = status.phase;
  return <div className="dictation-overlay" role="group" aria-label="Global dictation">
    <div className="dictation-overlay-head">
      <span className={`dictation-dot ${phase === 'recording' ? 'live' : ''}`} aria-hidden="true" />
      <strong>{dictationPhaseLabel[phase]}</strong>
      {status.target && <span className="dictation-overlay-target">{status.target.name}</span>}
    </div>
    {status.error && <p className="dictation-overlay-error" role="alert">{status.error}</p>}
    {error && <p className="dictation-overlay-error" role="alert">{error}</p>}
    <div className="dictation-overlay-actions">
      {status.canPause && <button type="button" className="secondary"
        onClick={() => void run(() => api.dictationPause(phase === 'paused'))}>
        {phase === 'paused' ? 'Resume' : 'Pause'}
      </button>}
      {(phase === 'recording' || phase === 'paused' || phase === 'stopped') &&
        <button type="button" className="primary" onClick={() => void run(() => api.dictationStop())}>Stop and save</button>}
      {status.canRetrySave && <button type="button" className="primary"
        onClick={() => void run(() => api.dictationRetrySave())}>Retry save</button>}
      {status.canRetryTranscribe && <button type="button" className="primary"
        onClick={() => void run(() => api.dictationRetryTranscribe())}>Retry transcription</button>}
      {status.canDismiss && <button type="button" className="secondary"
        onClick={() => void run(() => api.dictationDismiss())}>Dismiss</button>}
      {status.canDiscard && !confirmDiscard && <button type="button" className="secondary"
        onClick={() => setConfirmDiscard(true)}>Discard…</button>}
      {status.canDiscard && confirmDiscard && <>
        <button type="button" className="secondary" onClick={() => setConfirmDiscard(false)}>Keep recording</button>
        <button type="button" className="danger-button" onClick={() => void run(() => api.dictationDiscard())}>Discard recording</button>
      </>}
      <button type="button" className="secondary" onClick={() => void run(() => api.dictationHideOverlay())}>Hide</button>
    </div>
    <p className="dictation-overlay-note">This overlay cannot take keyboard focus. Review and edit the text in the Golos window.</p>
  </div>;
}
