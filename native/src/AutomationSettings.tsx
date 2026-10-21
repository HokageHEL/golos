import { useEffect, useRef, useState } from 'react';
import { api, type AutomationView, type Language } from './api';
import AiAutomation from './AiAutomation';
import SettingsNavigation from './SettingsNavigation';
type Props={ languages: Language[]; busy: string | null; background: string | null; onOperation: (label: string, task: () => Promise<void>) => Promise<void>; onClose: () => void };
const formats=['txt','srt','vtt','md','html','csv','json','docx'];
export default function AutomationSettings({languages,busy,background,onOperation,onClose}:Props){
  const dialog=useRef<HTMLDialogElement>(null);
  const content=useRef<HTMLDivElement>(null);
  const [view,setView]=useState<AutomationView|null>(null);
  const [language,setLanguage]=useState('auto');
  const [auto,setAuto]=useState(false);
  const [existing,setExisting]=useState(false);
  const [selected,setSelected]=useState(['txt']);
  const [enabled,setEnabled]=useState(false);
  const [error,setError]=useState<string|null>(null);
  const [message,setMessage]=useState<string|null>(null);
  const [page,setPage]=useState('folders');
  useEffect(()=>{if(content.current)content.current.scrollTop=0;},[page]);
  useEffect(()=>{
    const element=dialog.current;element?.showModal();let disposed=false;let loading=false;let initialized=false;
    const reload=async()=>{if(loading)return;loading=true;try{const next=await api.automation();if(!disposed){setView(next);if(!initialized){initialized=true;setSelected(next.settings.exportFormats);setEnabled(next.settings.exportEnabled);}}}catch(e){if(!disposed)setError(String(e));}finally{loading=false;}};
    void reload();const timer=setInterval(()=>{void reload();},3000);
    return()=>{disposed=true;clearInterval(timer);element?.close();};
  },[]);
  async function action(label:string,task:()=>Promise<void>){
    setError(null);setMessage(null);
    await onOperation(label,async()=>{try{await task();setView(await api.automation());}catch(e){setError(String(e));}});
  }
  async function control(task:()=>Promise<void>){setError(null);try{await task();setView(await api.automation());}catch(e){setError(String(e));}}
  const locked=!!busy||!!background||!view;
  return <dialog ref={dialog} className="settings-dialog automation-dialog settings-workspace-dialog" aria-labelledby="automation-title" onCancel={event=>{event.preventDefault();if(!busy)onClose();}}>
    <div className="settings-content settings-workspace">
      <header className="settings-header"><div><div className="eyebrow">LOCAL WORKFLOWS</div><h1 id="automation-title">Folders &amp; automation</h1></div><button className="secondary" aria-label="Close automation settings" disabled={!!busy} onClick={onClose} autoFocus>✕</button></header>
      <div className="settings-workspace-body">
      <SettingsNavigation label="Automation sections" selected={page} onSelect={setPage} pages={[
        {id:'folders',label:'Watched folders',hint:'Import & transcribe'},
        {id:'exports',label:'Local exports',hint:'Saved transcripts'},
        {id:'ai-rules',label:'AI rules',hint:'Opt-in processing'},
        {id:'ai-model',label:'AI model',hint:'Local service'},
        {id:'ai-tasks',label:'AI tasks',hint:'Queue & results'},
      ]}/>
      <div className="settings-page-content" ref={content}>
      {error&&<div className="notice error" role="alert">{error}</div>}{message&&<div className="notice" role="status">{message}</div>}
      <section id="settings-page-folders" hidden={page!=='folders'} className="settings-section" aria-labelledby="watched-folders-title"><h2 id="watched-folders-title">Watched folders</h2><p>Only folders you select are observed. New or changed audio files are imported after at least five seconds without a size or modification-time change. Original files are never changed. Subfolders and symlinks are skipped. Each file keeps the 30-minute / 512-MiB limit.</p>
        <div className="editor-tools"><label>New folder language<select aria-label="New folder language" value={language} disabled={locked} onChange={event=>setLanguage(event.target.value)}><option value="auto">Detect automatically</option>{languages.map(item=><option value={item.code} key={item.code}>{item.name}</option>)}</select></label><label><input type="checkbox" checked={auto} disabled={locked} onChange={event=>setAuto(event.target.checked)}/> Transcribe automatically</label><label><input type="checkbox" checked={existing} disabled={locked} onChange={event=>setExisting(event.target.checked)}/> Include files already in the folder</label><button className="secondary" disabled={locked} onClick={()=>{void action('Adding watched folder',async()=>{if(await api.addWatch(language,auto,existing))setMessage('Folder added. New stable files will appear in the batch queue.');});}}>Add watched folder</button></div>
        <p className="muted">Automatic transcription needs an installed model. Otherwise imported files wait in the batch queue for Start / resume. Turn off automatic transcription to review the queue first.</p>
        <ul className="batch-jobs">{view?.folders.map(folder=><li key={folder.id}><div><strong>{folder.path}</strong><span>{folder.enabled?'Watching':'Paused'} · {folder.language} · {folder.autoTranscribe?'Automatic transcription':'Queue for review'}</span>{folder.error&&<p className="error-text">{folder.error}</p>}</div><div className="actions"><button className="secondary" disabled={!!busy} onClick={()=>{void control(()=>api.watchAction(folder.id,folder.enabled?'pause':'resume'));}}>{folder.enabled?'Pause':'Resume'} folder</button><button className="secondary" disabled={!!busy} onClick={()=>{void control(()=>api.watchAction(folder.id,folder.autoTranscribe?'manual':'auto'));}}>{folder.autoTranscribe?'Queue for review':'Transcribe automatically'}</button><button className="secondary" disabled={locked} onClick={()=>{void control(()=>api.watchAction(folder.id,'retry'));}}>Retry failed imports</button><button className="text-button" disabled={!!busy} onClick={()=>{void control(()=>api.watchAction(folder.id,'remove'));}}>Stop watching folder</button></div></li>)}</ul>
        {!view?.folders.length&&<p className="muted">No watched folders selected.</p>}
        <details><summary>Recent detected files (up to 200)</summary><ul className="batch-jobs">{view?.files.map(file=><li key={`${file.folderId}:${file.name}`}><div><strong>{file.name}</strong><span>{file.status}</span>{file.error&&<p className="error-text">{file.error}</p>}</div></li>)}</ul></details>
      </section>
      <section id="settings-page-exports" hidden={page!=='exports'} className="settings-section" aria-labelledby="local-exports-title"><h2 id="local-exports-title">Local export automation</h2><p>Choose an output folder and one or more formats. Completed transcripts can export automatically. Existing files are never overwritten; export errors can be retried without running transcription again.</p><p className="automation-path">{view?.settings.exportDirectory??'No output folder selected'}</p><button className="secondary" disabled={locked} onClick={()=>{void action('Choosing export folder',async()=>{await api.chooseExportFolder();});}}>Choose export folder</button>
        <fieldset className="export-formats"><legend>Export formats</legend>{formats.map(format=><label key={format}><input type="checkbox" checked={selected.includes(format)} disabled={locked} onChange={event=>setSelected(current=>event.target.checked?[...current,format]:current.filter(item=>item!==format))}/>{format.toUpperCase()}</label>)}</fieldset>
        <label><input type="checkbox" checked={enabled} disabled={locked} onChange={event=>setEnabled(event.target.checked)}/> Export automatically after transcription</label><div className="editor-tools"><button className="primary" disabled={locked||!selected.length} onClick={()=>{void action('Saving export settings',async()=>{await api.configureExports(enabled,selected);setMessage('Export settings saved on this device.');});}}>Save export settings</button><button className="secondary" disabled={locked||!view?.settings.exportDirectory} onClick={()=>{void action('Scheduling exports',async()=>{const count=await api.exportQueue();setMessage(`${count} completed queue results scheduled for export using saved settings.`);});}}>Export completed queue results</button><button className="secondary" disabled={locked} onClick={()=>{void action('Retrying exports',async()=>{await api.retryExports();});}}>Retry failed exports</button></div>
        <p className="muted">Exports use saved transcript content and paragraph timestamps. Paused automation leaves export tasks pending. Re-exporting a changed saved transcript creates new files; identical results are not duplicated. Use Markdown to save directly into a local notes vault.</p>
        <ul className="batch-jobs" aria-label="Export tasks">{view?.exports.map(task=><li key={task.noteId}><div><strong>{task.title}</strong><span>{task.status}</span>{task.error&&<p className="error-text">{task.error}</p>}</div></li>)}</ul>
      </section>
      <div hidden={!page.startsWith('ai-')}>
      <AiAutomation page={page==='ai-model'?'model':page==='ai-tasks'?'tasks':'rules'} busy={busy} background={background} paused={Boolean(view?.settings.paused)} onPaused={()=>setView(current=>current?{...current,settings:{...current.settings,paused:true}}:current)} onOperation={onOperation}/>
      </div>
      </div>
      </div>
      <footer className="settings-footer automation-footer"><div><span role="status">{background??(!view?'Loading automation…':view.settings.paused?'Automation paused':'Automation ready')}</span><span className="muted">Runs while Golos is open. Everything stays on this device.</span></div><button className="secondary" disabled={!view} onClick={()=>{void control(()=>api.pauseAutomation(!view?.settings.paused));}}>{view?.settings.paused?'Resume automation':'Pause automation'}</button></footer>
    </div>
  </dialog>;
}
