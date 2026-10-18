import { useEffect, useRef } from 'react';
import type { Setup } from './api';
import type { Theme } from './theme';

type Props = {
  theme: Theme;
  onThemeChange: (theme: Theme) => void;
  setup: Setup | null;
  modelDisabled: boolean;
  loadingModel: boolean;
  dirty: boolean;
  nativeAvailable: boolean;
  error: string | null;
  message: string | null;
  onImportModel: () => void;
  onClose: () => void;
};

const themes = [
  { value: 'light', label: 'Light', description: 'Warm and bright' },
  { value: 'dark', label: 'Dark', description: 'Easy on the eyes' },
  { value: 'system', label: 'System', description: 'Follow your device' },
] as const;

export default function Settings({ theme, onThemeChange, setup, modelDisabled, loadingModel, dirty,
  nativeAvailable, error, message, onImportModel, onClose }: Props) {
  const dialog = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);

  return <dialog ref={dialog} className="settings-dialog" aria-labelledby="settings-title"
    onCancel={onClose} onClick={event => { if (event.target === event.currentTarget) onClose(); }}>
    <div className="settings-content">
      <header className="settings-header">
        <div><div className="eyebrow">MAKE IT YOURS</div><h1 id="settings-title">Settings</h1></div>
        <button className="secondary settings-close" aria-label="Close settings" onClick={onClose} autoFocus>✕</button>
      </header>

      <section className="settings-section" aria-labelledby="appearance-title">
        <h2 id="appearance-title">Appearance</h2>
        <p id="appearance-description">Choose how Golos looks. Your preference is saved on this device.</p>
        <fieldset className="theme-options" aria-describedby="appearance-description">
          <legend className="sr-only">Theme</legend>
          {themes.map(option => <label className="theme-option" key={option.value}>
            <input type="radio" name="theme" value={option.value} checked={theme === option.value}
              onChange={() => onThemeChange(option.value)}/>
            <span className="theme-option-body">
              <span className={`theme-preview theme-preview-${option.value}`} aria-hidden="true"><span/><span/></span>
              <span className="theme-option-label">{option.label}<span className="theme-check" aria-hidden="true">✓</span></span>
              <span className="theme-option-description">{option.description}</span>
            </span>
          </label>)}
        </fieldset>
      </section>

      <section className="settings-section" aria-labelledby="model-title">
        <h2 id="model-title">Transcription model</h2>
        <p>Import a model once to transcribe offline. Audio stays on your device.</p>
        <div className="model-card">
          <div><div className="eyebrow">OFFLINE ENGINE</div>
            <h3>{setup?.modelReady ? setup.modelName : 'No model selected'}</h3>
            <p>{setup?.modelReady ? 'Ready for local transcription.' : 'Choose a supported Whisper model to get started.'}</p>
          </div>
          <button className="secondary" disabled={modelDisabled} onClick={onImportModel}>
            {loadingModel ? 'Loading model…' : setup?.modelReady ? 'Replace model' : 'Import model'}
          </button>
        </div>
        <p className="model-guidance">Recommended: multilingual Whisper Small (<code>ggml-small-q5_1.bin</code>, about 190 MB).
          Tiny and Base also work. English-only models support English audio only.</p>
        {!nativeAvailable && <p className="settings-hint">Open the desktop app to import a model.</p>}
        {dirty && <p className="settings-hint">Save or discard transcript edits before replacing the model.</p>}
        {error && <div className="notice error" role="alert">{error}</div>}
        {message && <div className="notice success" role="status">{message}</div>}
      </section>
      <footer className="settings-footer"><span>Preferences stay on this device.</span><button className="primary" onClick={onClose}>Done</button></footer>
    </div>
  </dialog>;
}
