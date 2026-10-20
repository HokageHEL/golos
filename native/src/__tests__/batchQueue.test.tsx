// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import BatchQueue from '../BatchQueue';
import type { Job } from '../api';

const fixture=vi.hoisted(()=>({
  jobs:[] as Job[],
  importBatch:vi.fn(),
  importDroppedBatch:vi.fn(),
  discardDroppedBatch:vi.fn(),
  run:vi.fn(),
  action:vi.fn(),
  cancel:vi.fn(),
  listeners:{} as Record<string,((event:{payload:unknown})=>void)[]>,
  unlisten:[] as ReturnType<typeof vi.fn>[],
}));
vi.mock('../api',()=>({api:{jobs:async()=>fixture.jobs,importBatch:fixture.importBatch,importDroppedBatch:fixture.importDroppedBatch,discardDroppedBatch:fixture.discardDroppedBatch,runQueue:fixture.run,jobAction:fixture.action,cancel:fixture.cancel}}));
vi.mock('@tauri-apps/api/event',()=>({listen:async (name:string,callback:(event:{payload:unknown})=>void)=>{
  (fixture.listeners[name]??=[]).push(callback);
  const unlisten=vi.fn();
  fixture.unlisten.push(unlisten);
  return unlisten;
}}));
beforeEach(()=>{
  fixture.jobs=[{id:1,noteId:'one',title:'Interview',durationSeconds:120,language:'uk',status:'queued',error:null}];
  fixture.listeners={};fixture.unlisten=[];
  fixture.importBatch.mockReset();fixture.importDroppedBatch.mockReset();fixture.discardDroppedBatch.mockReset().mockResolvedValue(undefined);
  fixture.run.mockReset();fixture.action.mockReset();fixture.cancel.mockReset().mockResolvedValue(undefined);
  HTMLDialogElement.prototype.showModal=vi.fn(function(this:HTMLDialogElement){this.open=true;});
  HTMLDialogElement.prototype.close=vi.fn(function(this:HTMLDialogElement){this.open=false;});
});
afterEach(cleanup);
const operation=async (_label:string,task:()=>Promise<void>)=>{try{await task();}catch{/* App.perform renders the error outside this test. */}};
const props=()=>({languages:[{code:'uk',name:'ukrainian'}],modelReady:true,busy:null,progress:0,error:null,onOperation:operation,onChanged:vi.fn().mockResolvedValue(undefined),onClose:vi.fn()});
async function emit(name:string,payload:unknown){
  await act(async()=>{fixture.listeners[name]?.forEach(callback=>callback({payload}));});
}

it('loads persisted jobs and imports additional files with independent failures',async()=>{
  fixture.importBatch.mockResolvedValue({notes:[{id:'two'}],failures:['broken.mp3: Cannot decode audio'],cancelled:false,duplicates:0});
  const callbacks=props();render(<BatchQueue {...callbacks}/>);
  await screen.findByText('Interview');
  expect(screen.getByText(/1 queued · 2:00 audio/)).toBeTruthy();
  fireEvent.change(screen.getByRole('combobox',{name:'Batch audio language'}),{target:{value:'uk'}});
  fireEvent.click(screen.getByRole('button',{name:'Add files to queue'}));
  await waitFor(()=>expect(fixture.importBatch).toHaveBeenCalledWith('uk'));
  await screen.findByText('broken.mp3: Cannot decode audio');
  expect(screen.getByText('1 recordings imported and queued.')).toBeTruthy();
  expect(callbacks.onChanged).toHaveBeenCalled();
});

it('previews a native drop and imports only after the explicit add button',async()=>{
  fixture.importDroppedBatch.mockResolvedValue({notes:[{id:'two'},{id:'three'}],failures:['bad.wav: Cannot decode audio'],cancelled:false,duplicates:1});
  const callbacks=props();render(<BatchQueue {...callbacks}/>);
  await screen.findByText('Interview');
  await emit('batch-drop',{id:'drop-1',names:['First.wav','Second.wav']});
  expect(screen.getByText('2 dropped files ready.')).toBeTruthy();
  expect(screen.getByText('First.wav')).toBeTruthy();
  expect(screen.getByText('Second.wav')).toBeTruthy();
  expect(fixture.importDroppedBatch).not.toHaveBeenCalled();
  fireEvent.change(screen.getByRole('combobox',{name:'Batch audio language'}),{target:{value:'uk'}});
  fireEvent.click(screen.getByRole('button',{name:'Add dropped files'}));
  await waitFor(()=>expect(fixture.importDroppedBatch).toHaveBeenCalledWith('drop-1','uk'));
  await screen.findByText('bad.wav: Cannot decode audio');
  expect(screen.getByText('2 recordings imported and queued. 1 duplicate file skipped within this selection.')).toBeTruthy();
  expect(screen.queryByLabelText('Dropped file names')).toBeNull();
  expect(callbacks.onChanged).toHaveBeenCalled();
});

