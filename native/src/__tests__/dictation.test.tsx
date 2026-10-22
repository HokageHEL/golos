// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import App from '../App';
import Overlay from '../Overlay';
import DictationSettings from '../DictationSettings';
import type { DictationStatus } from '../api';

const fixture = vi.hoisted(() => {
  const segment = { startMs: 0, endMs: 1000, speaker: 'Alice', text: 'Original saved text' };
  const notes = ['One', 'Two'].map((title, index) => ({ id: String(index), title, createdAt: 1, durationSeconds: 1, status: 'done', segments: [segment], error: null, folder: '', tags: [] }));
  return {
    notes,
    status: null as DictationStatus | null,
    listener: null as null | ((event: { payload: DictationStatus }) => void),
    inserts: [] as number[],
    retries: [] as string[],
    permissions: [] as string[],
    copied: [] as string[],
  };
});

const baseStatus = (overrides: Partial<DictationStatus> = {}): DictationStatus => ({
  enabled: true, shortcut: 'CmdOrCtrl+Shift+Space', shortcutDisplay: '⇧⌘Space', mode: 'toggle',
  language: 'auto', phase: 'idle', active: false, overlayVisible: false, showOverlay: true,
  overlayAlwaysOnTop: true, restoreClipboard: true, aiEnabled: false, aiKind: 'cleanup',
  aiPrompt: '', appOverrides: [], shortcutError: null, startupFailed: false,
  insertionSupported: true, permissions: { accessibility: false, postEvents: false },
  target: null, noteId: null, originalText: '', processedText: null, previewText: '',
  processedModel: null, processedPrompt: null, error: null, warning: null, pasteMessage: null,
  clipboard: null, recovered: false, canStart: false, canPause: false, canDiscard: false,
  canRetrySave: false, canRetryTranscribe: false, canRetryProcess: false, canInsert: false,
  canDismiss: false,
  ...overrides,
});

vi.mock('../api', () => ({
  nativeAvailable: true,
  dictationPhaseLabel: {
    idle: 'Idle', starting: 'Starting', recording: 'Recording', paused: 'Paused', stopped: 'Stopped',
    saving: 'Saving', saveFailed: 'Save failed', transcribing: 'Transcribing', processing: 'Processing',
    preview: 'Preview', inserting: 'Inserting', pasteRequested: 'Paste requested', failed: 'Failed',
  },
  api: {
    automation: async () => ({ label: null, settings: { paused: false, exportEnabled: false, exportDirectory: null, exportFormats: ['txt'] }, folders: [], files: [], exports: [] }),
    languages: async () => [{ code: 'en', name: 'english' }],
    setup: async () => ({ modelReady: true, modelName: 'Small', diarizationReady: false }),
    list: async ({ search = '' } = {}) => fixture.notes.filter(n => n.title.toLowerCase().includes(search.toLowerCase())),
    get: async (id: string) => fixture.notes.find(n => n.id === id),
    audioSource: async () => '/audio.wav',
    captureInfo: async () => ({ system: true, mixed: true }),
    startRecording: async () => {},
    recordingStatus: async () => ({ elapsedSeconds: 1, stopped: false, paused: false, error: null }),
    pauseRecording: async () => {},
    stopRecording: async () => null,
    save: async () => {},
    transcribe: async () => {},
    export: async () => true,
    exportSaved: async () => true,
    downloadModels: async () => {}, cancel: async () => {},
    dictationStatus: async () => fixture.status ?? baseStatus(),
    dictationSettings: async () => ({ enabled: false, shortcut: '', mode: 'toggle', language: 'auto', showOverlay: true, overlayAlwaysOnTop: true, restoreClipboard: true, aiEnabled: false, aiKind: 'cleanup', aiPrompt: '', appOverrides: [] }),
    dictationConfigure: async (settings: unknown) => settings,
    dictationStart: async () => baseStatus({ phase: 'recording', active: true }),
    dictationStop: async () => baseStatus({ phase: 'saving', active: true }),
    dictationPause: async () => baseStatus({ phase: 'paused', active: true, canPause: true }),
    dictationRetrySave: async () => { fixture.retries.push('save'); return baseStatus({ phase: 'preview', noteId: 'dictated', originalText: 'Dictated words.', previewText: 'Dictated words.', canInsert: true }); },
    dictationRetryTranscribe: async () => { fixture.retries.push('transcribe'); return baseStatus({ phase: 'preview', noteId: 'dictated', originalText: 'Dictated words.', previewText: 'Dictated words.', canInsert: true }); },
    dictationRetryProcess: async () => { fixture.retries.push('process'); return baseStatus({ phase: 'preview', noteId: 'dictated', originalText: 'Dictated words.', previewText: 'Dictated words.', canInsert: true }); },
    dictationPreviewText: async () => baseStatus(),
    dictationInsert: async () => { fixture.inserts.push(1); return { status: 'pasteRequested', clipboard: 'unchanged', message: 'Paste requested; verify the target app.' }; },
    dictationDismiss: async () => baseStatus(),
    dictationDiscard: async () => baseStatus(),
    dictationPermissions: async () => { fixture.permissions.push('check'); return { accessibility: true, postEvents: true }; },
    dictationRequestPermission: async (kind: string) => { fixture.permissions.push(kind); return { accessibility: true, postEvents: true }; },
    dictationOpenPermissionSettings: async () => { fixture.permissions.push('settings'); },
    dictationHideOverlay: async () => {},
  },
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: async (name: string, callback: (event: { payload: unknown }) => void) => {
    if (name === 'dictation-state') fixture.listener = callback as (event: { payload: DictationStatus }) => void;
    return () => {};
  },
}));
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onCloseRequested: async () => () => {} }) }));
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc: (path: string) => path }));

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} });
  window.matchMedia = vi.fn().mockReturnValue({ matches: false, addEventListener() {}, removeEventListener() {} });
  Element.prototype.scrollIntoView = vi.fn();
  HTMLMediaElement.prototype.pause = vi.fn();
  HTMLMediaElement.prototype.load = vi.fn();
  HTMLDialogElement.prototype.showModal = vi.fn(function (this: HTMLDialogElement) { this.open = true; });
  HTMLDialogElement.prototype.close = vi.fn(function (this: HTMLDialogElement) { this.open = false; });
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async (text: string) => { fixture.copied.push(text); } } });
  fixture.status = null;
  fixture.listener = null;
  fixture.inserts = [];
  fixture.retries = [];
  fixture.permissions = [];
  fixture.copied = [];
});
afterEach(cleanup);

