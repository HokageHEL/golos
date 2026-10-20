// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import LocalAi from '../LocalAi';
import type { AiWork, Note } from '../api';
const mocks=vi.hoisted(()=>({status:vi.fn(),generate:vi.fn(),cancel:vi.fn(),models:vi.fn(),get:vi.fn(),configure:vi.fn(),retry:vi.fn(),progress:null as null|((event:{payload:{noteId:string;phase:string}})=>void)}));
const note:Note={id:'one',title:'Interview',createdAt:1,durationSeconds:1,folder:'',tags:[],status:'done',error:null,segments:[{startMs:0,endMs:1000,speaker:'Alice',text:'Original text'}]};
const work:AiWork={id:'result',noteId:'one',kind:'summary',target:'',prompt:'',conversation:'',sourceLanguage:'auto',model:'test:local',contextWindow:8192,source:note.segments,sourceHash:'hash',status:'cancelled',content:'',segments:[],createdAt:1,error:'Generation cancelled. Source preserved.',phase:'Cancelled'};
vi.mock('../api',()=>({api:{aiStatus:mocks.status,aiGenerate:mocks.generate,aiConfigure:mocks.configure,aiModels:mocks.models,aiGet:mocks.get,aiRetry:mocks.retry,cancel:mocks.cancel}}));
vi.mock('@tauri-apps/api/event',()=>({listen:async(_name:string,callback:typeof mocks.progress)=>{mocks.progress=callback;return ()=>{};}}));
function Harness(){const[busy,setBusy]=useState<string|null>(null);return <LocalAi note={note} dirty={false} modelReady={true} languages={[{code:'uk',name:'ukrainian'}]} busy={busy} background={null} audioAvailable={false} onOperation={async(label,task)=>{setBusy(label);try{await task();}finally{setBusy(null);}}} onApply={()=>{}} onCreated={async()=>{}} onSeek={()=>{}} onClose={()=>{}}/>;}
beforeEach(()=>{vi.clearAllMocks();mocks.status.mockReset().mockResolvedValue({settings:{port:11434,model:'test:local',contextWindow:8192},results:[]});HTMLDialogElement.prototype.showModal=vi.fn(function(this:HTMLDialogElement){this.open=true;});HTMLDialogElement.prototype.close=vi.fn(function(this:HTMLDialogElement){this.open=false;});});
afterEach(cleanup);
it('keeps the dialog protected until cancellation returns and preserves cancelled results for retry',async()=>{
  let finish:(work:AiWork)=>void=()=>{};
  mocks.generate.mockImplementation(()=>new Promise<AiWork>(resolve=>{finish=resolve;}));
  mocks.cancel.mockImplementation(async()=>{finish(work);});
  mocks.retry.mockResolvedValue({...work,id:'retry',status:'done',error:null,content:'Reviewed summary'});
  render(<Harness/>);
  await waitFor(()=>expect((screen.getByRole('button',{name:'Generate locally'}) as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByRole('button',{name:'Generate locally'}));
  await waitFor(()=>expect(screen.getByRole('button',{name:'Cancel generation'})).toBeTruthy());
  expect((screen.getByRole('button',{name:'Close local AI'}) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole('button',{name:'Cancel generation'}));
  await screen.findAllByText('Generation cancelled. Source preserved.');
  expect(mocks.cancel).toHaveBeenCalledOnce();
  expect((screen.getByRole('button',{name:'Use as transcript draft'}) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole('button',{name:'Retry captured source'}));
  await screen.findByText('Reviewed summary');expect(mocks.retry).toHaveBeenCalledWith('result');
  expect(note.segments[0].text).toBe('Original text');
});
it('reports local-service errors and allows switching to Whisper without local AI configuration',async()=>{
  mocks.models.mockRejectedValue(new Error('Start local Ollama on this port.'));
  mocks.generate.mockResolvedValue({...work,kind:'speech_english',status:'done',error:null,content:'English result'});
  render(<Harness/>);
  await waitFor(()=>expect((screen.getByRole('button',{name:'Generate locally'}) as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByText('Set up optional local AI'));
  fireEvent.click(screen.getByRole('button',{name:'Refresh installed models'}));
  await screen.findByRole('alert');expect(screen.getByRole('alert').textContent).toContain('Start local Ollama');
  fireEvent.change(screen.getByRole('combobox',{name:'Local task'}),{target:{value:'speech_english'}});
  expect((screen.getByRole('combobox',{name:'Speech source language'}) as HTMLSelectElement).value).toBe('auto');
  fireEvent.change(screen.getByRole('combobox',{name:'Speech source language'}),{target:{value:'uk'}});
  fireEvent.click(screen.getByRole('button',{name:'Generate locally'}));await screen.findByText('English result');
  expect(mocks.configure).not.toHaveBeenCalled();expect(mocks.generate.mock.calls[0][0].kind).toBe('speech_english');expect(mocks.generate.mock.calls[0][0].sourceLanguage).toBe('uk');
});

it('resets completed progress before a repeat generation',async()=>{
  const done={...work,status:'done',error:null,content:'First summary'};
  mocks.generate.mockResolvedValueOnce(done).mockImplementationOnce(()=>new Promise(()=>{}));
  render(<Harness/>);
  await waitFor(()=>expect((screen.getByRole('button',{name:'Generate locally'}) as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByRole('button',{name:'Generate locally'}));await screen.findByText('First summary');
  act(()=>mocks.progress?.({payload:{noteId:'one',phase:'Complete'}}));
  fireEvent.click(screen.getByRole('button',{name:'Generate locally'}));
  await screen.findByText('Preparing local model…');
});

it('does not start inference when cancelled while local settings are still saving',async()=>{
  let finish:()=>void=()=>{};
  mocks.configure.mockImplementationOnce(()=>new Promise<void>(resolve=>{finish=resolve;}));
  mocks.cancel.mockResolvedValue(undefined);
  render(<Harness/>);
  await waitFor(()=>expect((screen.getByRole('button',{name:'Generate locally'}) as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByRole('button',{name:'Generate locally'}));
  await screen.findByRole('button',{name:'Cancel generation'});
  fireEvent.click(screen.getByRole('button',{name:'Cancel generation'}));
  await act(async()=>{finish();});
  await screen.findByText('Generation cancelled before starting. Source preserved.');
  expect(mocks.generate).not.toHaveBeenCalled();
});

it('loads and retains a second page of saved results',async()=>{
  const history=Array.from({length:105},(_,index)=>({...work,id:`result-${index}`,status:'done',error:null,stale:false}));
  mocks.status.mockImplementation(async(_id:string,offset=0)=>({settings:{port:11434,model:'test:local',contextWindow:8192},results:history.slice(offset,offset+100)}));
  render(<Harness/>);
  await screen.findByRole('button',{name:'Load older results'});
  expect(screen.getAllByRole('button',{name:'Summary'})).toHaveLength(100);
  fireEvent.click(screen.getByRole('button',{name:'Load older results'}));
  await waitFor(()=>expect(screen.getAllByRole('button',{name:'Summary'})).toHaveLength(105));
  expect(mocks.status).toHaveBeenLastCalledWith('one',100);
  expect(screen.queryByRole('button',{name:'Load older results'})).toBeNull();
});
