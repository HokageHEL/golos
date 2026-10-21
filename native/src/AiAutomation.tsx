import { useEffect, useRef, useState } from 'react';
import {
  api, type AiAutomationRule, type AiAutomationSource, type AiAutomationTask,
  type AiAutomationView, type AiSettings,
} from './api';

type Props = {
  busy: string | null;
  background: string | null;
  paused: boolean;
  onOperation: (label: string, task: () => Promise<void>) => Promise<void>;
  onPaused?: () => void;
};

const maxRules = 10;
const defaultSettings: AiSettings = { port: 11434, model: '', contextWindow: 8192 };
const kindOptions: [AiAutomationRule['kind'], string][] = [
  ['summary', 'Summary'],
  ['actions', 'Action points'],
  ['cleanup', 'Clean up transcript'],
  ['translate', 'Translate transcript'],
  ['custom', 'Custom prompt'],
];
const sourceOptions: [AiAutomationSource, string][] = [
  ['single', 'Single recording'],
  ['batch', 'Manual batch'],
  ['watch', 'Watched folders'],
];
const kindName = (kind: string) => kindOptions.find(([value]) => value === kind)?.[1] ?? kind;
const sourceName = (source: string) => sourceOptions.find(([value]) => value === source)?.[1] ?? source;
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);
const byteLength = (value: string) => new TextEncoder().encode(value).length;

function newRule(): AiAutomationRule {
  return { id: crypto.randomUUID(), name: '', enabled: false, kind: 'summary', target: '', prompt: '', sources: [] };
}
function cloneRules(rules: AiAutomationRule[]): AiAutomationRule[] {
  return rules.map(rule => ({ ...rule, sources: [...rule.sources] }));
}
function validate(rules: AiAutomationRule[]): string | null {
  if (rules.length > maxRules) return `Only ${maxRules} automatic rules can be saved.`;
  for (const [index, rule] of rules.entries()) {
    const label = rule.name.trim() || `Rule ${index + 1}`;
    if (!rule.name.trim()) return `${label}: enter a rule name.`;
    if (byteLength(rule.name.trim()) > 100) return `${label}: keep the rule name to 100 bytes or fewer.`;
    if (!rule.sources.length) return `${label}: choose at least one source.`;
    if (rule.sources.some(source => !sourceOptions.some(([value]) => value === source))) return `${label}: remove the unknown source.`;
    if (byteLength(rule.target.trim()) > 80) return `${label}: keep the target language to 80 bytes or fewer.`;
    if (byteLength(rule.prompt) > 4000) return `${label}: keep the custom prompt to 4,000 bytes or fewer.`;
    if (rule.kind === 'translate' && !rule.target.trim()) return `${label}: enter a target language for translation.`;
    if (rule.kind === 'custom' && !rule.prompt.trim()) return `${label}: enter a custom prompt.`;
  }
  return null;
}
function normalize(rule: AiAutomationRule): AiAutomationRule {
  return {
    ...rule,
    name: rule.name.trim(),
    target: rule.kind === 'cleanup' ? '' : rule.target.trim(),
    prompt: rule.kind==='custom'?rule.prompt.trim():'',
    sources: [...rule.sources],
  };
}

