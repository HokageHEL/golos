import { useEffect, useRef, useState } from 'react';
import type { ModelOption, ModelProgress, Setup } from './api';
import type { Theme } from './theme';
import './settings.css';

export type SettingsSection = 'appearance' | 'transcription' | 'speakers';

type Props = {
  initialSection: SettingsSection;
  theme: Theme;
  onThemeChange: (theme: Theme) => void;
  language: string;
  onLanguageChange: (value: string) => void;
  autoSpeakers: boolean;
  onAutoSpeakersChange: (value: boolean) => void;
  speakerCount: string | number;
  onSpeakerCountChange: (value: string) => void;
  preferencesDisabled: boolean;
  setup: Setup | null;
  modelDisabled: boolean;
  loadingModel: boolean;
  dirty: boolean;
  nativeAvailable: boolean;
  error: string | null;
  message: string | null;
  onImportModel: () => void;
  modelCatalog: ModelOption[];
  catalogLoading: boolean;
  catalogError: string | null;
  onDownloadModel: (id: string) => void;
  modelOperation: 'import' | 'download' | null;
  downloadModelId: string | null;
  modelProgress: ModelProgress | null;
  cancelRequested: boolean;
  onCancelModel: () => void;
  onImportSpeakerModels: () => void;
  loadingSpeakers: boolean;
  onClose: () => void;
};

const themes = [
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
  { value: 'system', label: 'System' },
] as const;

export default function Settings({ initialSection, theme, onThemeChange, language, onLanguageChange,
  autoSpeakers, onAutoSpeakersChange, speakerCount, onSpeakerCountChange, preferencesDisabled,
  setup, modelDisabled, loadingModel, dirty, nativeAvailable, error, message, onImportModel,
  modelCatalog, catalogLoading, catalogError, onDownloadModel, modelOperation, downloadModelId,
  modelProgress, cancelRequested, onCancelModel,
  onImportSpeakerModels, loadingSpeakers, onClose }: Props) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [section, setSection] = useState<SettingsSection>(initialSection);
  const sections = ['appearance', 'transcription', 'speakers'] as const;

  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    (element?.querySelector<HTMLElement>('[autofocus]') ?? element?.querySelector<HTMLElement>('button'))?.focus();
    return () => element?.close();
  }, []);

  return <dialog ref={dialog} className="settings-dialog" aria-labelledby="settings-title"
    onCancel={event => { event.preventDefault(); onClose(); }}
    onClick={event => { if (event.target === event.currentTarget) onClose(); }}>
    <div className="settings-content">
      <header className="settings-header">
        <h1 id="settings-title">Settings</h1>
        <button className="secondary settings-close" aria-label="Close settings" onClick={onClose} autoFocus>✕</button>
      </header>

      <div className="settings-tabs" role="tablist" aria-label="Settings sections">
        {sections.map(value => <button key={value} type="button" role="tab" id={`settings-tab-${value}`}
          aria-controls={`settings-panel-${value}`} aria-selected={section === value}
          tabIndex={section === value ? 0 : -1} onClick={() => setSection(value)}
          onKeyDown={event => {
            if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
            event.preventDefault();
            const index = sections.indexOf(value);
            const next = event.key === 'Home' ? 0 : event.key === 'End' ? sections.length - 1
              : (index + (event.key === 'ArrowRight' ? 1 : -1) + sections.length) % sections.length;
            setSection(sections[next]);
            document.getElementById(`settings-tab-${sections[next]}`)?.focus();
          }}>{value === 'appearance' ? 'Appearance' : value === 'transcription' ? 'Transcription' : 'Speakers'}</button>)}
      </div>

      {(error || message || modelOperation) && <div className="settings-feedback">
        {error && <div className="notice error" role="alert">{error}</div>}
        {message && <div className="notice success" role="status">{message}</div>}
        {modelOperation && <ModelOperationStatus progress={modelProgress} cancelRequested={cancelRequested} onCancel={onCancelModel}/>}
      </div>}

      <section className="settings-section" role="tabpanel" id="settings-panel-appearance"
        aria-labelledby="settings-tab-appearance" hidden={section !== 'appearance'} tabIndex={0}>
        <fieldset className="theme-options">
          <legend className="settings-legend">Theme</legend>
          {themes.map(option => <label className="theme-option" key={option.value}>
            <input type="radio" name="theme" value={option.value} checked={theme === option.value}
              onChange={() => onThemeChange(option.value)}/>
            <span className="theme-option-body">
              <span className={`theme-preview theme-preview-${option.value}`} aria-hidden="true"><span/><span/></span>
              <span className="theme-option-label">{option.label}<span className="theme-check" aria-hidden="true">✓</span></span>
            </span>
          </label>)}
        </fieldset>
      </section>

      <section className="settings-section" role="tabpanel" id="settings-panel-transcription"
        aria-labelledby="settings-tab-transcription" hidden={section !== 'transcription'} tabIndex={0}>
        <label className="settings-field">Audio language
          <select value={language} disabled={preferencesDisabled} onChange={event => onLanguageChange(event.target.value)}>
            <option value="auto">Detect automatically</option><option value="en">English</option><option value="uk">Ukrainian</option>
          </select>
        </label>
        <div className="settings-model">
          <div className="settings-model-info"><h3>Transcription model</h3>
            <p>{setup?.modelReady ? setup.modelName : 'No model selected'}</p>
          </div>
          <button className="secondary" disabled={modelDisabled} onClick={onImportModel}>
            {loadingModel ? 'Loading model…' : setup?.modelReady ? 'Replace model' : 'Import model'}
          </button>
        </div>
        <section className="model-downloads" aria-labelledby="model-downloads-title">
          <h3 id="model-downloads-title">Optional model downloads</h3>
          <p>Choose a model to download it now. Downloads contact Hugging Face and its CDN only after you click a download button. Your audio and transcripts stay on this device. Downloads are checked and loaded before they can replace your current model.</p>
          <p className="offline-model-hint">For offline setup, transfer a compatible Whisper <code>.bin</code> file to this device, then use Import model above.</p>
          {catalogLoading && <p role="status">Checking available models…</p>}
          {catalogError && <p className="notice error" role="alert">Model downloads are unavailable: {catalogError} You can still import a model file.</p>}
          {!catalogLoading && !catalogError && modelCatalog.length === 0 && <p>No downloadable models are available. You can still import a model file.</p>}
          {modelCatalog.map(model => <div className="model-download" key={model.id}>
            <div><h4>{model.name}</h4><p><code>{model.fileName}</code> · {formatBytes(model.bytes)}</p><p>{model.description}</p></div>
            <button className="secondary" aria-label={`Download ${model.name}`} disabled={modelDisabled || !nativeAvailable || Boolean(modelOperation)}
              onClick={() => onDownloadModel(model.id)}>
              {modelOperation === 'download' && downloadModelId === model.id ? 'Downloading…' : 'Download'}
            </button>
          </div>)}
        </section>
        <details className="settings-guidance"><summary>Choosing a transcription model</summary>
          <p>For multilingual audio, we recommend Whisper Small (<code>ggml-small-q5_1.bin</code>, about 190 MB). Tiny, Base, Medium, Large and Large V3 Turbo also work. Larger models need more memory; English-only models support English audio only.</p>
        </details>
      </section>

      <section className="settings-section" role="tabpanel" id="settings-panel-speakers"
        aria-labelledby="settings-tab-speakers" hidden={section !== 'speakers'} tabIndex={0}>
        <label className="settings-checkbox"><input type="checkbox" checked={autoSpeakers} disabled={preferencesDisabled}
          onChange={event => onAutoSpeakersChange(event.target.checked)}/> Detect speakers after transcription</label>
        <label className="settings-field">Number of speakers
          <select value={speakerCount} disabled={preferencesDisabled} onChange={event => onSpeakerCountChange(event.target.value)}>
            <option value="auto">Detect automatically</option>{[1, 2, 3, 4, 5, 6, 8, 10, 20].map(count => <option key={count} value={count}>{count}</option>)}
          </select>
        </label>
        <div className="settings-model">
          <div className="settings-model-info"><h3>Speaker models</h3>
            <p>{setup?.diarizationReady ? 'Ready for speaker detection' : 'Not installed'}</p>
          </div>
          <button className="secondary" disabled={modelDisabled} onClick={onImportSpeakerModels}>
            {loadingSpeakers ? 'Loading speaker models…' : setup?.diarizationReady ? 'Replace speaker models' : 'Import speaker models'}
          </button>
        </div>
        <details className="settings-guidance"><summary>Choosing speaker models</summary>
          <p>Choose two different models: pyannote <code>model.int8.onnx</code> first, then <code>nemo_en_titanet_small.onnx</code> for speaker embeddings.</p>
        </details>
      </section>
      {(!nativeAvailable || dirty) && <div className="settings-hints">
        {!nativeAvailable && <p>Open the desktop app to import models.</p>}
        {dirty && <p>Save or discard transcript edits before replacing models.</p>}
      </div>}
      <footer className="settings-footer"><button className="primary" onClick={onClose}>Done</button></footer>
    </div>
  </dialog>;
}

