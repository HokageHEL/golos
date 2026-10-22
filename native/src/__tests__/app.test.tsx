// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import App from '../App';

const fixture = vi.hoisted(() => {
  const segment = { startMs: 0, endMs: 1000, speaker: 'Alice', text: 'Original saved text' };
  const notes = ['One', 'Two'].map((title, index) => ({ id: String(index), title, createdAt: 1, durationSeconds: 1, status: 'done', segments: [segment], error: null, folder: '', tags: [] }));
  return { notes, capture: { elapsedSeconds: 1, stopped: false, paused: false, error: null }, saves: vi.fn(), transcribes: vi.fn(), exports: vi.fn(), source: vi.fn(), download: vi.fn(), cancel: vi.fn(), automationListener: null as null | ((event: { payload: string | null }) => void), progressListener: null as null | ((event: { payload: { received: number; total: number; phase: string } }) => void) };
});
vi.mock('../api', () => ({
  nativeAvailable: true,
  api: {
    automation: async () => ({ label: null, settings: { paused: false, exportEnabled: false, exportDirectory: null, exportFormats: ['txt'] }, folders: [], files: [], exports: [] }),
    languages: async () => [{ code: 'en', name: 'english' }, { code: 'uk', name: 'ukrainian' }, { code: 'ja', name: 'japanese' }],
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
    transcribe: fixture.transcribes,
    export: fixture.exports,
    exportSaved: fixture.exports,
    downloadModels: fixture.download, cancel: fixture.cancel,
    pauseAutomation: async () => {},
    aiAutomation: async () => ({ rules: [], tasks: [], model: '', label: null }),
    aiConfiguration: async () => ({ settings: { port: 11434, model: '', contextWindow: 8192 }, results: [] }),
  },
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: async (name: string, callback: typeof fixture.progressListener) => { if (name === 'model-download') fixture.progressListener = callback; if (name === 'automation-state') fixture.automationListener = callback as unknown as typeof fixture.automationListener; return () => {}; } }));
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
  fixture.capture.paused = false;
  fixture.saves.mockClear(); fixture.source.mockClear(); fixture.download.mockReset(); fixture.cancel.mockReset(); fixture.progressListener = null; fixture.automationListener = null;
});
afterEach(cleanup);

/** Opens or closes one in-flow disclosure by its summary text. */
function toggleSection(label: string) {
  fireEvent.click(screen.getByText(label, { selector: 'summary' }));
}
function section(label: string) {
  return screen.getByText(label, { selector: 'summary' }).closest('details') as HTMLDetailsElement;
}

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
  const cachedRecording = await screen.findByRole('button', { name: /One.*Transcribed/ });
  fireEvent.contextMenu(cachedRecording);
  fireEvent.click(screen.getByRole('menuitem', { name: 'Delete recording…' }));
  expect(screen.getByText('Open this recording and save or discard its draft before deleting it.')).toBeTruthy();
  expect(screen.queryByRole('dialog')).toBeNull();
  fireEvent.click(cachedRecording);
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
  toggleSection('Export');
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
  fireEvent.click(screen.getByRole('button', { name: /Transcription Offline model/ }));
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


it('exports structured formats and copies the current draft without silently saving it', async () => {
  fixture.exports.mockResolvedValue(true);
  const copy = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: copy } });
  render(<App/>);
  const editor = await screen.findByRole('textbox', { name: 'Transcript at 0:00' });
  toggleSection('Export');
  fireEvent.change(screen.getByRole('combobox', { name: 'Other export format' }), { target: { value: 'json' } });
  fireEvent.click(screen.getByRole('button', { name: 'Export JSON' }));
  await waitFor(() => expect(fixture.exports).toHaveBeenCalledWith('json', expect.stringContaining('"timestampUnit": "milliseconds"')));
  fireEvent.change(editor, { target: { value: 'Private unsaved revision' } });
  expect((screen.getByRole('button', { name: 'Export JSON' }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole('button', { name: 'Copy transcript' }));
  await waitFor(() => expect(copy).toHaveBeenCalledWith('Alice: Private unsaved revision'));
  expect(fixture.saves).not.toHaveBeenCalled();
  expect(screen.getByText('Draft copied to clipboard. Your edits are still unsaved.')).toBeTruthy();
});


it('applies explicit manual export layout and DOCX options without saving the draft', async () => {
  fixture.exports.mockClear();
  fixture.exports.mockResolvedValue(true);
  render(<App/>);
  await screen.findByRole('textbox', { name: 'Transcript at 0:00' });
  fireEvent.click(screen.getByRole('button', { name: 'Export DOCX' }));
  await waitFor(() => expect(fixture.exports).toHaveBeenCalledWith('0', 'docx'));
  fixture.exports.mockClear();
  fireEvent.click(screen.getByText('Export options'));
  fireEvent.click(screen.getByRole('checkbox', { name: 'Customize export layout' }));
  fireEvent.click(screen.getByRole('checkbox', { name: 'Include timestamps' }));
  fireEvent.click(screen.getByRole('checkbox', { name: 'Include speaker labels' }));
  fireEvent.change(screen.getByRole('combobox', { name: 'Paragraph grouping' }), { target: { value: 'same-speaker' } });
  fireEvent.click(screen.getByRole('button', { name: 'Export TXT' }));
  await waitFor(() => expect(fixture.exports).toHaveBeenCalledWith('txt', 'Original saved text'));
  fireEvent.click(screen.getByRole('button', { name: 'Export DOCX' }));
  await waitFor(() => expect(fixture.exports).toHaveBeenCalledWith('0', 'docx', {
    includeTimestamps: false,
    includeSpeakers: false,
    grouping: 'same-speaker',
  }));
  expect(fixture.saves).not.toHaveBeenCalled();
});