export default function AiAutomation({ busy, background, paused, onOperation,onPaused }: Props) {
  const [view, setView] = useState<AiAutomationView | null>(null);
  const [rules, setRules] = useState<AiAutomationRule[]>([]);
  const [tasks, setTasks] = useState<AiAutomationTask[]>([]);
  const [settings, setSettings] = useState<AiSettings>(defaultSettings);
  const [models, setModels] = useState<{ name: string; size: number }[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const initialized = useRef(false);
  useEffect(() => {
    let disposed = false;
    let loading = false;
    const load = async () => {
      if (loading) return;
      loading = true;
      try {
        const initial = !initialized.current;
        const [next, configuration] = await Promise.all([
          api.aiAutomation(),
          initial ? api.aiConfiguration() : Promise.resolve(null),
        ]);
        if (disposed) return;
        // Polling refreshes the task list only. Unsaved rule drafts and model
        // settings stay intact while the user is editing them.
        setView(next);
        setTasks(next.tasks);
        setLoadError(null);
        if (initial) {
          setRules(cloneRules(next.rules));
          if (configuration) setSettings({ ...configuration.settings });
          initialized.current = true;
        }
      } catch (e) {
        if (!disposed) setLoadError(errorText(e));
      } finally {
        loading = false;
      }
    };
    void load();
    const timer = setInterval(() => { void load(); }, 3000);
    return () => { disposed = true; clearInterval(timer); };
  }, [attempt]);
  const locked = Boolean(busy || background) || !view;
  const effectivePaused = paused;
  const validPort = Number.isInteger(settings.port) && settings.port >= 1024 && settings.port <= 65535;
  const rulesDirty = Boolean(view) && JSON.stringify(rules) !== JSON.stringify(view?.rules);
  async function run(label: string, task: () => Promise<void>) {
    setError(null); setMessage(null);
    await onOperation(label, async () => {
      try { await task(); } catch (e) { setError(errorText(e)); }
    });
  }
  async function reload() {
    const next = await api.aiAutomation();
    setView(next); setTasks(next.tasks);
  }
  function updateRule(id: string, patch: Partial<AiAutomationRule>) {
    setRules(current => current.map(rule => {
      if (rule.id !== id) return rule;
      const next = { ...rule, ...patch };
      if (next.kind === 'cleanup') next.target = '';
      if (patch.kind && patch.kind !== 'custom') next.prompt = '';
      return next;
    }));
  }
  function toggleSource(rule: AiAutomationRule, source: AiAutomationSource, checked: boolean) {
    updateRule(rule.id, { sources: checked ? [...rule.sources, source] : rule.sources.filter(item => item !== source) });
  }
  async function saveRules() {
    const problem = validate(rules);
    if (problem) { setMessage(null); setError(problem); return; }
    await run('Saving automatic AI rules', async () => {
      await api.aiAutomationConfigure(rules.map(normalize));
      const saved=await api.aiAutomation();
      setView(saved);setTasks(saved.tasks);setRules(cloneRules(saved.rules));
      setMessage('Automatic AI rules saved. They apply to future transcriptions only; saved edits are not backfilled or changed.');
    });
  }
  async function saveModel() {
    if (!validPort) { setMessage(null); setError('Use a local Ollama port from 1024 to 65535.'); return; }
    if (!settings.model.trim()) { setMessage(null); setError('Enter the tag of an installed local model.'); return; }
    await run('Saving local AI settings', async () => {
      const next = { ...settings, model: settings.model.trim() };
      await api.aiConfigure(next);
      setSettings(next);
      setView(current => current ? { ...current, model: next.model } : current);
      setMessage('Local AI model settings saved. Queued automatic tasks keep the model settings captured when they were queued.');
    });
  }
  async function refreshModels() {
    await run('Checking local models', async () => {
      const found = await api.aiModels(settings.port);
      setModels(found);
      setMessage(found.length ? 'Installed local models loaded.' : 'No local GGUF models found. Install a local instruct model in Ollama first.');
    });
  }
  async function pauseAutomatic() {
    await run('Pausing automatic AI', async () => {
      await api.pauseAutomation(true);
      onPaused?.();
      await reload();
      setMessage('Automatic AI paused. Running work is cancelling; queued rule work waits until you resume automation above.');
    });
  }
  async function skipQueued() {
    await run('Skipping queued automatic tasks',async()=>{
      const count=await api.aiAutomationSkipQueued();
      await reload();
      setMessage(`${count} queued automatic ${count===1?'task':'tasks'} skipped, including tasks outside this list. Transcripts and saved results are unchanged. Saved rules still apply to future transcriptions.`);
    });
  }
  async function taskAction(task: AiAutomationTask, action: 'retry' | 'skip' | 'remove') {
    const label = action === 'retry' ? 'Retrying automatic task' : action === 'skip' ? 'Skipping automatic task' : 'Removing automatic task';
    await run(label, async () => {
      await api.aiAutomationAction(task.id, action);
      await reload();
      setMessage(action === 'retry' ? 'Task queued again with its captured source. Saved results are kept separate.'
        : action === 'skip' ? 'Queued automatic task skipped. No transcript or result was changed.'
          : 'Automatic task removed from this list. Saved results and transcripts are unchanged.');
    });
  }
  return <section className="settings-section ai-automation" aria-labelledby="automatic-ai-title">
    <h2 id="automatic-ai-title">Automatic AI rules</h2>
    <p>Run local AI after a transcript is saved, only when you opt in. No account, API key, cloud provider or automatic model download is used. Automatic results are always separate from your transcript.</p>
    {loadError&&<div className="notice error" role="alert"><p>Automatic AI status unavailable: {loadError}</p><button className="secondary" disabled={!!busy} onClick={()=>{setLoadError(null);setAttempt(value=>value+1);}}>Retry loading automatic AI</button></div>}
    {error&&<div className="notice error" role="alert">{error}</div>}
    {message&&<div className="notice" role="status">{message}</div>}
    {background&&<div className="notice"><p>{background}. Rule and model changes are disabled while background work runs, but your rule drafts stay readable.</p></div>}
    {!view&&!loadError&&<p role="status">Loading automatic AI settings…</p>}
    {view&&<>
      <h3>Rules</h3>
      <p className="muted">Saved rules run only after new transcriptions that match their sources. They never backfill existing recordings, change saved transcript edits or overwrite a transcript. Disabling or deleting a saved rule stops future matches; queued work for that rule is skipped rather than run later. Review generated text for accuracy.</p>
      <div className="editor-tools"><button className="primary" disabled={locked||rules.length>=maxRules} onClick={()=>{setError(null);setMessage(null);setRules(current=>[...current,newRule()]);}}>Add automatic rule</button><span>{rules.length} of {maxRules} rules</span>{rulesDirty&&<span role="status">Unsaved rule changes</span>}</div>
      {!rules.length&&<p>No rules are configured. Automatic AI is off, so future transcriptions never run local AI unless you add and save a rule.</p>}
      {rules.length>0&&rules.every(rule=>!rule.enabled)&&<p className="muted">All rules are disabled, so automatic AI stays off until you enable and save a rule.</p>}
      {rules.map((rule,index)=><article className="ai-automation-rule" key={rule.id} aria-label={`Automatic rule ${index+1}`}>
        <div className="editor-tools"><strong>{rule.name.trim()||`Rule ${index+1}`}</strong><button className="text-button" disabled={locked} aria-label={`Delete rule ${index+1}`} onClick={()=>setRules(current=>current.filter(item=>item.id!==rule.id))}>Delete rule</button></div>
        <label>Rule name<input aria-label="Rule name" value={rule.name} maxLength={100} disabled={locked} placeholder="e.g. Meeting notes" onChange={event=>updateRule(rule.id,{name:event.target.value})}/></label>
        <label className="rule-enabled"><input type="checkbox" aria-label={`Enable rule ${index+1}`} checked={rule.enabled} disabled={locked} onChange={event=>updateRule(rule.id,{enabled:event.target.checked})}/> Enabled for future transcriptions</label>
        <label>Task<select aria-label="Rule task" value={rule.kind} disabled={locked} onChange={event=>updateRule(rule.id,{kind:event.target.value as AiAutomationRule['kind']})}>{kindOptions.map(([value,label])=><option key={value} value={value}>{label}</option>)}</select></label>
        {rule.kind==='cleanup'?<p className="muted">Cleanup always preserves the source language. Use a separate Translate transcript rule with an explicit target language for automatic translation.</p>:<label>Target language<input aria-label="Rule target language" value={rule.target} maxLength={80} disabled={locked} placeholder={rule.kind==='translate'?'Required for translation':'Optional; empty keeps the source language'} onChange={event=>updateRule(rule.id,{target:event.target.value})}/></label>}
        {rule.kind==='custom'&&<label>Custom prompt<textarea aria-label="Rule custom prompt" value={rule.prompt} maxLength={4000} rows={3} disabled={locked} placeholder="Describe exactly what the local model should produce." onChange={event=>updateRule(rule.id,{prompt:event.target.value})}/></label>}
        <fieldset className="rule-sources"><legend>Run after</legend>{sourceOptions.map(([value,label])=><label key={value}><input type="checkbox" checked={rule.sources.includes(value)} disabled={locked} onChange={event=>toggleSource(rule,value,event.target.checked)}/> {label}</label>)}</fieldset>
        <p className="muted">Single recording: a finished, saved transcript. Manual batch: only recordings you add to the batch queue. Watched folders: imported transcripts from watched folders.</p>
      </article>)}
      <div className="editor-tools"><button className="primary" disabled={locked} onClick={()=>{void saveRules();}}>Save automatic AI rules</button>{rulesDirty&&<button className="secondary" disabled={locked} onClick={()=>{setRules(cloneRules(view.rules));setError(null);setMessage(null);}}>Discard rule changes</button>}</div>
      <p className="muted">Saving rules applies them to future transcriptions only, never to saved edits, existing results or earlier recordings. Queued work waits while automation is paused.</p>

      <h3>Local model for automatic AI</h3>
      <p>Install Ollama and a local instruct model separately. Start it with <code>OLLAMA_NO_CLOUD=1</code> on the loopback interface: <code>OLLAMA_NO_CLOUD=1 ollama serve</code>. Golos only connects to a local service on 127.0.0.1, never pulls models and accepts no cloud endpoints or API keys.</p>
      {!settings.model&&<p className="unsaved" role="status">No local model tag is configured. Automatic rules cannot run until you save an installed local model.</p>}
      <p className="muted">Saved local model: {view.model||'not configured'}.</p>
      <div className="editor-tools"><label>Local Ollama port<input aria-label="Local Ollama port" type="number" min="1024" max="65535" value={settings.port} disabled={locked} onChange={event=>setSettings(current=>({...current,port:Number(event.target.value)}))}/></label><button className="secondary" disabled={locked||!validPort} onClick={()=>{void refreshModels();}}>Refresh installed models</button></div>
      <label>Installed local model<input aria-label="Installed local model" value={settings.model} maxLength={200} disabled={locked} placeholder="qwen2.5:1.5b" onChange={event=>setSettings(current=>({...current,model:event.target.value}))}/></label>
      <label>Context window<select aria-label="Context window" value={settings.contextWindow} disabled={locked} onChange={event=>setSettings(current=>({...current,contextWindow:Number(event.target.value)}))}>{[8192,16384,32768].map(size=><option key={size} value={size}>{size.toLocaleString()} tokens</option>)}</select></label>
      <ul className="ai-models">{models.map(model=><li key={model.name}>{model.name} · {(model.size/1e9).toFixed(2)} GB on disk <button className="text-button" disabled={locked} onClick={()=>setSettings(current=>({...current,model:model.name}))}>Use {model.name}</button></li>)}</ul>
      <div className="editor-tools"><button className="secondary" disabled={locked||!validPort||!settings.model.trim()} onClick={()=>{void saveModel();}}>Save local model settings</button><span className="muted">Opening this section or saving settings does not connect to Ollama.</span></div>
      <p className="muted">Changing rule prompts, targets or sources affects future jobs; queued jobs keep their captured rule. Disable a saved rule or skip its queued tasks to stop them. Changing model settings never changes queued jobs; each task keeps the model captured when it was queued. Larger contexts use more memory. Restart an existing Ollama service after enabling cloud-disabled mode.</p>

      <h3>Recent automatic tasks</h3>
      <p className="muted">The first 100 tasks, with running and oldest queued work first. The queue can contain up to 1,000 pending tasks. Skip all queued automatic tasks includes tasks outside this list. Finished task metadata is pruned; saved results stay available. Queued work waits while automation is paused; retry uses the captured source and model settings.</p>
      <div className="editor-tools"><button className="secondary" disabled={!!busy||effectivePaused} onClick={()=>{void pauseAutomatic();}}>Cancel automatic AI and pause</button><button className="secondary" disabled={locked||!tasks.some(task=>task.status==='queued')} onClick={()=>{void skipQueued();}}>Skip all queued automatic tasks</button><span role="status">{effectivePaused?'Automatic AI paused':'Automatic AI follows saved rules'}</span></div>
      {effectivePaused?<p className="unsaved" role="status">Automatic AI is paused. Running work is cancelling or stopped; queued rule work waits. Use Resume automation above to run queued and future work.</p>:<p className="muted">Global pause also pauses watched folders and local exports. It cancels the running background operation and leaves queued rule work waiting until you resume automation above. Running tasks cannot be cancelled individually.</p>}
      {view.label&&<p role="status">{view.label}</p>}
      {tasks.some(task=>task.status==='running')&&<p className="unsaved" role="status">A task is running. Use Cancel automatic AI and pause to stop it; the next queued task will not start until you resume automation above.</p>}
      <ul className="batch-jobs" aria-label="Recent automatic tasks">{tasks.map(task=><li key={task.id}>
        <div><strong>{task.title}</strong><span>{task.ruleName} · {kindName(task.kind)} · {sourceName(task.source)} · {task.model||'model not recorded'}</span><span aria-label={`Status of ${task.title}`}>{task.status}</span>{task.error&&<p className={task.status==='cancelled'||task.status==='skipped'?'muted':'error-text'}>{task.error}</p>}{task.resultId&&<p className="muted">{task.status==='done'?'Saved separately. Open recording → Local AI & translation → Saved results to review it.':'A separate attempt was saved. Open recording → Local AI & translation → Saved results to inspect its status and any partial output.'}</p>}{task.status==='done'&&!task.resultId&&<p className="muted">The saved result was deleted. The original transcript is unchanged.</p>}</div>
        <div className="actions"><button className="secondary" aria-label={`Retry automatic task ${task.title}`} disabled={locked||!['failed','cancelled','skipped'].includes(task.status)} onClick={()=>{void taskAction(task,'retry');}}>Retry</button><button className="secondary" aria-label={`Skip queued automatic task ${task.title}`} disabled={locked||task.status!=='queued'} onClick={()=>{void taskAction(task,'skip');}}>Skip</button><button className="text-button" aria-label={`Remove automatic task ${task.title}`} disabled={locked||!['done','failed','cancelled','skipped'].includes(task.status)} onClick={()=>{void taskAction(task,'remove');}}>Remove</button></div>
      </li>)}</ul>
      {!tasks.length&&<p>No automatic tasks yet.</p>}
      <p className="muted">Results are saved separately from the transcript and are never auto-exported. Existing exports still use the original saved transcript. To review an automatic result, open the recording and choose Local AI &amp; translation → Saved results.</p>
    </>}
  </section>;
}
