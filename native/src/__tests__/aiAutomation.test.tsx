// @vitest-environment jsdom
// Native commands are mocked here. These tests cover UI states and validation,
// not real Ollama inference, audio hardware or Rust task execution.
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import AiAutomation from '../AiAutomation';
import type { AiAutomationRule, AiAutomationTask, AiAutomationView } from '../api';

const mocks = vi.hoisted(() => ({
  skipQueued: vi.fn(), status: vi.fn(), configureRules: vi.fn(), action: vi.fn(), configuration: vi.fn(),
  configureModel: vi.fn(), models: vi.fn(), pause: vi.fn(),
}));
vi.mock('../api', () => ({
  api: {
    aiAutomation: mocks.status,
    aiAutomationSkipQueued: mocks.skipQueued,
    aiAutomationConfigure: mocks.configureRules,
    aiAutomationAction: mocks.action,
    aiConfiguration: mocks.configuration,
    aiConfigure: mocks.configureModel,
    aiModels: mocks.models,
    pauseAutomation: mocks.pause,
  },
}));

const run = async (_label: string, task: () => Promise<void>) => task();
const props = (patch: Partial<{ busy: string | null; background: string | null; paused: boolean }> = {}) => ({
  busy: null, background: null, paused: false, onOperation: run, ...patch,
});
let state: AiAutomationView;
let modelSettings = { port: 11434, model: 'test:local', contextWindow: 8192 };
const rule = (patch: Partial<AiAutomationRule> = {}): AiAutomationRule => ({
  id: 'rule-1', name: 'Saved summary', enabled: true, kind: 'summary', target: '', prompt: '', sources: ['single'], ...patch,
});
const task = (patch: Partial<AiAutomationTask> = {}): AiAutomationTask => ({
  id: 'task-1', noteId: 'note-1', title: 'Weekly planning', kind: 'summary', ruleName: 'Saved summary',
  status: 'failed', error: 'Local model unavailable.', resultId: null, model: 'test:local', source: 'single', ...patch,
});

beforeEach(() => {
  vi.clearAllMocks();
  state = { rules: [], tasks: [], model: 'test:local', label: null };
  modelSettings = { port: 11434, model: 'test:local', contextWindow: 8192 };
  mocks.status.mockImplementation(async () => structuredClone(state));
  mocks.configuration.mockImplementation(async () => ({ settings: { ...modelSettings }, results: [] }));
  mocks.configureRules.mockImplementation(async (rules: AiAutomationRule[]) => { state.rules = structuredClone(rules); });
  mocks.action.mockImplementation(async (id: string, action: 'retry' | 'skip' | 'remove') => {
    const found = state.tasks.find(item => item.id === id);
    if (!found) return;
    if (action === 'remove') state.tasks = state.tasks.filter(item => item.id !== id);
    if (action === 'skip') { found.status = 'skipped'; found.error = null; }
    if (action === 'retry') { found.status = 'queued'; found.error = null; }
  });
  mocks.configureModel.mockImplementation(async (settings: typeof modelSettings) => {
    modelSettings = { ...settings }; state.model = settings.model;
  });
  mocks.models.mockResolvedValue([{ name: 'qwen2.5:1.5b', size: 986061892, digest: 'mock', context: 32768 }]);
  mocks.pause.mockResolvedValue(undefined);
  mocks.skipQueued.mockImplementation(async()=>{let count=0;state.tasks.forEach(item=>{if(item.status==='queued'){item.status='skipped';count++;}});return count;});
});
afterEach(cleanup);