it('rejects an unsafe subtitle offset in the manual flow with a clear error', async () => {
  fixture.exports.mockClear();
  render(<App/>);
  await screen.findByRole('textbox', { name: 'Transcript at 0:00' });
  fireEvent.click(screen.getByText('Export options'));
  fireEvent.click(screen.getByRole('checkbox', { name: 'Customize export layout' }));
  fireEvent.change(screen.getByRole('textbox', { name: 'Subtitle timestamp offset' }), { target: { value: '-2000' } });
  fireEvent.click(screen.getByRole('button', { name: 'Export SRT' }));
  await screen.findByText(/negative cue times/);
  expect(fixture.exports).not.toHaveBeenCalled();
  expect(fixture.saves).not.toHaveBeenCalled();
});


it('offers engine languages beyond English and Ukrainian and forwards the selected code', async () => {
  const previousStatus = fixture.notes[0].status;
  fixture.notes[0].status = 'ready';
  fixture.transcribes.mockResolvedValue(undefined);
  try {
    render(<App/>);
    const language = await screen.findByRole('combobox', { name: 'Audio language' });
    await screen.findByRole('option', { name: 'Japanese' });
    fireEvent.change(language, { target: { value: 'ja' } });
    fireEvent.click(screen.getByRole('button', { name: 'Transcribe offline' }));
    await waitFor(() => expect(fixture.transcribes).toHaveBeenCalledWith('0', 'ja'));
    await waitFor(() => expect((screen.getByRole('button', { name: 'Transcribe offline' }) as HTMLButtonElement).disabled).toBe(false));
  } finally {
    fixture.notes[0].status = previousStatus;
  }
});


it('keeps draft editing usable while native automation runs and restores saving afterward',async()=>{
  render(<App/>);const editor=await screen.findByRole('textbox',{name:'Transcript at 0:00'});
  await act(async()=>fixture.automationListener?.({payload:'Transcribing watched audio'}));
  expect((editor as HTMLTextAreaElement).disabled).toBe(false);
  fireEvent.change(editor,{target:{value:'A draft edited during background transcription'}});
  expect((screen.getByRole('button',{name:'Save edits'}) as HTMLButtonElement).disabled).toBe(true);
  await act(async()=>fixture.automationListener?.({payload:null}));
  expect((screen.getByRole('button',{name:'Save edits'}) as HTMLButtonElement).disabled).toBe(false);
  expect((editor as HTMLTextAreaElement).value).toBe('A draft edited during background transcription');
});

it('groups every export in one disclosure and keeps each export handler working', async () => {
  fixture.exports.mockResolvedValue(true);
  render(<App/>);
  await screen.findByRole('textbox', { name: 'Transcript at 0:00' });
  // Secondary exports stay collapsed until their section is opened.
  expect(section('Export').open).toBe(false);
  toggleSection('Export');
  expect(section('Export').open).toBe(true);
  fireEvent.click(screen.getByRole('button', { name: 'Export TXT' }));
  await waitFor(() => expect(fixture.exports).toHaveBeenCalledWith('txt', expect.stringContaining('Original saved text')));
  fireEvent.click(screen.getByRole('button', { name: 'Export audio' }));
  await waitFor(() => expect(fixture.exports).toHaveBeenCalledWith('0', 'wav'));
  fireEvent.click(screen.getByRole('button', { name: 'Export project backup' }));
  await waitFor(() => expect(fixture.exports).toHaveBeenCalledWith('0', 'golos'));
  expect(screen.getByText('Exported.')).toBeTruthy();
});

it('keeps the find & replace draft when the section is closed and reopened', async () => {
  render(<App/>);
  await screen.findByRole('textbox', { name: 'Transcript at 0:00' });
  toggleSection('Find & replace');
  fireEvent.change(screen.getByRole('searchbox', { name: 'Find in transcript' }), { target: { value: 'saved' } });
  fireEvent.change(screen.getByRole('textbox', { name: 'Replacement text' }), { target: { value: 'reviewed' } });
  toggleSection('Find & replace');
  expect(section('Find & replace').open).toBe(false);
  toggleSection('Find & replace');
  expect(section('Find & replace').open).toBe(true);
  expect((screen.getByRole('searchbox', { name: 'Find in transcript' }) as HTMLInputElement).value).toBe('saved');
  expect((screen.getByRole('textbox', { name: 'Replacement text' }) as HTMLInputElement).value).toBe('reviewed');
});

