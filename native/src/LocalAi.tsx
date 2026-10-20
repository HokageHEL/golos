import { useEffect, useMemo, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { api, type AiSettings, type AiSummary, type AiWork, type Language, type Note, type TextExportFormat } from './api';
import type { Segment } from './transcript/types';
import { exportAiResult } from './export/ai';
import { formatClock } from './playback';

type Props = { note: Note; dirty: boolean; modelReady: boolean; languages: Language[]; busy: string | null; background: string | null; audioAvailable: boolean; onOperation: (label: string, task: () => Promise<void>) => Promise<void>; onApply: (segments: Segment[]) => void; onCreated: (note: Note) => Promise<void>; onSeek: (ms: number) => void; onClose: () => void };
const tasks = [['summary','Summary'],['actions','Action points'],['chat','Chat with transcript'],['custom','Custom prompt'],['cleanup','Clean up transcript'],['translate','Translate transcript'],['speech_english','Speech to English (Whisper)']];
const taskName = (kind: string) => tasks.find(task => task[0] === kind)?.[1] ?? kind;
const text = (error: unknown) => error instanceof Error ? error.message : String(error);
export default function LocalAi({note,dirty,modelReady,languages,busy,background,audioAvailable,onOperation,onApply,onCreated,onSeek,onClose}: Props) {
  const dialog=useRef<HTMLDialogElement>(null);
  const cancelRef=useRef(false);
  const [moreResults,setMoreResults]=useState(false);
  const [settings,setSettings]=useState<AiSettings|null>(null);
  const [results,setResults]=useState<AiSummary[]>([]);
  const [selected,setSelected]=useState<AiWork|null>(null);
  const [models,setModels]=useState<{name:string;size:number}[]>([]);
  const [kind,setKind]=useState('summary');
  const [target,setTarget]=useState('');
  const [sourceLanguage,setSourceLanguage]=useState('auto');
  const [setupExpanded,setSetupExpanded]=useState(false);
  const [sourceExpanded,setSourceExpanded]=useState(false);
  const [sourceCount,setSourceCount]=useState(100);
  const [previewCount,setPreviewCount]=useState(100);
  const [prompt,setPrompt]=useState('');
  const [conversation,setConversation]=useState('');
  const [error,setError]=useState<string|null>(null);
  const [message,setMessage]=useState<string|null>(null);
  const [phase,setPhase]=useState('');
  const [format,setFormat]=useState<TextExportFormat>('txt');
  const [deleting,setDeleting]=useState(false);
  const [cancelled,setCancelled]=useState(false);
  useEffect(()=>{
    const element=dialog.current;element?.showModal();let disposed=false;
    void api.aiStatus(note.id).then(view=>{if(!disposed){setSettings(view.settings);setSetupExpanded(!view.settings.model);setResults(view.results);setMoreResults(view.results.length===100);}}).catch(e=>{if(!disposed)setError(text(e));});
    const subscription=listen<{noteId:string;phase:string}>('ai-progress',event=>{if(!disposed&&event.payload.noteId===note.id)setPhase(event.payload.phase);});
    return()=>{disposed=true;element?.close();void subscription.then(unlisten=>unlisten()).catch(()=>{});};
  },[note.id]);
  const locked=!!busy||!!background||!settings;
  const speech=kind==='speech_english';
  const generating=busy==='Generating locally';
  const localRequest=generating||busy==='Checking local models';
  const validPort=!!settings&&Number.isInteger(settings.port)&&settings.port>=1024&&settings.port<=65535;
  const stale=useMemo(()=>!!selected&&(selected.source.length!==note.segments.length||selected.source.some((segment,index)=>{const current=note.segments[index];return segment.text!==current.text||segment.startMs!==current.startMs||segment.endMs!==current.endMs||segment.speaker!==current.speaker;})),[note.segments,selected]);
  const complete=selected?.status==='done';
  const structured=!!selected?.segments.length;
  const canGenerate=!locked&&!dirty&&(speech?modelReady:note.status==='done'&&!!note.segments.length&&!!settings?.model.trim()&&validPort)&&(kind!=='translate'||!!target.trim())&&(!['chat','custom'].includes(kind)||!!prompt.trim());
  async function reload(){const view=await api.aiStatus(note.id);setResults(view.results);setMoreResults(view.results.length===100);}
  async function action(label:string,task:()=>Promise<void>){setError(null);setMessage(null);setCancelled(false);cancelRef.current=false;if(label==='Generating locally')setPhase('Preparing local model…');await onOperation(label,async()=>{try{await task();if(!['Loading older results','Opening generated result'].includes(label))await reload();}catch(e){setError(text(e));}});}
  function accept(work:AiWork){setSelected(work);setDeleting(false);setFormat('txt');if(work.kind==='chat')setConversation(work.conversation);if(work.status==='cancelled')setMessage(work.error||'Generation cancelled. Source preserved.');else if(work.error)setError(work.error);else setMessage('Result saved locally. Review generated text before using it.');}
  async function generate(){await action('Generating locally',async()=>{if(!speech)await api.aiConfigure(settings!);if(cancelRef.current){setMessage('Generation cancelled before starting. Source preserved.');return;}accept(await api.aiGenerate({noteId:note.id,kind,target:speech?'English':kind==='cleanup'?'':target,prompt,conversation:kind==='chat'?conversation:'',sourceLanguage}));});}
  async function exportResult(){if(!selected)return;await action('Exporting generated result',async()=>{const content=exportAiResult(`${note.title} — ${taskName(selected.kind)}`,selected,format);if(await api.export(format,content))setMessage('Generated result exported.');});}
  return <dialog ref={dialog} className="settings-dialog ai-dialog" aria-labelledby="ai-title" aria-busy={!!busy} onCancel={event=>{event.preventDefault();if(!busy)onClose();}}>
    <div className="settings-content">
      <header className="settings-header"><div><div className="eyebrow">ON THIS MAC</div><h1 id="ai-title">Local AI &amp; translation</h1></div><button className="secondary" aria-label="Close local AI" disabled={!!busy} onClick={onClose}>✕</button></header>
      <p>Generate separate, saved results for <strong>{note.title}</strong>. Your transcript stays unchanged. No account, API key or cloud provider is used.</p>
      {error&&<div className="notice error" role="alert">{error}</div>}{message&&<div className="notice" role="status">{message}</div>}
      {busy&&!generating&&<p role="status">{busy}…</p>}
      {!settings&&error&&<button className="secondary" onClick={()=>{void api.aiStatus(note.id).then(view=>{setSettings(view.settings);setResults(view.results);setMoreResults(view.results.length===100);setSetupExpanded(!view.settings.model);setError(null);}).catch(e=>setError(text(e)));}}>Retry loading local AI</button>}
      {dirty&&<p className="unsaved" role="status">Save your transcript edits before generating or applying a result. Your current draft is preserved.</p>}
      {background&&<div className="notice"><p>{background}. Wait for it to finish, or pause automation to leave the next job queued.</p><button className="secondary" disabled={!!busy} onClick={()=>{void api.pauseAutomation(true).catch(e=>setError(text(e)));}}>Pause automation</button></div>}
      <details className="settings-section" open={setupExpanded} onToggle={event=>setSetupExpanded(event.currentTarget.open)}><summary>Set up optional local AI</summary>
        <p>Install Ollama and a local instruct model separately. Start Ollama with <code>OLLAMA_NO_CLOUD=1</code> and restart its service after changing this setting. Download model weights once; generation then works offline. Golos never pulls models or accepts cloud endpoints.</p>
        <p>Quit a running Ollama app before starting a cloud-disabled service in Terminal: <code>OLLAMA_NO_CLOUD=1 ollama serve</code>. In another Terminal run <code>ollama pull qwen2.5:1.5b</code> for a small starter model. Larger models may improve quality but need more memory and disk space.</p>
        <p>Only a local Ollama service you installed and trust should listen on this port. If connection fails, check that it is running on 127.0.0.1 and that the port matches. Local weights are checked before each request; cloud-backed tags are refused.</p>
        <div className="editor-tools"><label>Local Ollama port<input type="number" min="1024" max="65535" value={settings?.port??11434} disabled={locked} onChange={event=>setSettings(current=>current?{...current,port:Number(event.target.value)}:current)}/></label><button className="secondary" disabled={locked||!validPort} onClick={()=>{void action('Checking local models',async()=>{const found=await api.aiModels(settings!.port);setModels(found);setMessage(found.length?'Installed local models loaded.':'No local GGUF models found. Install a local instruct model in Ollama first.');});}}>Refresh installed models</button></div>
        <p className="muted">Listed models have installed GGUF weights. Text-generation capability and context are verified when generating; embedding-only models cannot generate.</p><ul className="ai-models">{models.map(model=><li key={model.name}>{model.name} · {(model.size/1e9).toFixed(2)} GB on disk <button className="text-button" disabled={locked} onClick={()=>setSettings(current=>current?{...current,model:model.name}:current)}>Use {model.name}</button></li>)}</ul>
        <label>Installed local model<input aria-label="Installed local model" value={settings?.model??''} maxLength={200} placeholder="qwen2.5:1.5b" disabled={locked} onChange={event=>setSettings(current=>current?{...current,model:event.target.value}:current)}/></label>
        <label>Context window<select value={settings?.contextWindow??8192} disabled={locked} onChange={event=>setSettings(current=>current?{...current,contextWindow:Number(event.target.value)}:current)}>{[8192,16384,32768].map(size=><option key={size} value={size}>{size.toLocaleString()} tokens</option>)}</select></label>
        <p className="muted">Generate saves the displayed local AI settings. Larger contexts use more memory. Long transcripts are processed in bounded chunks, with every paragraph covered; tasks that exceed the supported budget fail explicitly. Summaries combine all chunks.</p>
        <button className="secondary" disabled={locked||!validPort||!settings?.model.trim()} onClick={()=>{void action('Saving local AI settings',async()=>{await api.aiConfigure(settings!);setMessage('Local AI settings saved.');});}}>Save local AI settings</button>
      </details>
      <section className="settings-section"><h2>Generate a result</h2>
        <label>Task<select aria-label="Local task" autoFocus disabled={!!busy} value={kind} onChange={event=>{setKind(event.target.value);setConversation('');}}>{tasks.map(([value,label])=><option key={value} value={value}>{label}</option>)}</select></label>
        {speech&&<label>Speech source language<select aria-label="Speech source language" value={sourceLanguage} disabled={!!busy} onChange={event=>setSourceLanguage(event.target.value)}><option value="auto">Detect automatically</option>{languages.map(item=><option key={item.code} value={item.code}>{item.name}</option>)}</select></label>}
        {!speech&&note.status!=='done'&&<p>Transcribe this recording first for text tasks. Speech to English can run directly from saved audio.</p>}
        {!speech&&!settings?.model&&<p>Local AI is not configured. Open setup above and choose an installed model.</p>}
        {speech?<p>Whisper translates speech directly into English using your installed multilingual model. English-only and Turbo models are unsupported for this task. New paragraph boundaries come from audio; transferred speaker labels are overlap estimates and need review. The 30-minute limit applies. No Ollama setup is required.</p>:<p className="muted">Uses {settings?.model||'an installed local instruct model'}. Generation uses the saved transcript. Cleanup is manual and translation keeps paragraph times and speaker labels; neither adds word alignment.</p>}
        {!speech&&kind!=='cleanup'&&<label>{kind==='translate'?'Target language (required)':'Output language (optional)'}<input value={target} maxLength={80} disabled={!!busy} placeholder="e.g. Ukrainian" onChange={event=>setTarget(event.target.value)}/></label>}
        {['chat','custom'].includes(kind)&&<label>{kind==='chat'?'Question':'Custom prompt'}<textarea aria-label="Local AI prompt" value={prompt} maxLength={4000} disabled={!!busy} onChange={event=>setPrompt(event.target.value)} rows={3}/></label>}
        {kind==='chat'&&<div className="editor-tools"><span>{conversation?'Continuing saved chat':'New chat — all source paragraphs are included'}</span><button className="secondary" disabled={!!busy} onClick={()=>{setConversation('');setPrompt('');}}>New chat</button></div>}
        <div className="editor-tools"><button className="primary" disabled={!canGenerate} onClick={()=>{void generate();}}>Generate locally</button>{localRequest&&<><span role="status">{phase||'Preparing local model…'}</span><button className="secondary" disabled={cancelled} onClick={()=>{setCancelled(true);cancelRef.current=true;void api.cancel().catch(e=>{setCancelled(false);setError(text(e));});}}>{cancelled?'Cancelling…':generating?'Cancel generation':'Cancel model check'}</button></>}</div>
        {speech&&!modelReady&&<p>Download or import a multilingual Whisper model in Settings first.</p>}
      </section>
      <section className="settings-section"><h2>Saved results</h2><p className="muted">Results are listed in pages of 100. Project backups include all results within the documented size limits. Earlier results stay saved when you retry. Each result stores a source snapshot and adds disk usage; delete unwanted results to reclaim space.</p>
        <ul className="batch-jobs">{results.map(result=><li key={result.id}><div><button className="text-button" disabled={!!busy} onClick={()=>{void action('Opening generated result',async()=>{setSelected(await api.aiGet(result.id));setDeleting(false);setFormat('txt');setConversation(result.kind==='chat'?result.conversation:'');setSourceExpanded(false);setSourceCount(100);setPreviewCount(100);});}}>{taskName(result.kind)}{result.target?` · ${result.target}`:''}</button><span>{result.prompt?`${result.prompt.slice(0,100)} · `:''}{result.status} · {new Date(result.createdAt*1000).toLocaleString()} · {result.model}{result.stale?' · source has changed':''}</span></div></li>)}</ul>
        {moreResults&&<button className="secondary" disabled={!!busy} onClick={()=>{void action('Loading older results',async()=>{const page=await api.aiStatus(note.id,results.length);setResults(current=>[...current,...page.results.filter(item=>!current.some(old=>old.id===item.id))]);setMoreResults(page.results.length===100);});}}>Load older results</button>}
        {!results.length&&<p>No generated results yet.</p>}
        {selected&&<article className="ai-result"><h3>{taskName(selected.kind)}{selected.target?` · ${selected.target}`:''}</h3><p>{selected.status} · {selected.phase}</p>{selected.prompt&&<p><strong>Request:</strong> {selected.prompt}</p>}{selected.error&&selected.error!==error&&<p className={selected.status==='cancelled'?'muted':'error-text'}>{selected.error}</p>}{stale&&<p className="unsaved">The saved transcript changed after this result was created. The captured source is preserved below.</p>}
          {structured?<div className="ai-output" aria-label="Generated paragraphs">{selected.segments.slice(0,previewCount).map((segment,index)=><section key={index}><p className="muted">{formatClock(segment.startMs)}–{formatClock(segment.endMs)}{segment.speaker?` · ${segment.speaker}`:''}</p><p>{segment.text}</p></section>)}{selected.segments.length>previewCount&&<button className="secondary" onClick={()=>setPreviewCount(count=>count+100)}>Show more generated paragraphs</button>}</div>:<div className="ai-output">{selected.content||'No output was produced.'}</div>}
          <p className="muted">Generated text may contain errors. Review facts, translation, names and speaker labels.</p>
          <p className="muted">Retry uses the captured source below and your current model settings. To use saved transcript edits, choose a task above and Generate locally. A separate recording copies this captured result with the original audio, even when the source has changed.</p><div className="editor-tools"><button className="secondary" disabled={locked||!selected.content} onClick={()=>{void action('Copying generated result',async()=>{await navigator.clipboard.writeText(selected.content);setMessage('Generated result copied.');});}}>Copy result</button><button className="secondary" disabled={locked||dirty} onClick={()=>{void action('Generating locally',async()=>{if(selected.kind!=='speech_english')await api.aiConfigure(settings!);if(cancelRef.current){setMessage('Generation cancelled before starting. Source preserved.');return;}accept(await api.aiRetry(selected.id));});}}>Retry captured source</button>
            <button className="secondary" disabled={locked||dirty||!complete||!structured||!!stale||note.status!=='done'} onClick={()=>{void action('Preparing generated draft',async()=>{onApply(await api.aiSegments(selected.id));});}}>Use as transcript draft</button><button className="secondary" disabled={locked||!complete||!structured} onClick={()=>{void action('Creating translated recording',async()=>{await onCreated(await api.aiRecording(selected.id));});}}>Create separate recording</button></div>
          {complete&&<div className="editor-tools"><label>Result export format<select aria-label="Result export format" disabled={locked} value={format} onChange={event=>setFormat(event.target.value as TextExportFormat)}>{(structured?['txt','srt','vtt','md','html','csv','json']:['txt','json']).map(value=><option key={value} value={value}>{value.toUpperCase()}</option>)}</select></label><button className="secondary" disabled={locked} onClick={()=>{void exportResult();}}>Export result</button><button className="secondary" disabled={locked} onClick={()=>{void action('Exporting generated DOCX',async()=>{if(await api.aiDocx(selected.id))setMessage('Generated DOCX exported.');});}}>Export result DOCX</button></div>}
          <details open={sourceExpanded} onToggle={event=>setSourceExpanded(event.currentTarget.open)}><summary>Captured source &amp; paragraph references</summary><p>References such as [s1] point to these saved paragraphs. Playback seeks to paragraph times in the original recording.</p>{sourceExpanded&&selected.source.slice(0,sourceCount).map((segment,index)=><p key={index}><button className="text-button" disabled={!!busy||!audioAvailable} onClick={()=>{onSeek(segment.startMs);onClose();}}>[s{index+1}] {formatClock(segment.startMs)}</button>{segment.speaker?` ${segment.speaker}: `:' '}{segment.text}</p>)}{sourceExpanded&&selected.source.length>sourceCount&&<button className="secondary" onClick={()=>setSourceCount(count=>count+100)}>Show more source paragraphs</button>}</details>
          <div className="editor-tools">{deleting?<><span>Delete only this generated result?</span><button className="danger" disabled={locked} onClick={()=>{void action('Deleting generated result',async()=>{await api.aiDelete(selected.id);setSelected(null);setDeleting(false);});}}>Delete result permanently</button><button className="secondary" onClick={()=>setDeleting(false)}>Keep result</button></>:<button className="text-button" disabled={locked} onClick={()=>setDeleting(true)}>Delete result</button>}</div>
        </article>}
      </section>
    </div>
  </dialog>;
}