it('discards a drop preview without reading or queueing files',async()=>{
  render(<BatchQueue {...props()}/>);
  await screen.findByText('Interview');
  await emit('batch-drop',{id:'drop-2',names:['Unwanted.wav']});
  fireEvent.click(screen.getByRole('button',{name:'Discard selection'}));
  await waitFor(()=>expect(fixture.discardDroppedBatch).toHaveBeenCalledWith('drop-2'));
  expect(screen.queryByText('Unwanted.wav')).toBeNull();
  expect(fixture.importDroppedBatch).not.toHaveBeenCalled();
});

it('replaces an older preview, enforces the front limit and surfaces busy drops',async()=>{
  const callbacks=props();render(<BatchQueue {...callbacks}/>);
  await screen.findByText('Interview');
  await emit('batch-drop',{id:'old',names:['Old.wav']});
  await emit('batch-drop',{id:'new',names:['New.wav']});
  expect(screen.queryByText('Old.wav')).toBeNull();
  expect(screen.getByText('New.wav')).toBeTruthy();
  await emit('batch-drop',{id:'too-many',names:Array.from({length:101},(_,index)=>`${index}.wav`)});
  expect(screen.getByRole('alert').textContent).toContain('Drop between 1 and 100');
  // The rejected oversized event never replaces the valid current preview.
  expect(screen.getByText('New.wav')).toBeTruthy();
  await emit('batch-drop-rejected',{message:'Finish or cancel the current operation before dropping files.'});
  expect(screen.getByRole('alert').textContent).toContain('Finish or cancel the current operation');
  expect(fixture.importDroppedBatch).not.toHaveBeenCalled();
  // A busy claim failure must not clear a preview whose native grant remains.
  fixture.importDroppedBatch.mockRejectedValueOnce(new Error('Wait for the current operation to finish.'));
  fireEvent.click(screen.getByRole('button',{name:'Add dropped files'}));
  await waitFor(()=>expect(fixture.importDroppedBatch).toHaveBeenCalledWith('new','auto'));
  expect(screen.getByText('New.wav')).toBeTruthy();
  fireEvent.click(screen.getByRole('button',{name:'Discard selection'}));
});

it('unsubscribes native listeners and invalidates an unconsumed grant on close',async()=>{
  const callbacks=props();const {unmount}=render(<BatchQueue {...callbacks}/>);
  await screen.findByText('Interview');
  await waitFor(()=>expect(fixture.unlisten.length).toBe(3));
  await emit('batch-drop',{id:'drop-close',names:['Pending.wav']});
  unmount();
  await waitFor(()=>fixture.unlisten.forEach(unlisten=>expect(unlisten).toHaveBeenCalled()));
  await waitFor(()=>expect(fixture.discardDroppedBatch).toHaveBeenCalledWith('drop-close'));
});

it('keeps a running queue visible and requests cancellation without deleting work',async()=>{
  fixture.jobs[0].status='running';
  const callbacks=props();render(<BatchQueue {...callbacks} busy="Transcribing batch" progress={42}/>);
  await screen.findByText(/Processing Interview \(42%\)/);
  expect((screen.getByRole('button',{name:'Close batch queue'}) as HTMLButtonElement).disabled).toBe(true);
  fireEvent(screen.getByRole('dialog'),new Event('cancel',{cancelable:true}));
  expect(callbacks.onClose).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button',{name:'Cancel current operation'}));
  await waitFor(()=>expect(fixture.cancel).toHaveBeenCalled());
  expect(fixture.action).not.toHaveBeenCalled();
});

it('retries cancelled jobs explicitly and removes only their queue entries',async()=>{
  fixture.jobs[0].status='cancelled';fixture.jobs[0].error='Cancelled';
  fixture.action.mockResolvedValue(undefined);
  render(<BatchQueue {...props()}/>);
  fireEvent.click(await screen.findByRole('button',{name:'Retry Interview'}));
  await waitFor(()=>expect(fixture.action).toHaveBeenCalledWith(1,'retry'));
  fireEvent.click(screen.getByRole('button',{name:'Remove Interview from queue'}));
  await waitFor(()=>expect(fixture.action).toHaveBeenCalledWith(1,'remove'));
});

it('refreshes job state from native events and requires a model before running',async()=>{
  render(<BatchQueue {...props()} modelReady={false}/>);
  await screen.findByText('Interview');
  expect((screen.getByRole('button',{name:'Start / resume queue'}) as HTMLButtonElement).disabled).toBe(true);
  fixture.jobs=[{...fixture.jobs[0],status:'done'}];
  await emit('queue-changed',null);
  await screen.findByText('2:00 · uk · done');
  expect(screen.getByText(/0 queued/)).toBeTruthy();
});
