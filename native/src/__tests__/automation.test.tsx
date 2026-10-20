// @vitest-environment jsdom
import { afterEach,beforeEach,expect,it,vi } from 'vitest';
import { cleanup,fireEvent,render,screen,waitFor } from '@testing-library/react';
import AutomationSettings from '../AutomationSettings';
import type { AutomationView } from '../api';
const fixture=vi.hoisted(()=>({view:null as AutomationView|null,add:vi.fn(),watch:vi.fn(),pause:vi.fn(),choose:vi.fn(),configure:vi.fn(),export:vi.fn(),retry:vi.fn()}));
vi.mock('../api',()=>({api:{automation:async()=>structuredClone(fixture.view),addWatch:fixture.add,watchAction:fixture.watch,pauseAutomation:fixture.pause,chooseExportFolder:fixture.choose,configureExports:fixture.configure,exportQueue:fixture.export,retryExports:fixture.retry,aiAutomation:async()=>({rules:[],tasks:[],model:'',label:null}),aiConfiguration:async()=>({settings:{port:11434,model:'',contextWindow:8192},results:[]})}}));
const props=()=>({languages:[{code:'uk',name:'ukrainian'}],busy:null,background:null,onOperation:async(_label:string,task:()=>Promise<void>)=>task(),onClose:vi.fn()});
beforeEach(()=>{
  fixture.view={settings:{paused:false,exportEnabled:false,exportDirectory:'/local/exports',exportFormats:['txt']},folders:[{id:'folder',path:'/local/interviews',enabled:true,autoTranscribe:false,language:'uk',error:null}],files:[],exports:[],label:null};
  for(const mock of [fixture.add,fixture.watch,fixture.pause,fixture.choose,fixture.configure,fixture.retry])mock.mockReset().mockResolvedValue(true);
  fixture.export.mockReset().mockResolvedValue(2);
  HTMLDialogElement.prototype.showModal=vi.fn(function(this:HTMLDialogElement){this.open=true;});HTMLDialogElement.prototype.close=vi.fn(function(this:HTMLDialogElement){this.open=false;});
});
afterEach(cleanup);
it('requires explicit folder selection and defaults to new files queued for review',async()=>{
  render(<AutomationSettings {...props()}/>);await screen.findByText('/local/interviews');
  fireEvent.change(screen.getByRole('combobox',{name:'New folder language'}),{target:{value:'uk'}});
  fireEvent.click(screen.getByRole('button',{name:'Add watched folder'}));
  await waitFor(()=>expect(fixture.add).toHaveBeenCalledWith('uk',false,false));
  fireEvent.click(screen.getByRole('checkbox',{name:'Include files already in the folder'}));
  fireEvent.click(screen.getByRole('checkbox',{name:'Transcribe automatically'}));
  fireEvent.click(screen.getByRole('button',{name:'Add watched folder'}));
  await waitFor(()=>expect(fixture.add).toHaveBeenLastCalledWith('uk',true,true));
});
it('can pause background work and stop observing a folder without a delete command',async()=>{
  render(<AutomationSettings {...props()} background="Importing watched audio"/>);await screen.findByText('/local/interviews');
  expect((screen.getByRole('button',{name:'Add watched folder'}) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole('button',{name:'Pause automation'}));await waitFor(()=>expect(fixture.pause).toHaveBeenCalledWith(true));
  fireEvent.click(screen.getByRole('button',{name:'Stop watching folder'}));await waitFor(()=>expect(fixture.watch).toHaveBeenCalledWith('folder','remove'));
});
it('persists selected output formats and schedules saved results independently of transcription',async()=>{
  render(<AutomationSettings {...props()}/>);await screen.findByText('/local/exports');
  fireEvent.click(screen.getByRole('checkbox',{name:'VTT'}));fireEvent.click(screen.getByRole('checkbox',{name:'Export automatically after transcription'}));
  fireEvent.click(screen.getByRole('button',{name:'Save export settings'}));await waitFor(()=>expect(fixture.configure).toHaveBeenCalledWith(true,['txt','vtt']));
  fireEvent.click(screen.getByRole('button',{name:'Export completed queue results'}));await screen.findByText('2 completed queue results scheduled for export using saved settings.');
  fireEvent.click(screen.getByRole('button',{name:'Retry failed exports'}));await waitFor(()=>expect(fixture.retry).toHaveBeenCalled());
});
it('surfaces denied directory access and keeps retry controls available',async()=>{
  fixture.add.mockRejectedValue('Cannot read folder. Reselect it or grant Files and Folders access.');
  render(<AutomationSettings {...props()}/>);await screen.findByText('/local/interviews');fireEvent.click(screen.getByRole('button',{name:'Add watched folder'}));
  expect(await screen.findByRole('alert')).toHaveProperty('textContent','Cannot read folder. Reselect it or grant Files and Folders access.');
  expect((screen.getByRole('button',{name:'Add watched folder'}) as HTMLButtonElement).disabled).toBe(false);
});
