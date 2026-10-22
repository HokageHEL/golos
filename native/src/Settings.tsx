import { useEffect, useRef, useState } from 'react';
import type { Setup } from './api';
import type { Theme } from './theme';
import SettingsNavigation from './SettingsNavigation';

type Props = {
  onDownloadModels: (speakers: boolean) => void;
  downloadProgress: { received: number; total: number; phase: string } | null;
  onCancelDownload: () => void;
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
  onImportSpeakerModels: () => void;
  loadingSpeakers: boolean;
  onClose: () => void;
};

const themes = [
  { value: 'light', label: 'Light', description: 'Warm and bright' },
  { value: 'dark', label: 'Dark', description: 'Easy on the eyes' },
  { value: 'system', label: 'System', description: 'Follow your device' },
] as const;

export default function Settings({ onDownloadModels, downloadProgress, onCancelDownload, theme, onThemeChange, setup, modelDisabled, loadingModel, dirty,
  nativeAvailable, error, message, onImportModel, onImportSpeakerModels, loadingSpeakers, onClose }: Props) {
  const dialog = useRef<HTMLDialogElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const [page, setPage] = useState(() => downloadProgress || loadingModel || !setup?.modelReady ? 'model' : 'appearance');
  useEffect(() => { if (content.current) content.current.scrollTop = 0; }, [page]);

  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);

  useEffect(() => {
    if (error) dialog.current?.querySelector('[role="alert"]')?.scrollIntoView({ block: 'nearest' });
  }, [error]);

  return <dialog ref={dialog} className="settings-dialog settings-workspace-dialog" aria-labelledby="settings-title"
    onCancel={onClose} onClick={event => { if (event.target === event.currentTarget) onClose(); }}>
    <div className="settings-content settings-workspace">
      <header className="settings-header">
        <div><div className="eyebrow">MAKE IT YOURS</div><h1 id="settings-title">Settings</h1></div>
        <button className="secondary settings-close" aria-label="Close settings" onClick={onClose} autoFocus>✕</button>
      </header>
      <div className="settings-workspace-body">
      <SettingsNavigation label="Settings sections" selected={page} onSelect={setPage} pages={[
        { id: 'appearance', label: 'Appearance', hint: 'Theme' },
        { id: 'model', label: 'Transcription', hint: 'Offline model' },
        { id: 'speakers', label: 'Speakers', hint: 'Optional models' },
      ]}/>
      <div className="settings-page-content" ref={content}>
        {error && <div className="notice error" role="alert">{error}</div>}
        {message && <div className="notice success" role="status">{message}</div>}
        {downloadProgress && <div role="status"><p>{downloadProgress.phase} · {(downloadProgress.received / 1_000_000).toFixed(1)} / {(downloadProgress.total / 1_000_000).toFixed(1)} MB</p><progress value={downloadProgress.received} max={downloadProgress.total}/><button className="secondary" onClick={onCancelDownload}>Cancel download</button></div>}

      <section id="settings-page-appearance" hidden={page !== 'appearance'} className="settings-section" aria-labelledby="appearance-title">
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

      <section id="settings-page-model" hidden={page !== 'model'} className="settings-section" aria-labelledby="model-title">
        <h2 id="model-title">Transcription model</h2>
        <p>Download a model once, then transcribe offline. Audio stays on your device.</p>
        <p>Multilingual Whisper Small · 190 MB download · 200 MB free disk space required. Source: whisper.cpp on Hugging Face. Verified with a pinned SHA-256.</p>
        <button className="primary" disabled={modelDisabled} onClick={() => onDownloadModels(false)}>Download recommended model</button>
        {error && !downloadProgress && <button className="secondary" disabled={modelDisabled} onClick={() => onDownloadModels(false)}>Retry recommended model</button>}
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
          Tiny, Base, Medium, Large, and Large V3 Turbo also work; larger models need more memory. English-only models support English audio only.</p>
        {!nativeAvailable && <p className="settings-hint">Open the desktop app to import a model.</p>}
        {dirty && <p className="settings-hint">Save or discard transcript edits before replacing the model.</p>}
      </section>
      <section id="settings-page-speakers" hidden={page !== 'speakers'} className="settings-section" aria-labelledby="speakers-title">
        <h2 id="speakers-title">Offline speaker detection</h2>
        <p>{setup?.diarizationReady ? 'Ready. Detection runs after transcription; existing transcripts can use Detect speakers.' : 'Import a pyannote segmentation model and a speaker embedding model to label recurring voices. Models and audio stay on this device.'}</p>
        <p>Guided setup downloads both models from sherpa-onnx on GitHub and verifies pinned SHA-256 checksums. 47.3 MB download · 120 MB free disk space required. Optional; transcription works without these models.</p>
        <button className="primary" disabled={modelDisabled} onClick={() => onDownloadModels(true)}>Download speaker models / Retry</button>
        <p>Choose pyannote <code>model.int8.onnx</code> first, then <code>nemo_en_titanet_small.onnx</code>. These are two different models.</p>
        <button className="secondary" disabled={modelDisabled} onClick={onImportSpeakerModels}>
          {loadingSpeakers ? 'Loading speaker models…' : setup?.diarizationReady ? 'Replace speaker models' : 'Import speaker models'}
        </button>
      </section>
      </div>
      </div>
      <footer className="settings-footer"><span>Preferences stay on this device.</span><button className="primary" onClick={onClose}>Done</button></footer>
    </div>
  </dialog>;
}