it('defaults to off, validates opt-in choices and saves cleanup/custom rules explicitly', async () => {
  render(<AiAutomation {...props()}/>);
  await screen.findByText(/No rules are configured/);
  expect(screen.getByText(/Saved local model: test:local/)).toBeTruthy();
  expect(mocks.configureRules).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Add automatic rule' }));
  fireEvent.change(screen.getByLabelText('Rule name'), { target: { value: '  Meeting notes  ' } });
  fireEvent.click(screen.getByRole('button', { name: 'Save automatic AI rules' }));
  expect((await screen.findByRole('alert')).textContent).toContain('Meeting notes: choose at least one source.');
  expect(mocks.configureRules).not.toHaveBeenCalled();

  fireEvent.click(screen.getByRole('checkbox', { name: 'Single recording' }));
  fireEvent.change(screen.getByLabelText('Rule task'), { target: { value: 'cleanup' } });
  expect(screen.queryByLabelText('Rule target language')).toBeNull();
  expect(screen.getByText(/Cleanup always preserves the source language/)).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Save automatic AI rules' }));
  await waitFor(() => expect(mocks.configureRules).toHaveBeenCalledTimes(1));
  const savedCleanup = mocks.configureRules.mock.calls[0][0] as AiAutomationRule[];
  expect(savedCleanup[0]).toMatchObject({ name: 'Meeting notes', enabled: false, kind: 'cleanup', target: '', sources: ['single'] });
  expect(savedCleanup[0].id).toMatch(/^[0-9a-f-]{36}$/);
  await screen.findByText(/Automatic AI rules saved/);
  expect((screen.getByLabelText('Rule name') as HTMLInputElement).value).toBe('Meeting notes');
  expect(screen.queryByText('Unsaved rule changes')).toBeNull();

  fireEvent.change(screen.getByLabelText('Rule task'), { target: { value: 'custom' } });
  expect(screen.getByLabelText('Rule target language')).toBeTruthy();
  fireEvent.change(screen.getByLabelText('Rule target language'), { target: { value: 'Ukrainian' } });
  fireEvent.click(screen.getByRole('button', { name: 'Save automatic AI rules' }));
  await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('Meeting notes: enter a custom prompt.'));
  expect(mocks.configureRules).toHaveBeenCalledTimes(1);
  fireEvent.change(screen.getByLabelText('Rule custom prompt'), { target: { value: 'List decisions and owners.' } });
  fireEvent.click(screen.getByRole('button', { name: 'Save automatic AI rules' }));
  await waitFor(() => expect(mocks.configureRules).toHaveBeenCalledTimes(2));
  const savedCustom = mocks.configureRules.mock.calls[1][0] as AiAutomationRule[];
  expect(savedCustom[0]).toMatchObject({ kind: 'custom', target: 'Ukrainian', prompt: 'List decisions and owners.', sources: ['single'] });
}, 10000);

it('validates UTF-8 byte limits for rule fields', async () => {
  render(<AiAutomation {...props()}/>);
  await screen.findByText(/No rules are configured/);
  fireEvent.click(screen.getByRole('button', { name: 'Add automatic rule' }));
  fireEvent.click(screen.getByRole('checkbox', { name: 'Single recording' }));
  fireEvent.change(screen.getByLabelText('Rule name'), { target: { value: 'é'.repeat(51) } });
  fireEvent.click(screen.getByRole('button', { name: 'Save automatic AI rules' }));
  expect((await screen.findByRole('alert')).textContent).toContain('keep the rule name to 100 bytes or fewer');
  expect(mocks.configureRules).not.toHaveBeenCalled();
  fireEvent.change(screen.getByLabelText('Rule name'), { target: { value: 'Valid rule' } });
  fireEvent.change(screen.getByLabelText('Rule task'), { target: { value: 'custom' } });
  fireEvent.change(screen.getByLabelText('Rule custom prompt'), { target: { value: '😀'.repeat(1001) } });
  fireEvent.click(screen.getByRole('button', { name: 'Save automatic AI rules' }));
  expect((await screen.findByRole('alert')).textContent).toContain('keep the custom prompt to 4,000 bytes or fewer');
  expect(mocks.configureRules).not.toHaveBeenCalled();
});

