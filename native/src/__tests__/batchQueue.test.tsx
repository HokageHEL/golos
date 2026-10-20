// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import BatchQueue from '../BatchQueue';
import type { Job } from '../api';

const fixture=vi.hoisted(()=>({ jobs:[] as Job[], importBatch:vi.fn(), run:vi.fn(), action:vi.fn(), cancel:vi.fn(), listener:null as null|(()=>void) }));
vi.mock('../api',()=>({api:{jobs:async()=>fixture.jobs,importBatch:fixture.importBatch,runQueue:fixture.run,jobAction:fixture.action,cancel:fixture.cancel}}));
vi.mock('@tauri-apps/api/event',()=>({listen:async (_name:string,callback:()=>void)=>{fixture.listener=callback;return ()=>{};}}));
beforeEach(()=>{
  fixture.jobs=[{id:1,noteId:'one',title:'Interview',durationSeconds:120,language:'uk',status:'queued',error:null}];
  fixture.importBatch.mockReset();fixture.run.mockReset();fixture.action.mockReset();fixture.cancel.mockReset().mockResolvedValue(undefined);
  HTMLDialogElement.prototype.showModal=vi.fn(function(this:HTMLDialogElement){this.open=true;});
  HTMLDialogElement.prototype.close=vi.fn(function(this:HTMLDialogElement){this.open=false;});
});
afterEach(cleanup);
const operation=async (_label:string,task:()=>Promise<void>)=>task();
const props=()=>({languages:[{code:'uk',name:'ukrainian'}],modelReady:true,busy:null,progress:0,error:null,onOperation:operation,onChanged:vi.fn().mockResolvedValue(undefined),onClose:vi.fn()});

it('loads persisted jobs and imports additional files with independent failures',async()=>{
  fixture.importBatch.mockResolvedValue({notes:[{id:'two'}],failures:['broken.mp3: Cannot decode audio'],cancelled:false});
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
  await act(async()=>fixture.listener?.());
  await screen.findByText('2:00 · uk · done');
  expect(screen.getByText(/0 queued/)).toBeTruthy();
});