it('the overlay offers recording controls and never an editable text field', async () => {
  fixture.status = baseStatus({ phase: 'recording', active: true, canPause: true, canDiscard: true, target: { bundleId: 'com.example.Notes', name: 'Notes' } });
  render(<Overlay/>);
  await screen.findByRole('group', { name: 'Global dictation' });
  expect(screen.getByRole('button', { name: 'Stop and save' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Pause' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Discard…' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Hide' })).toBeTruthy();
  expect(screen.queryByRole('textbox')).toBeNull();
  expect(screen.getByText(/cannot take keyboard focus/)).toBeTruthy();
});

it('the overlay asks before it discards a capture', async () => {
  fixture.status = baseStatus({ phase: 'recording', active: true, canPause: true, canDiscard: true });
  render(<Overlay/>);
  fireEvent.click(await screen.findByRole('button', { name: 'Discard…' }));
  expect(screen.getByRole('button', { name: 'Keep recording' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Discard recording' })).toBeTruthy();
});

it('the review panel retries a failed save and inserts only on request', async () => {
  fixture.status = baseStatus({ phase: 'saveFailed', active: true, canRetrySave: true, noteId: 'dictated' });
  render(<App/>);
  await screen.findByRole('region', { name: 'Global dictation' });
  fireEvent.click(screen.getByRole('button', { name: 'Retry save' }));
  await waitFor(() => expect(fixture.retries).toEqual(['save']));
  await waitFor(() => expect((screen.getByRole('button', { name: 'Insert into target' }) as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByRole('button', { name: 'Insert into target' }));
  await waitFor(() => expect(fixture.inserts).toHaveLength(1));
  expect(await screen.findByText('Paste requested; verify the target app.')).toBeTruthy();
});

it('the review panel offers explicit permission recovery and a copy fallback', async () => {
  fixture.status = baseStatus({ phase: 'preview', noteId: 'dictated', originalText: 'Dictated words.', previewText: 'Dictated words.', canInsert: false });
  render(<App/>);
  await screen.findByRole('region', { name: 'Global dictation' });
  expect(screen.getByRole('button', { name: 'Request access' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Open Settings' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Check again' })).toBeTruthy();
  expect((screen.getByRole('button', { name: 'Insert into target' }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole('button', { name: 'Copy text' }));
  await waitFor(() => expect(fixture.copied).toEqual(['Dictated words.']));
});

it('a background dictation keeps the selected recording and the unsaved draft', async () => {
  fixture.status = baseStatus();
  render(<App/>);
  const editor = await screen.findByRole('textbox', { name: 'Transcript at 0:00' });
  fireEvent.change(editor, { target: { value: 'My unsaved draft' } });
  await waitFor(() => expect(fixture.listener).not.toBeNull());
  act(() => {
    fixture.listener?.({ payload: baseStatus({ phase: 'recording', active: true, canPause: true }) });
  });
  expect(await screen.findByText(/Global dictation is running/)).toBeTruthy();
  expect((screen.getByRole('textbox', { name: 'Transcript at 0:00' }) as HTMLTextAreaElement).value).toBe('My unsaved draft');
  // A new dictation note never replaces the current selection.
  act(() => {
    fixture.listener?.({ payload: baseStatus({ phase: 'preview', noteId: 'dictated', originalText: 'Dictated words.', previewText: 'Dictated words.', canInsert: true }) });
  });
  expect((screen.getByRole('textbox', { name: 'Transcript at 0:00' }) as HTMLTextAreaElement).value).toBe('My unsaved draft');
  expect(screen.getByRole('heading', { name: 'One' })).toBeTruthy();
});


it('keeps the per-app override input mounted and requests both insertion permissions explicitly', async () => {
  render(<DictationSettings open status={baseStatus()} onStatus={() => {}} onError={() => {}} nativeAvailable onClose={() => {}}/>);
  await waitFor(() => expect(screen.getByRole('textbox', { name: 'Dictation shortcut' })).toBeTruthy());
  fireEvent.click(screen.getByText('Local processing (optional)', { selector: 'summary' }));
  fireEvent.click(screen.getByRole('button', { name: 'Add app override' }));
  const bundle = screen.getByRole('textbox', { name: 'Bundle ID for override 1' });
  bundle.focus();
  fireEvent.change(bundle, { target: { value: 'com.example' } });
  expect(screen.getByRole('textbox', { name: 'Bundle ID for override 1' })).toBe(bundle);
  expect(document.activeElement).toBe(bundle);
  expect(fixture.permissions).toEqual([]);
  fireEvent.click(screen.getByText('Text insertion permissions', { selector: 'summary' }));
  fireEvent.click(screen.getByRole('button', { name: 'Request Accessibility access' }));
  await waitFor(() => expect(fixture.permissions).toEqual(['accessibility']));
  fireEvent.click(screen.getByRole('button', { name: 'Request keyboard-event access' }));
  await waitFor(() => expect(fixture.permissions).toEqual(['accessibility', 'postEvents']));
});
