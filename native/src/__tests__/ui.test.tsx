// @vitest-environment jsdom
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import type { Note, Setup } from '../api';

const mocks = vi.hoisted(() => ({
  api: {
    setup: vi.fn(), list: vi.fn(), audioSource: vi.fn(), save: vi.fn(), delete: vi.fn(), export: vi.fn(),
    cancel: vi.fn(), detectSpeakers: vi.fn(), transcribe: vi.fn(), importAudio: vi.fn(), importModel: vi.fn(),
    importSpeakerModels: vi.fn(),
  },
  listen: vi.fn(), onCloseRequested: vi.fn(),
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
  mocks.listen.mockResolvedValue(vi.fn());
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