it('caps automatic rules at ten without creating one by default', async () => {
  state.rules = Array.from({ length: 10 }, (_, index) => rule({ id: `rule-${index}`, name: `Rule ${index + 1}` }));
  render(<AiAutomation {...props()}/>);
  await screen.findByText('10 of 10 rules');
  expect((screen.getByRole('button', { name: 'Add automatic rule' }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getAllByLabelText('Rule name')).toHaveLength(10);
});

it('refreshes tasks by polling without clobbering unsaved rule or model drafts', async () => {
  state.rules = [rule()];
  state.tasks = [task({ status: 'queued', error: null })];
  render(<AiAutomation {...props()}/>);
  await screen.findByDisplayValue('Saved summary');
  fireEvent.change(screen.getByLabelText('Rule name'), { target: { value: 'Unsaved draft rule' } });
  fireEvent.change(screen.getByLabelText('Installed local model'), { target: { value: 'draft:tag' } });
  fireEvent.change(screen.getByLabelText('Context window'), { target: { value: '16384' } });
  state.tasks = [task({ status: 'running', error: null })];
  await waitFor(() => expect(screen.getByLabelText('Status of Weekly planning').textContent).toBe('running'), { timeout: 6000 });
  expect((screen.getByLabelText('Rule name') as HTMLInputElement).value).toBe('Unsaved draft rule');
  expect((screen.getByLabelText('Installed local model') as HTMLInputElement).value).toBe('draft:tag');
  expect((screen.getByLabelText('Context window') as HTMLSelectElement).value).toBe('16384');
  expect(mocks.status.mock.calls.length).toBeGreaterThan(1);
}, 10000);

it('surfaces status errors and retries loading the shared local setup', async () => {
  mocks.status.mockRejectedValueOnce(new Error('Native status unavailable.')).mockImplementation(async () => structuredClone(state));
  render(<AiAutomation {...props()}/>);
  expect((await screen.findByRole('alert')).textContent).toContain('Automatic AI status unavailable: Native status unavailable.');
  fireEvent.click(screen.getByRole('button', { name: 'Retry loading automatic AI' }));
  await screen.findByText(/No rules are configured/);
  expect(screen.getByText(/Saved local model: test:local/)).toBeTruthy();
  expect(mocks.status).toHaveBeenCalledTimes(2);
});

it('keeps configuration disabled during background work and pauses cancellation explicitly', async () => {
  state.rules = [rule()];
  state.tasks = [task({ id: 'queued', title: 'Queued meeting', status: 'queued', error: null }), task({ id: 'running', title: 'Research interview', status: 'running', error: null })];
  render(<AiAutomation {...props({ background: 'Transcribing watched audio' })}/>);
  await screen.findByDisplayValue('Saved summary');
  expect((screen.getByRole('button', { name: 'Add automatic rule' }) as HTMLButtonElement).disabled).toBe(true);
  expect((screen.getByRole('button', { name: 'Save automatic AI rules' }) as HTMLButtonElement).disabled).toBe(true);
  expect((screen.getByRole('button', { name: 'Save local model settings' }) as HTMLButtonElement).disabled).toBe(true);
  expect((screen.getByLabelText('Rule name') as HTMLInputElement).disabled).toBe(true);
  expect((screen.getByLabelText('Rule name') as HTMLInputElement).value).toBe('Saved summary');
  expect((screen.getByRole('button', { name: 'Skip queued automatic task Queued meeting' }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByText(/Running tasks cannot be cancelled individually/)).toBeTruthy();
  const pause = screen.getByRole('button', { name: 'Cancel automatic AI and pause' }) as HTMLButtonElement;
  expect(pause.disabled).toBe(false);
  fireEvent.click(pause);
  await waitFor(() => expect(mocks.pause).toHaveBeenCalledWith(true));
  await screen.findByText(/Automatic AI paused\. Running work is cancelling/);
  expect(screen.getAllByText(/resume automation above/).length).toBeGreaterThan(0);
  expect(screen.getByText(/Running tasks cannot be cancelled individually/)).toBeTruthy();
});

it('offers retry, skip and remove only for valid task states', async () => {
  state.tasks = [
    task({ id: 'queued', title: 'Queued meeting', status: 'queued', error: null }),
    task({ id: 'failed', title: 'Failed meeting', status: 'failed' }),
    task({ id: 'done', title: 'Done meeting', status: 'done', error: null, resultId: 'result-1' }),
    task({ id: 'running', title: 'Running meeting', status: 'running', error: null }),
  ];
  render(<AiAutomation {...props()}/>);
  await screen.findByLabelText('Status of Queued meeting');
  expect((screen.getByRole('button', { name: 'Retry automatic task Queued meeting' }) as HTMLButtonElement).disabled).toBe(true);
  expect((screen.getByRole('button', { name: 'Skip queued automatic task Queued meeting' }) as HTMLButtonElement).disabled).toBe(false);
  expect((screen.getByRole('button', { name: 'Retry automatic task Failed meeting' }) as HTMLButtonElement).disabled).toBe(false);
  expect((screen.getByRole('button', { name: 'Remove automatic task Failed meeting' }) as HTMLButtonElement).disabled).toBe(false);
  expect((screen.getByRole('button', { name: 'Retry automatic task Done meeting' }) as HTMLButtonElement).disabled).toBe(true);
  expect((screen.getByRole('button', { name: 'Skip queued automatic task Running meeting' }) as HTMLButtonElement).disabled).toBe(true);
  expect((screen.getByRole('button', { name: 'Remove automatic task Running meeting' }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByText(/Saved separately/)).toBeTruthy();

  fireEvent.click(screen.getByRole('button', { name: 'Skip queued automatic task Queued meeting' }));
  await waitFor(() => expect(mocks.action).toHaveBeenCalledWith('queued', 'skip'));
  fireEvent.click(screen.getByRole('button', { name: 'Retry automatic task Failed meeting' }));
  await waitFor(() => expect(mocks.action).toHaveBeenCalledWith('failed', 'retry'));
  fireEvent.click(screen.getByRole('button', { name: 'Remove automatic task Done meeting' }));
  await waitFor(() => expect(mocks.action).toHaveBeenCalledWith('done', 'remove'));
});

it('configures a loopback local model without connecting on open or save', async () => {
  render(<AiAutomation {...props()}/>);
  await screen.findByText(/No rules are configured/);
  expect(screen.getByText('Local model for automatic AI')).toBeTruthy();
  expect(mocks.models).not.toHaveBeenCalled();
  const context = screen.getByLabelText('Context window') as HTMLSelectElement;
  expect([...context.options].map(option => option.value)).toEqual(['8192', '16384', '32768']);

  const refresh = screen.getByRole('button', { name: 'Refresh installed models' }) as HTMLButtonElement;
  fireEvent.change(screen.getByLabelText('Local Ollama port'), { target: { value: '65536' } });
  expect(refresh.disabled).toBe(true);
  fireEvent.change(screen.getByLabelText('Local Ollama port'), { target: { value: '12345' } });
  expect(refresh.disabled).toBe(false);
  fireEvent.change(screen.getByLabelText('Installed local model'), { target: { value: ' qwen2.5:3b ' } });
  fireEvent.change(context, { target: { value: '32768' } });
  fireEvent.click(screen.getByRole('button', { name: 'Save local model settings' }));
  await waitFor(() => expect(mocks.configureModel).toHaveBeenCalledWith({ port: 12345, model: 'qwen2.5:3b', contextWindow: 32768 }));
  expect(mocks.models).not.toHaveBeenCalled();
  expect(mocks.configureRules).not.toHaveBeenCalled();
  await screen.findByText(/keep the model settings captured/);

  fireEvent.click(refresh);
  await waitFor(() => expect(mocks.models).toHaveBeenCalledWith(12345));
  await screen.findByText('Installed local models loaded.');
  fireEvent.click(screen.getByRole('button', { name: 'Use qwen2.5:1.5b' }));
  expect((screen.getByLabelText('Installed local model') as HTMLInputElement).value).toBe('qwen2.5:1.5b');
});

it('clears hidden custom instructions when changing to a standard task',async()=>{
  state.rules=[rule({kind:'custom',prompt:'Translate to French'})];render(<AiAutomation {...props()}/>);
  await screen.findByDisplayValue('Saved summary');
  fireEvent.change(screen.getByLabelText('Rule task'),{target:{value:'summary'}});
  fireEvent.click(screen.getByRole('button',{name:'Save automatic AI rules'}));
  await waitFor(()=>expect(mocks.configureRules).toHaveBeenCalled());
  expect(mocks.configureRules.mock.calls[0][0][0].prompt).toBe('');
});

it('requires an explicit target for automatic translation',async()=>{
  state.rules=[rule({kind:'translate',target:''})];render(<AiAutomation {...props()}/>);await screen.findByDisplayValue('Saved summary');
  fireEvent.click(screen.getByRole('button',{name:'Save automatic AI rules'}));expect((await screen.findByRole('alert')).textContent).toContain('enter a target language');expect(mocks.configureRules).not.toHaveBeenCalled();
  fireEvent.change(screen.getByLabelText('Rule target language'),{target:{value:'Ukrainian'}});fireEvent.click(screen.getByRole('button',{name:'Save automatic AI rules'}));await waitFor(()=>expect(mocks.configureRules).toHaveBeenCalled());expect(mocks.configureRules.mock.calls[0][0][0]).toMatchObject({kind:'translate',target:'Ukrainian',prompt:''});
});

it('distinguishes complete, deleted and incomplete automatic results',async()=>{
  state.tasks=[task({id:'complete',title:'Complete',status:'done',resultId:'result-complete',error:null}),task({id:'deleted',title:'Deleted',status:'done',resultId:null,error:null}),task({id:'partial',title:'Partial',status:'failed',resultId:'result-partial'})];
  render(<AiAutomation {...props()}/>);
  await screen.findByLabelText('Status of Complete');
  expect(screen.getAllByText(/Saved separately\./)).toHaveLength(1);
  expect(screen.getByText(/saved result was deleted/)).toBeTruthy();
  expect(screen.getByText(/A separate attempt was saved/)).toBeTruthy();
});

it('skips all queued automatic work without losing unsaved rule drafts',async()=>{
  state.rules=[rule()];state.tasks=[task({status:'queued',error:null})];
  render(<AiAutomation {...props({paused:true})}/>);
  await screen.findByDisplayValue('Saved summary');
  fireEvent.change(screen.getByLabelText('Rule name'),{target:{value:'Unsaved rule draft'}});
  fireEvent.click(screen.getByRole('button',{name:'Skip all queued automatic tasks'}));
  await waitFor(()=>expect(mocks.skipQueued).toHaveBeenCalledTimes(1));
  await screen.findByText(/1 queued automatic task skipped, including tasks outside this list/);
  expect((screen.getByLabelText('Rule name') as HTMLInputElement).value).toBe('Unsaved rule draft');
  expect(screen.getByLabelText('Status of Weekly planning').textContent).toBe('skipped');
  expect((screen.getByRole('button',{name:'Skip all queued automatic tasks'}) as HTMLButtonElement).disabled).toBe(true);
  expect(mocks.configureRules).not.toHaveBeenCalled();
});
