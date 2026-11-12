// @vitest-environment jsdom
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import type { Note, Setup } from '../api';

const mocks = vi.hoisted(() => ({
  api: {
    setup: vi.fn(), list: vi.fn(), audioSource: vi.fn(), save: vi.fn(), delete: vi.fn(), export: vi.fn(),
    cancel: vi.fn(), detectSpeakers: vi.fn(), transcribe: vi.fn(), importAudio: vi.fn(), importModel: vi.fn(),
    importSpeakerModels: vi.fn(), modelCatalog: vi.fn(), downloadModel: vi.fn(),
  },
  listen: vi.fn(), modelProgressHandler: vi.fn(), onCloseRequested: vi.fn(),
}));

vi.mock('../api', () => ({ api: mocks.api, nativeAvailable: true }));
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onCloseRequested: mocks.onCloseRequested }) }));
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc: (path: string) => path }));
vi.mock('../AudioPlayer', () => ({ default: () => <div aria-label="audio player"/> }));
vi.mock('../TranscriptText', () => ({ default: ({ text, label, onChange }: {
  text: string; label: string; onChange: (text: string) => void;
}) => <textarea aria-label={label} value={text} onChange={event => onChange(event.target.value)}/> }));

import App from '../App';
import RecordingMenu, { type RecordingMenuTarget } from '../RecordingMenu';
import { readPreferences } from '../preferences';

const segment = { startMs: 0, endMs: 1200, speaker: null, text: 'Original words' };
const note: Note = {
  id: 'note-1', title: 'Interview', createdAt: 1_700_000_000, durationSeconds: 12,
  status: 'done', segments: [segment], error: null,
};
const setup: Setup = { modelReady: true, modelName: 'small', dataDirectory: '/data', diarizationReady: true };
let host: HTMLDivElement;
let root: Root;

async function render(element: React.ReactNode) {
  await act(async () => { root.render(element); await Promise.resolve(); });
}

async function settle() {
  await act(async () => { await Promise.resolve(); await Promise.resolve(); });
}

function key(target: Element, value: string) {
  act(() => { target.dispatchEvent(new KeyboardEvent('keydown', { key: value, bubbles: true, cancelable: true })); });
}

