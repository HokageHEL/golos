// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import App from '../App';

const fixture = vi.hoisted(() => {
  const segment = { startMs: 0, endMs: 1000, speaker: 'Alice', text: 'Original saved text' };
  const notes = ['One', 'Two'].map((title, index) => ({ id: String(index), title, createdAt: 1, durationSeconds: 1, status: 'done', segments: [segment], error: null, folder: '', tags: [] }));
  return { notes, capture: { elapsedSeconds: 1, stopped: false, paused: false, error: null }, saves: vi.fn(), exports: vi.fn(), source: vi.fn(), download: vi.fn(), cancel: vi.fn(), progressListener: null as null | ((event: { payload: { received: number; total: number; phase: string } }) => void) };
});
vi.mock('../api', () => ({
  nativeAvailable: true,
  api: {
    setup: async () => ({ modelReady: true, modelName: 'Small', diarizationReady: false }),
    list: async ({ search = '' } = {}) => fixture.notes.filter(n => n.title.toLowerCase().includes(search.toLowerCase())),
    get: async (id: string) => fixture.notes.find(n => n.id === id),
    audioSource: async () => '/audio.wav',
    captureInfo: async () => ({ system: true, mixed: true }),
    startRecording: fixture.source,
    recordingStatus: async () => ({ ...fixture.capture }),
    pauseRecording: async (paused: boolean) => { fixture.capture.paused = paused; },
    stopRecording: async () => null,
    save: fixture.saves,
    export: fixture.exports,
    exportSaved: fixture.exports,
    downloadModels: fixture.download, cancel: fixture.cancel,
  },
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: async (name: string, callback: typeof fixture.progressListener) => { if (name === 'model-download') fixture.progressListener = callback; return () => {}; } }));
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onCloseRequested: async () => () => {} }) }));
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc: (path: string) => path }));
beforeEach(() => {
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} });
  window.matchMedia = vi.fn().mockReturnValue({ matches: false, addEventListener() {}, removeEventListener() {} });
  HTMLMediaElement.prototype.pause = vi.fn();
  HTMLMediaElement.prototype.load = vi.fn();
  HTMLDialogElement.prototype.showModal = vi.fn(function (this: HTMLDialogElement) { this.open = true; });
  HTMLDialogElement.prototype.close = vi.fn(function (this: HTMLDialogElement) { this.open = false; });
  fixture.capture.paused = false;
  fixture.saves.mockClear(); fixture.source.mockClear(); fixture.download.mockReset(); fixture.cancel.mockReset(); fixture.progressListener = null;
});
afterEach(cleanup);

it('retains edits across search and navigation, and saves the restored draft', async () => {
  render(<App/>);
  const editor = await screen.findByRole('textbox', { name: 'Transcript at 0:00' });
  fireEvent.change(editor, { target: { value: 'My unsaved draft' } });
  fireEvent.change(screen.getByRole('searchbox', { name: 'Search library' }), { target: { value: 'Two' } });
  await waitFor(() => expect(screen.queryByRole('button', { name: /One.*Transcribed/ })).toBeNull());
  expect((screen.getByRole('textbox', { name: 'Transcript at 0:00' }) as HTMLTextAreaElement).value).toBe('My unsaved draft');
  fireEvent.click(screen.getByRole('button', { name: /Two.*Transcribed/ }));
  await waitFor(() => expect((screen.getByRole('textbox', { name: 'Transcript at 0:00' }) as HTMLTextAreaElement).value).toBe('Original saved text'));
  fireEvent.change(screen.getByRole('searchbox', { name: 'Search library' }), { target: { value: '' } });
  fireEvent.click(await screen.findByRole('button', { name: /One.*Transcribed/ }));
  await waitFor(() => expect((screen.getByRole('textbox', { name: 'Transcript at 0:00' }) as HTMLTextAreaElement).value).toBe('My unsaved draft'));
  fireEvent.click(screen.getByRole('button', { name: 'Save edits' }));
  await waitFor(() => expect(fixture.saves).toHaveBeenCalledWith('0', [expect.objectContaining({ text: 'My unsaved draft', speaker: 'Alice' })]));
});

it('shows pause/resume and protects discard, with the chosen source passed to native capture', async () => {
  render(<App/>);
  await screen.findByRole('textbox', { name: 'Transcript at 0:00' });
  fireEvent.change(screen.getByRole('combobox', { name: 'Recording source' }), { target: { value: 'mixed' } });
  fireEvent.click(screen.getByRole('button', { name: '● Record audio' }));
  fireEvent.click(await screen.findByRole('button', { name: 'Pause' }));
  await screen.findByRole('heading', { name: 'Recording paused' });
  expect(fixture.source).toHaveBeenCalledWith(expect.any(String), 'mixed', 0);
  fireEvent.click(screen.getByRole('button', { name: 'Resume' }));
  await screen.findByRole('heading', { name: 'Recording microphone + application audio' });
  fireEvent.click(screen.getByRole('button', { name: 'Discard…' }));
  expect(screen.getByRole('group', { name: 'Discard this recording?' })).toBeTruthy();
});

it('supports undo/redo of text and keeps saved exports disabled for unsaved edits', async () => {
  render(<App/>);
  const editor = await screen.findByRole('textbox', { name: 'Transcript at 0:00' });
  fireEvent.change(editor, { target: { value: 'Changed' } });
  expect((screen.getByRole('button', { name: 'Export DOCX' }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
  expect((editor as HTMLTextAreaElement).value).toBe('Original saved text');
  fireEvent.click(screen.getByRole('button', { name: 'Redo' }));
  expect((editor as HTMLTextAreaElement).value).toBe('Changed');
});

it('shows model download progress and cancellation, then retries successfully', async () => {
  let rejectDownload: (reason: Error) => void = () => {};
  fixture.download.mockImplementationOnce(() => new Promise((_, reject) => { rejectDownload = reject; })).mockResolvedValueOnce(undefined);
  fixture.cancel.mockImplementation(async () => { rejectDownload(new Error('Download cancelled. Installed model unchanged.')); });
  render(<App/>);
  await screen.findByRole('textbox', { name: 'Transcript at 0:00' });
  fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
  fireEvent.click(screen.getByRole('button', { name: 'Download recommended model' }));
  await waitFor(() => expect(fixture.download).toHaveBeenCalledWith(false));
  act(() => { fixture.progressListener?.({ payload: { received: 1000000, total: 190085487, phase: 'Downloading Whisper Small' } }); });
  expect(screen.getByText(/Downloading Whisper Small · 1.0/)).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Cancel download' }));
  await screen.findByText('Download cancelled. Installed model unchanged.');
  fireEvent.click(screen.getByRole('button', { name: 'Retry recommended model' }));
  await screen.findByText('Models installed. Recording and transcription are ready offline.');
  expect(fixture.download).toHaveBeenCalledTimes(2);
});