it('keeps the speaker choice when the speaker section is closed and reopened', async () => {
  render(<App/>);
  await screen.findByRole('textbox', { name: 'Transcript at 0:00' });
  toggleSection('Speaker detection');
  fireEvent.change(screen.getByRole('combobox', { name: 'Speakers' }), { target: { value: '3' } });
  toggleSection('Speaker detection');
  expect(section('Speaker detection').open).toBe(false);
  toggleSection('Speaker detection');
  expect((screen.getByRole('combobox', { name: 'Speakers' }) as HTMLSelectElement).value).toBe('3');
});

it('reveals system audio permissions as soon as a system source is chosen', async () => {
  render(<App/>);
  await screen.findByRole('textbox', { name: 'Transcript at 0:00' });
  expect(screen.queryByText('System audio & permissions', { selector: 'summary' })).toBeNull();
  fireEvent.change(screen.getByRole('combobox', { name: 'Recording source' }), { target: { value: 'system' } });
  expect(section('System audio & permissions').open).toBe(true);
  expect(screen.getByRole('button', { name: 'Refresh applications / grant access' })).toBeTruthy();
  expect(screen.getByText(/Allow Golos Audio Capture in System Settings/)).toBeTruthy();
  expect(screen.getByRole('combobox', { name: 'Application to capture' })).toBeTruthy();
  // A different system source brings the permission steps back into view.
  toggleSection('System audio & permissions');
  expect(section('System audio & permissions').open).toBe(false);
  fireEvent.change(screen.getByRole('combobox', { name: 'Recording source' }), { target: { value: 'mixed' } });
  expect(section('System audio & permissions').open).toBe(true);
});

it('opens recording actions inside the library row and closes them with Escape', async () => {
  render(<App/>);
  const trigger = await screen.findByRole('button', { name: 'Actions for One' });
  fireEvent.click(trigger);
  const menu = screen.getByRole('menu');
  // The menu expands in the row, so it cannot float over the player or editor.
  expect(menu.closest('.recording-row')).toBeTruthy();
  expect(menu.className).toBe('recording-menu');
  expect(trigger.getAttribute('aria-expanded')).toBe('true');
  fireEvent.keyDown(document, { key: 'Escape' });
  await waitFor(() => expect(screen.queryByRole('menu')).toBeNull());
  expect(document.activeElement).toBe(trigger);
});

it('lets the find field clear its own query with Escape and keeps the section open', async () => {
  render(<App/>);
  await screen.findByRole('textbox', { name: 'Transcript at 0:00' });
  toggleSection('Find & replace');
  const field = screen.getByRole('searchbox', { name: 'Find in transcript' }) as HTMLInputElement;
  fireEvent.change(field, { target: { value: 'saved' } });
  expect(field.value).toBe('saved');
  fireEvent.keyDown(field, { key: 'Escape' });
  await waitFor(() => expect(field.value).toBe(''));
  // The field handled the key, so the section around it stays open.
  expect(section('Find & replace').open).toBe(true);
});

it('shows the applied filters and the match count while the sections are collapsed', async () => {
  render(<App/>);
  await screen.findByRole('textbox', { name: 'Transcript at 0:00' });
  const filters = screen.getByText('Filters', { selector: 'summary' });
  expect(filters.getAttribute('data-state')).toBeNull();
  fireEvent.change(screen.getByRole('combobox', { name: 'Recording status' }), { target: { value: 'done' } });
  fireEvent.change(screen.getByLabelText('Filter folder'), { target: { value: 'Work' } });
  await waitFor(() => expect(filters.getAttribute('data-state')).toBe('Transcribed · Work'));
  toggleSection('Find & replace');
  const find = screen.getByText('Find & replace', { selector: 'summary' });
  fireEvent.change(screen.getByRole('searchbox', { name: 'Find in transcript' }), { target: { value: 'saved' } });
  await waitFor(() => expect(find.getAttribute('data-state')).toBe('1 match'));
  toggleSection('Find & replace');
  // The count stays visible while the section is closed.
  expect(section('Find & replace').open).toBe(false);
  expect(find.getAttribute('data-state')).toBe('1 match');
});

it('returns focus to the trigger when the settings dialog closes', async () => {
  render(<App/>);
  await screen.findByRole('textbox', { name: 'Transcript at 0:00' });
  const trigger = screen.getByRole('button', { name: 'Settings' });
  fireEvent.click(trigger);
  await screen.findByRole('dialog');
  fireEvent.click(screen.getByRole('button', { name: 'Close settings' }));
  await waitFor(() => expect(document.activeElement).toBe(trigger));
});

it('returns focus to the trigger when the folders and automation dialog closes', async () => {
  render(<App/>);
  await screen.findByRole('textbox', { name: 'Transcript at 0:00' });
  toggleSection('More tools');
  const trigger = screen.getByRole('button', { name: 'Folders & automation' });
  fireEvent.click(trigger);
  await screen.findByRole('dialog');
  fireEvent.click(screen.getByRole('button', { name: 'Close automation settings' }));
  await waitFor(() => expect(document.activeElement).toBe(trigger));
});