function setTextarea(textarea: HTMLTextAreaElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set;
  act(() => {
    setter?.call(textarea, value);
    textarea.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

beforeEach(() => {
  host = document.createElement('div'); document.body.append(host); root = createRoot(host);
  Object.defineProperty(window, 'matchMedia', { configurable: true, value: () => ({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() }) });
  HTMLDialogElement.prototype.showModal = function showModal() { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function close() { this.removeAttribute('open'); };
  mocks.api.setup.mockResolvedValue(setup);
  mocks.api.list.mockResolvedValue([note]);
  mocks.api.audioSource.mockResolvedValue('/audio.wav');
  mocks.api.export.mockResolvedValue(true);
  mocks.api.save.mockResolvedValue(undefined);
  mocks.api.delete.mockResolvedValue(undefined);
  mocks.api.detectSpeakers.mockResolvedValue([{ ...segment, speaker: 'Speaker 1' }]);
  mocks.api.importSpeakerModels.mockResolvedValue(true);
  mocks.api.modelCatalog.mockResolvedValue([
    { id: 'tiny', name: 'Tiny', fileName: 'ggml-tiny.bin', bytes: 77_691_713, description: 'Fastest model.' },
    { id: 'base', name: 'Base', fileName: 'ggml-base.bin', bytes: 147_951_465, description: 'Balanced model.' },
    { id: 'small', name: 'Small (quantized)', fileName: 'ggml-small-q5_1.bin', bytes: 190_085_487, description: 'Higher quality potential.' },
  ]);
  mocks.api.downloadModel.mockResolvedValue(undefined);
  mocks.api.importModel.mockResolvedValue(true);
  mocks.api.cancel.mockResolvedValue(undefined);
  mocks.listen.mockImplementation(async (name, handler) => {
    if (name === 'model-import-progress') mocks.modelProgressHandler.mockImplementation(handler);
    return vi.fn();
  });
  mocks.onCloseRequested.mockResolvedValue(vi.fn());
  (globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
});

afterEach(async () => {
  await act(async () => { root.unmount(); });
  host.remove(); vi.clearAllMocks(); localStorage.clear();
});

describe('recording actions menu', () => {
  function setupMenu() {
    const origin = document.createElement('button'); origin.textContent = 'Actions'; document.body.append(origin);
    const target: RecordingMenuTarget = { note, origin, x: 10, y: 10, workspace: true };
    const onClose = vi.fn(); const onDelete = vi.fn(); const onExport = vi.fn();
    const onDetect = vi.fn(); const onEditSpeakers = vi.fn(); const onSpeakerSettings = vi.fn();
    act(() => { root.render(<RecordingMenu target={target} onClose={onClose} onDelete={onDelete} onExport={onExport}
      onDetect={onDetect} onEditSpeakers={onEditSpeakers} onSpeakerSettings={onSpeakerSettings}
      canExport canDetect canEditSpeakers canDelete speakerReady/>); });
    return { origin, onClose, onDelete, onExport, onDetect, onEditSpeakers, onSpeakerSettings };
  }

  test('Escape dismisses and restores focus to the opener', async () => {
    const { origin, onClose } = setupMenu(); await settle();
    expect(document.activeElement?.textContent).toBe('Export ▸');
    key(document.activeElement!, 'Escape');
    expect(onClose).toHaveBeenCalled();
    expect(document.activeElement).toBe(origin);
  });

  test('keyboard opens export submenu and runs an export action', async () => {
    const { onExport, onClose } = setupMenu(); await settle();
    key(document.activeElement!, 'ArrowRight'); await settle();
    expect(document.activeElement?.textContent).toBe('Text (.txt)');
    act(() => { (document.activeElement as HTMLButtonElement).click(); });
    expect(onExport).toHaveBeenCalledWith('txt'); expect(onClose).toHaveBeenCalled();
  });

  test('speaker setup, manual edit, and delete actions call their handlers', async () => {
    const { onDetect, onEditSpeakers, onSpeakerSettings, onDelete } = setupMenu(); await settle();
    act(() => { (host.querySelector('[data-trigger="speakers"]') as HTMLButtonElement).click(); }); await settle();
    act(() => { (Array.from(host.querySelectorAll('button')).find(item => item.textContent === 'Detect speakers')!).click(); });
    expect(onDetect).toHaveBeenCalled();
    act(() => { (host.querySelector('[data-trigger="speakers"]') as HTMLButtonElement).click(); }); await settle();
    act(() => { (host.querySelector('[data-trigger="speakers"]') as HTMLButtonElement).click(); }); await settle();
    act(() => { (Array.from(host.querySelectorAll('button')).find(item => item.textContent === 'Edit speaker labels')!).click(); });
    expect(onEditSpeakers).toHaveBeenCalled();
    act(() => { host.querySelector('.danger')?.dispatchEvent(new MouseEvent('click', { bubbles: true })); });
    expect(onDelete).toHaveBeenCalledWith(note);
    expect(onSpeakerSettings).not.toHaveBeenCalled();
  });
});

test('settings routes to transcription options and tabs respond to keyboard', async () => {
  await render(<App/>); await settle();
  act(() => { (host.querySelector('.document-menu-trigger') as HTMLButtonElement).click(); }); await settle();
  act(() => { (host.querySelector('[data-trigger="speakers"]') as HTMLButtonElement).click(); }); await settle();
  act(() => { (Array.from(host.querySelectorAll('button')).find(item => item.textContent === 'Speaker settings…')!).click(); }); await settle();
  expect(host.querySelector('#settings-panel-speakers')?.hasAttribute('hidden')).toBe(false);
  const transcriptionTab = host.querySelector('#settings-tab-transcription')!;
  key(transcriptionTab, 'ArrowLeft'); await settle();
  expect(host.querySelector('#settings-tab-appearance')?.getAttribute('aria-selected')).toBe('true');
  expect(host.querySelector('#settings-panel-appearance')?.hasAttribute('hidden')).toBe(false);
  expect(document.activeElement?.id).toBe('settings-tab-appearance');
});

test('settings choices persist through an App remount', async () => {
  await render(<App/>); await settle();
  act(() => { (host.querySelector('.settings-trigger') as HTMLButtonElement).click(); }); await settle();
  act(() => { (host.querySelector('#settings-tab-transcription') as HTMLButtonElement).click(); }); await settle();
  const selects = host.querySelectorAll('select');
  act(() => {
    (selects[0] as HTMLSelectElement).value = 'uk';
    selects[0].dispatchEvent(new Event('change', { bubbles: true }));
    (selects[1] as HTMLSelectElement).value = '3';
    selects[1].dispatchEvent(new Event('change', { bubbles: true }));
    const checkbox = host.querySelector('input[type="checkbox"]') as HTMLInputElement;
    checkbox.click();
  });
  await settle();
  expect(readPreferences()).toEqual({ language: 'uk', autoSpeakers: false, speakerCount: 3 });

  await act(async () => { root.unmount(); });
  root = createRoot(host);
  await render(<App/>); await settle();
  act(() => { (host.querySelector('.settings-trigger') as HTMLButtonElement).click(); }); await settle();
  act(() => { (host.querySelector('#settings-tab-transcription') as HTMLButtonElement).click(); }); await settle();
  expect((host.querySelector('select') as HTMLSelectElement).value).toBe('uk');
});

test('speaker setup and delete actions open the expected dialogs with focus', async () => {
  mocks.api.setup.mockResolvedValue({ ...setup, diarizationReady: false });
  await render(<App/>); await settle();
  act(() => { (host.querySelector('.document-menu-trigger') as HTMLButtonElement).click(); }); await settle();
  act(() => { (host.querySelector('[data-trigger="speakers"]') as HTMLButtonElement).click(); }); await settle();
  act(() => { (Array.from(host.querySelectorAll('button')).find(item => item.textContent === 'Set up detection…')!).click(); }); await settle();
  expect(host.querySelector('#settings-panel-speakers')?.hasAttribute('hidden')).toBe(false);
  expect(document.activeElement?.getAttribute('aria-label')).toBe('Close settings');

  act(() => { (host.querySelector('.settings-close') as HTMLButtonElement).click(); }); await settle();
  act(() => { (host.querySelector('.document-menu-trigger') as HTMLButtonElement).click(); }); await settle();
  act(() => { (Array.from(host.querySelectorAll('[role="menuitem"]')).find(item => item.textContent?.includes('Delete recording')) as HTMLButtonElement).click(); }); await settle();
  expect(host.querySelector('.delete-dialog')?.hasAttribute('open')).toBe(true);
  expect(document.activeElement?.textContent).toBe('Keep recording');
});

test('unsaved edits disable switching, import, export, and delete actions', async () => {
  await render(<App/>); await settle();
  const transcript = host.querySelector('textarea[aria-label^="Transcript"]') as HTMLTextAreaElement;
  setTextarea(transcript, 'Changed words'); await settle();
  expect(host.textContent).toContain('Unsaved edits');
  expect((host.querySelector('.recording') as HTMLButtonElement).disabled).toBe(true);
  expect((host.querySelector('.import') as HTMLButtonElement).disabled).toBe(true);

  act(() => { (host.querySelector('.document-menu-trigger') as HTMLButtonElement).click(); }); await settle();
  expect((Array.from(host.querySelectorAll('[role="menuitem"]')).find(item => item.textContent?.includes('Delete recording')) as HTMLButtonElement).disabled).toBe(true);
  expect((host.querySelector('[data-trigger="export"]') as HTMLButtonElement).disabled).toBe(true);
});

async function openTranscriptionSettings() {
  act(() => { (host.querySelector('.settings-trigger') as HTMLButtonElement).click(); }); await settle();
  act(() => { (host.querySelector('#settings-tab-transcription') as HTMLButtonElement).click(); }); await settle();
}

test('no-model onboarding shows manual offline setup and downloads only after selection', async () => {
  mocks.api.setup.mockResolvedValue({ ...setup, modelReady: false, modelName: null });
  await render(<App/>); await settle();
  await openTranscriptionSettings();
  expect(host.textContent).toContain('No model selected');
  expect(Array.from(host.querySelectorAll('button')).some(button => button.textContent === 'Import model')).toBe(true);
  expect(host.textContent).toContain('For offline setup, transfer a compatible Whisper');
  expect(host.textContent).toContain('Hugging Face and its CDN only after you click a download button');
  expect(mocks.api.downloadModel).not.toHaveBeenCalled();
  const smallCard = Array.from(host.querySelectorAll('.model-download')).find(card => card.textContent?.includes('Small (quantized)'))!;
  expect(smallCard.textContent).toContain('190 MB');
  expect(smallCard.textContent).toContain('Higher quality potential.');
  act(() => { (smallCard.querySelector('button') as HTMLButtonElement).click(); }); await settle();
  expect(mocks.api.downloadModel).toHaveBeenCalledWith('small');
  const listenerIndex = mocks.listen.mock.calls.findIndex(([name]) => name === 'model-import-progress');
  const listenerOrder = mocks.listen.mock.invocationCallOrder[listenerIndex];
  const downloadOrder = mocks.api.downloadModel.mock.invocationCallOrder[0];
  expect(listenerOrder).toBeLessThan(downloadOrder);
});

test('download progress, validation cancel, settings-close cancel, and failure are visible', async () => {
  let rejectDownload!: (error: Error) => void;
  mocks.api.downloadModel.mockImplementation(() => new Promise((_, reject) => { rejectDownload = reject; }));
  await render(<App/>); await settle(); await openTranscriptionSettings();
  const smallCard = Array.from(host.querySelectorAll('.model-download')).find(card => card.textContent?.includes('Small (quantized)'))!;
  act(() => { (smallCard.querySelector('button') as HTMLButtonElement).click(); }); await settle();
  act(() => { mocks.modelProgressHandler({ payload: { phase: 'connecting', copiedBytes: 0, totalBytes: 5_000_000 } }); }); await settle();
  expect(host.textContent).toContain('Connecting to model host');
  act(() => { mocks.modelProgressHandler({ payload: { phase: 'downloading', copiedBytes: 2_000_000, totalBytes: 5_000_000 } }); }); await settle();
  expect(host.textContent).toContain('Downloading model… 2 MB of 5 MB');
  expect(host.querySelector('.model-operation-status progress')).not.toBeNull();
  act(() => { mocks.modelProgressHandler({ payload: { phase: 'validating', copiedBytes: 5_000_000, totalBytes: 5_000_000 } }); }); await settle();
  expect(host.textContent).toContain('Cancellation takes effect after the current check');
  expect(Array.from(host.querySelectorAll('.model-operation-status button')).some(button => button.textContent === 'Cancel after check')).toBe(true);

  act(() => { (host.querySelector('#settings-tab-appearance') as HTMLButtonElement).click(); }); await settle();
  expect(host.querySelector('.settings-feedback .model-operation-status button')).not.toBeNull();

  act(() => { (host.querySelector('.settings-close') as HTMLButtonElement).click(); }); await settle();
  expect(host.textContent).toContain('Checking and preparing model…');
  act(() => { (Array.from(host.querySelectorAll('.model-operation-status button')).find(button => button.textContent === 'Cancel after check') as HTMLButtonElement).click(); }); await settle();
  expect(mocks.api.cancel).toHaveBeenCalled();
  expect(host.textContent).toContain('Cancellation requested…');
  await act(async () => { rejectDownload(new Error('Model download cancelled. Your previous model is unchanged.')); await Promise.resolve(); });
  await settle();
  expect(host.textContent).toContain('Your previous model is unchanged.');
});

test('manual model import reports copy and validation progress', async () => {
  let finishImport!: (result: boolean) => void;
  const unlisten = vi.fn();
  mocks.api.importModel.mockImplementation(() => new Promise(resolve => { finishImport = resolve; }));
  mocks.listen.mockImplementation(async (name, handler) => {
    if (name === 'model-import-progress') mocks.modelProgressHandler.mockImplementation(handler);
    return name === 'model-import-progress' ? unlisten : vi.fn();
  });
  await render(<App/>); await settle(); await openTranscriptionSettings();
  act(() => { (Array.from(host.querySelectorAll('button')).find(button => button.textContent === 'Replace model') as HTMLButtonElement).click(); }); await settle();
  expect(mocks.api.importModel).toHaveBeenCalled();
  act(() => { mocks.modelProgressHandler({ payload: { phase: 'copying', copiedBytes: 1_000_000, totalBytes: 4_000_000 } }); }); await settle();
  expect(host.textContent).toContain('Copying model file… 1 MB of 4 MB');
  act(() => { mocks.modelProgressHandler({ payload: { phase: 'validating', copiedBytes: 4_000_000, totalBytes: 4_000_000 } }); }); await settle();
  expect(host.textContent).toContain('Checking and preparing model');
  await act(async () => { finishImport(true); await Promise.resolve(); }); await settle();
  expect(host.textContent).toContain('Model imported. You’re ready to transcribe offline.');
  expect(unlisten).toHaveBeenCalledOnce();
});

test('progress listener failure prevents model import from starting', async () => {
  mocks.listen.mockImplementation(async (name) => {
    if (name === 'model-import-progress') throw new Error('Progress listener unavailable');
    return vi.fn();
  });
  await render(<App/>); await settle(); await openTranscriptionSettings();
  act(() => { (Array.from(host.querySelectorAll('button')).find(button => button.textContent === 'Replace model') as HTMLButtonElement).click(); });
  await settle();
  expect(mocks.api.importModel).not.toHaveBeenCalled();
  expect(host.textContent).toContain('Progress listener unavailable');
});

test('catalog failure does not block the saved library or manual import', async () => {
  mocks.api.modelCatalog.mockRejectedValue(new Error('Catalog unavailable'));
  await render(<App/>); await settle();
  expect(host.textContent).toContain('Interview');
  await openTranscriptionSettings();
  expect(host.textContent).toContain('Model downloads are unavailable: Catalog unavailable');
  expect(Array.from(host.querySelectorAll('button')).some(button => button.textContent === 'Replace model')).toBe(true);
});

test('an in-flight model listener is released exactly once when App unmounts', async () => {
  let finishDownload!: () => void;
  mocks.api.downloadModel.mockImplementation(() => new Promise<void>(resolve => { finishDownload = resolve; }));
  await render(<App/>); await settle(); await openTranscriptionSettings();
  act(() => { (host.querySelector('.model-download button') as HTMLButtonElement).click(); }); await settle();
  const listenerIndex = mocks.listen.mock.calls.findIndex(([name]) => name === 'model-import-progress');
  const unlisten = await mocks.listen.mock.results[listenerIndex].value;
  await act(async () => { root.unmount(); });
  expect(unlisten).toHaveBeenCalledTimes(1);
  await act(async () => { finishDownload(); await Promise.resolve(); }); await settle();
  expect(unlisten).toHaveBeenCalledTimes(1);
  root = createRoot(host);
});
