import { useEffect, useRef, useState } from 'react';
import { api, type DictationAppOverride, type DictationSettings, type DictationStatus, type Language } from './api';
import './dictation.css';

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

const emptySettings = (): DictationSettings => ({
  enabled: false, shortcut: '', mode: 'toggle', language: 'auto', showOverlay: true,
  overlayAlwaysOnTop: true, restoreClipboard: true, aiEnabled: false, aiKind: 'cleanup',
  aiPrompt: '', appOverrides: [],
});

type Props = {
  open: boolean;
  onClose: () => void;
  status: DictationStatus | null;
  onStatus: (status: DictationStatus) => void;
  onError: (message: string | null) => void;
  nativeAvailable: boolean;
};

/**
 * The global dictation settings. Every section is a small, collapsible group
 * so the dialog stays short, and the draft survives switching between
 * sections until it is saved.
 */
export default function DictationSettings({ open, onClose, status, onStatus, onError, nativeAvailable }: Props) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [settings, setSettings] = useState<DictationSettings>(emptySettings);
  const [languages, setLanguages] = useState<Language[]>([]);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const element = dialog.current;
    if (!open) { element?.close(); return; }
    element?.showModal();
    let disposed = false;
    void api.dictationSettings().then(value => { if (!disposed) setSettings(value); })
      .catch(e => { if (!disposed) setError(errorText(e)); });
    void api.languages().then(value => { if (!disposed) setLanguages(value); })
      .catch(() => { if (!disposed) setLanguages([]); });
    return () => { disposed = true; };
  }, [open]);

  if (!open) return null;

  function update(patch: Partial<DictationSettings>) {
    setSettings(current => ({ ...current, ...patch }));
  }

  function updateOverride(index: number, patch: Partial<DictationAppOverride>) {
    setSettings(current => ({
      ...current,
      appOverrides: current.appOverrides.map((item, position) => position === index ? { ...item, ...patch } : item),
    }));
  }

  async function save() {
    onError(null);
    setError(null);
    setMessage(null);
    try {
      const saved = await api.dictationConfigure(settings);
      setSettings(saved);
      setMessage('Dictation settings saved on this device.');
      onStatus(await api.dictationStatus());
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function permissions() {
    setError(null);
    try {
      await api.dictationPermissions();
      setMessage('Checked the current access. Granting access never inserts text by itself.');
      onStatus(await api.dictationStatus());
    } catch (e) {
      setError(errorText(e));
    }
  }

  return <dialog ref={dialog} className="settings-dialog dictation-settings" aria-labelledby="dictation-settings-title"
    onCancel={event => { event.preventDefault(); onClose(); }}
    onClick={event => { if (event.target === event.currentTarget) onClose(); }}>
    <div className="settings-content">
      <header className="settings-header">
        <div><div className="eyebrow">GLOBAL DICTATION</div><h1 id="dictation-settings-title">Dictation</h1></div>
        <button type="button" className="secondary settings-close" aria-label="Close dictation settings" onClick={onClose} autoFocus>✕</button>
      </header>
      {error && <div className="notice error" role="alert">{error}</div>}
      {message && <div className="notice success" role="status">{message}</div>}
      {status?.shortcutError && <div className="notice error" role="alert">{status.shortcutError}</div>}

      <details className="dictation-settings-section" open>
        <summary>Shortcut</summary>
        <div className="dictation-settings-body">
          <label className="dictation-settings-row">
            <input type="checkbox" checked={settings.enabled} disabled={!nativeAvailable}
              onChange={event => update({ enabled: event.target.checked })}/>
            Enable the global dictation shortcut
          </label>
          <label className="dictation-settings-row">
            Shortcut
            <input type="text" value={settings.shortcut} maxLength={100} spellCheck={false}
              onChange={event => update({ shortcut: event.target.value })}
              aria-label="Dictation shortcut"/>
          </label>
          <p className="dictation-settings-hint">
            One real key with Command, Control or Option, for example Command+Shift+Space.
            A shortcut that the system already uses is refused and the working one stays.
          </p>
          {status?.shortcutDisplay && <p className="dictation-settings-hint">
            Active shortcut: {status.shortcutDisplay}
          </p>}
          <fieldset className="dictation-settings-row">
            <legend className="sr-only">Shortcut mode</legend>
            <label><input type="radio" name="dictation-mode" checked={settings.mode === 'toggle'}
              onChange={() => update({ mode: 'toggle' })}/> Toggle: press to start, press to stop</label>
            <label><input type="radio" name="dictation-mode" checked={settings.mode === 'hold'}
              onChange={() => update({ mode: 'hold' })}/> Hold: press and hold to talk</label>
          </fieldset>
          <label className="dictation-settings-row">
            Dictation language
            <select value={settings.language} onChange={event => update({ language: event.target.value })}
              aria-label="Dictation language">
              <option value="auto">Detect automatically</option>
              {languages.map(item => <option key={item.code} value={item.code}>{item.name}</option>)}
            </select>
          </label>
        </div>
      </details>

      <details className="dictation-settings-section">
        <summary>Overlay and clipboard</summary>
        <div className="dictation-settings-body">
          <label className="dictation-settings-row">
            <input type="checkbox" checked={settings.showOverlay}
              onChange={event => update({ showOverlay: event.target.checked })}/>
            Show the recording overlay
          </label>
          <label className="dictation-settings-row">
            <input type="checkbox" checked={settings.overlayAlwaysOnTop}
              onChange={event => update({ overlayAlwaysOnTop: event.target.checked })}/>
            Keep the overlay on top of other windows
          </label>
          <label className="dictation-settings-row">
            <input type="checkbox" checked={settings.restoreClipboard}
              onChange={event => update({ restoreClipboard: event.target.checked })}/>
            Restore the previous clipboard content after an insertion
          </label>
          <p className="dictation-settings-hint">
            The overlay shows buttons only and never takes keyboard focus. The editable review stays in the Golos window.
            With restoration off, the dictated text stays on the clipboard until you replace it.
          </p>
        </div>
      </details>

      <details className="dictation-settings-section">
        <summary>Local processing (optional)</summary>
        <div className="dictation-settings-body">
          <label className="dictation-settings-row">
            <input type="checkbox" checked={settings.aiEnabled}
              onChange={event => update({ aiEnabled: event.target.checked })}/>
            Process dictated text with the local model
          </label>
          <label className="dictation-settings-row">
            Task
            <select value={settings.aiKind} onChange={event => update({ aiKind: event.target.value })}
              aria-label="Local processing task">
              <option value="cleanup">Clean up punctuation and filler words</option>
              <option value="custom">Custom prompt</option>
            </select>
          </label>
          <label>
            Prompt
            <textarea value={settings.aiPrompt} maxLength={4000}
              onChange={event => update({ aiPrompt: event.target.value })}
              aria-label="Local processing prompt"/>
          </label>
          <p className="dictation-settings-hint">
            Processing runs on the local model you configured in Settings. The original transcript is always kept separately.
          </p>
          <div className="dictation-settings-hint">Per-app overrides</div>
          {settings.appOverrides.map((item, index) => <div className="dictation-override-row" key={`${item.bundleId}-${index}`}>
            <input type="text" value={item.bundleId} placeholder="com.example.App"
              aria-label={`Bundle ID for override ${index + 1}`}
              onChange={event => updateOverride(index, { bundleId: event.target.value })}/>
            <input type="text" value={item.aiPrompt ?? ''} placeholder="Prompt for this app"
              aria-label={`Prompt for ${item.bundleId || `override ${index + 1}`}`}
              onChange={event => updateOverride(index, { aiPrompt: event.target.value })}/>
            <button type="button" className="secondary"
              onClick={() => update({ appOverrides: settings.appOverrides.filter((_, position) => position !== index) })}>
              Remove override
            </button>
          </div>)}
          <button type="button" className="secondary" onClick={() => update({
            appOverrides: [...settings.appOverrides, { bundleId: '', aiEnabled: null, aiKind: null, aiPrompt: null }],
          })}>Add app override</button>
          <p className="dictation-settings-hint">
            Overrides match one bundle ID exactly. A prompt is plain text: it is never executed and never uploaded.
          </p>
        </div>
      </details>

      <details className="dictation-settings-section">
        <summary>Text insertion permissions</summary>
        <div className="dictation-settings-body">
          <div className="dictation-permissions">
            <button type="button" className="secondary" onClick={() => void (async () => {
              try { await api.dictationRequestPermission('accessibility'); setMessage('Requested accessibility access. Approving it never inserts text by itself.'); onStatus(await api.dictationStatus()); }
              catch (e) { setError(errorText(e)); }
            })()}>Request access</button>
            <button type="button" className="secondary" onClick={() => void (async () => {
              try { await api.dictationOpenPermissionSettings('accessibility'); }
              catch (e) { setError(errorText(e)); }
            })()}>Open Settings</button>
            <button type="button" className="secondary" onClick={() => void permissions()}>Check again</button>
          </div>
          <p className="dictation-settings-hint">
            Recording, transcription and copying work without this permission. Only the Insert button needs it.
          </p>
        </div>
      </details>

      <div className="dictation-settings-actions">
        <button type="button" className="secondary" onClick={onClose}>Close</button>
        <button type="button" className="secondary" onClick={() => void (async () => {
          try { onStatus(await api.dictationStart()); setMessage('Dictation started. The overlay shows the recording controls.'); }
          catch (e) { setError(errorText(e)); }
        })()}>Start dictation now</button>
        <button type="button" className="primary" onClick={() => void save()}>Save dictation settings</button>
      </div>
    </div>
  </dialog>;
}
