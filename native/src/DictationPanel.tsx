import { useState } from 'react';
import { api, dictationPhaseLabel, type DictationStatus } from './api';
import './dictation.css';

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

const PHASE_HELP: Record<string, string> = {
  recording: 'Recording from your microphone. The recording stops at 30 minutes.',
  paused: 'Paused. The paused time is not recorded and the capture continues when you resume.',
  saving: 'Saving the captured audio to your library…',
  saveFailed: 'The audio could not be saved. The capture is kept so you can retry.',
  transcribing: 'Transcribing offline on this device…',
  processing: 'Applying the optional local prompt to the saved transcript…',
  preview: 'Review the text below. Nothing is inserted until you choose Insert.',
  pasteRequested: 'Paste requested; verify the target app.',
  failed: 'The dictation stopped. The saved recording is kept for a retry.',
};

// The clipboard outcome is reported exactly as the native adapter saw it. A
// pending restoration is never reported as "restored".
const CLIPBOARD_HELP: Record<string, string> = {
  unchanged: 'The clipboard was not changed.',
  restored: 'The previous clipboard content was restored.',
  keptNewerContent: 'A newer clipboard copy was kept, so the previous content was not restored.',
  transcriptRetained: 'The dictated text stays on the clipboard until you replace it.',
  restoreFailed: 'The clipboard could not be restored.',
};

type Props = {
  status: DictationStatus;
  onStatus: (status: DictationStatus) => void;
  onError: (message: string | null) => void;
  onOpenSettings: () => void;
};

/**
 * The review surface of the global dictation. It lives in the main window, so
 * the text is editable and the keyboard works. It never changes the library
 * selection and never replaces the unsaved editor draft.
 */
export default function DictationPanel({ status, onStatus, onError, onOpenSettings }: Props) {
  // The editable text is derived from the job: a draft only survives while it
  // still belongs to the preview it was started from.
  const [draft, setDraft] = useState<{ base: string; value: string } | null>(null);
  const text = draft && draft.base === status.previewText ? draft.value : status.previewText;
  const [message, setMessage] = useState<string | null>(null);
  const [confirmDiscard, setConfirmDiscard] = useState(false);

  async function run(work: () => Promise<DictationStatus | void>) {
    onError(null);
    setMessage(null);
    try {
      const next = await work();
      if (next) onStatus(next);
    } catch (e) {
      onError(errorText(e));
    }
  }

  async function copy() {
    await run(async () => {
      await navigator.clipboard.writeText(text);
      setMessage('Copied to the clipboard. The text stays on this device until you paste it.');
    });
  }

  async function insert() {
    await run(async () => {
      const outcome = await api.dictationInsert();
      setMessage(outcome.message);
      onStatus(await api.dictationStatus());
    });
  }

  async function permissions() {
    await run(async () => {
      const value = await api.dictationPermissions();
      setMessage(value.accessibility
        ? 'Accessibility access is granted. Text insertion is still an explicit step.'
        : 'Accessibility access is missing. Request it, then choose Check again.');
      onStatus(await api.dictationStatus());
    });
  }

  return <section className="dictation-panel" aria-label="Global dictation">
    <div className="dictation-panel-head">
      <h2>Global dictation · {dictationPhaseLabel[status.phase]}</h2>
      <button type="button" className="secondary" onClick={onOpenSettings}>Dictation settings</button>
    </div>
    {status.recovered && <p className="dictation-panel-note" role="status">
      This dictation was interrupted and restored for review only. Nothing is inserted automatically.
    </p>}
    {status.target && <p className="dictation-panel-note">Target: {status.target.name}</p>}
    <p className="dictation-panel-note">{PHASE_HELP[status.phase] ?? 'The dictation is idle.'}</p>
    {status.error && <p className="dictation-panel-error" role="alert">{status.error}</p>}
    {status.warning && <p className="dictation-panel-note" role="status">{status.warning}</p>}
    {status.pasteMessage && <p className="dictation-panel-message" role="status">{status.pasteMessage}</p>}
    {status.clipboard && <p className="dictation-panel-note">{CLIPBOARD_HELP[status.clipboard] ?? status.clipboard}</p>}
    {message && <p className="dictation-panel-message" role="status">{message}</p>}

    {status.noteId && <div className="dictation-panel-texts">
      <details open={!status.processedText}>
        <summary>Original transcript</summary>
        <pre>{status.originalText || 'The transcript is not available yet.'}</pre>
      </details>
      {status.processedText && <details open>
        <summary>Processed text</summary>
        <pre>{status.processedText}</pre>
        {status.processedModel && <p className="dictation-panel-provenance">
          Model {status.processedModel} · prompt: {status.processedPrompt}
        </p>}
      </details>}
      <label>
        Text to insert
        <textarea value={text} maxLength={200000} spellCheck={false}
          onChange={event => setDraft({ base: status.previewText, value: event.target.value })}
          onBlur={() => { if (text !== status.previewText) void run(() => api.dictationPreviewText(text)); }}
          aria-label="Text to insert"/>
      </label>
      <div className="dictation-panel-actions">
        <button type="button" className="secondary" onClick={() => void run(() => api.dictationPreviewText(text))}>Apply edits</button>
        <button type="button" className="secondary" onClick={() => void copy()}>Copy text</button>
        <button type="button" className="primary" disabled={!status.canInsert} onClick={() => void insert()}>Insert into target</button>
      </div>
    </div>}

    <div className="dictation-panel-actions">
      {status.canRetrySave && <button type="button" className="primary"
        onClick={() => void run(() => api.dictationRetrySave())}>Retry save</button>}
      {status.canRetryTranscribe && <button type="button" className="primary"
        onClick={() => void run(() => api.dictationRetryTranscribe())}>Retry transcription</button>}
      {status.canRetryProcess && <button type="button" className="secondary"
        onClick={() => void run(() => api.dictationRetryProcess())}>Retry local processing</button>}
      {status.canDismiss && <button type="button" className="secondary"
        onClick={() => void run(() => api.dictationDismiss())}>Dismiss and keep the note</button>}
      {status.canDiscard && !confirmDiscard && <button type="button" className="secondary"
        onClick={() => setConfirmDiscard(true)}>Discard…</button>}
      {status.canDiscard && confirmDiscard && <>
        <button type="button" className="secondary" onClick={() => setConfirmDiscard(false)}>Keep recording</button>
        <button type="button" className="danger-button"
          onClick={() => { setConfirmDiscard(false); void run(() => api.dictationDiscard()); }}>Discard recording</button>
      </>}
    </div>

    {status.phase === 'preview' && <div className="dictation-permissions">
      {status.insertionSupported
        ? <span>{status.permissions?.accessibility
          ? 'Accessibility access is granted.'
          : 'Accessibility access is missing. Recording and copying work without it.'}</span>
        : <span>Text insertion is not available in this build. Copy the text instead.</span>}
      {status.insertionSupported && <>
        <button type="button" className="secondary"
          onClick={() => void run(async () => { await api.dictationRequestPermission('accessibility'); onStatus(await api.dictationStatus()); })}>
          Request access
        </button>
        <button type="button" className="secondary"
          onClick={() => void run(async () => { await api.dictationOpenPermissionSettings('accessibility'); })}>
          Open Settings
        </button>
        <button type="button" className="secondary" onClick={() => void permissions()}>Check again</button>
      </>}
    </div>}
  </section>;
}