export function ModelOperationStatus({ progress, cancelRequested, onCancel }: {
  progress: ModelProgress | null; cancelRequested: boolean; onCancel: () => void;
}) {
  if (!progress) return <div className="model-operation-status" aria-live="polite"><p role="status">Preparing model operation…</p></div>;
  const phaseText = {
    connecting: 'Connecting to model host…',
    copying: 'Copying model file…',
    downloading: 'Downloading model…',
    validating: 'Checking and preparing model…',
    complete: 'Model is ready.',
  }[progress.phase];
  const total = Math.max(0, progress.totalBytes);
  const copied = Math.max(0, Math.min(progress.copiedBytes, total || progress.copiedBytes));
  const showProgress = (progress.phase === 'copying' || progress.phase === 'downloading') && total > 0;
  return <div className="model-operation-status" aria-live="polite">
    <p role="status">{phaseText}{showProgress ? ` ${formatBytes(copied)} of ${formatBytes(total)}` : ''}</p>
    {showProgress && <progress max={total} value={copied} aria-label="Model setup progress"/>}
    {progress.phase !== 'complete' && <>
      {progress.phase === 'validating' && <p>Cancellation takes effect after the current check.</p>}
      <button className="secondary" disabled={cancelRequested} onClick={onCancel}>
        {cancelRequested ? 'Cancellation requested…' : progress.phase === 'validating' ? 'Cancel after check' : 'Cancel'}
      </button>
    </>}
  </div>;
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return 'Unknown size';
  if (bytes < 1_000_000) return `${Math.round(bytes / 1_000)} kB`;
  return `${(bytes / 1_000_000).toFixed(0)} MB`;
}
