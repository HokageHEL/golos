'use client';
import { useEffect, useState } from 'react';
import { useTranslations } from 'next-intl';
import { toTurns } from '@/lib/transcript/turns';
import { toPlainText } from '@/lib/export/txt';
import type { Segment } from '@/lib/transcript/types';
import TranscriptView from './TranscriptView';
import ExportMenu from './ExportMenu';
import Toast from './Toast';

export default function TranscriptEditor({ jobId, segments, canEdit, updatedAt, onSaved }: {
  jobId: string; segments: Segment[]; canEdit: boolean; updatedAt: string; onSaved?: (segments: Segment[]) => void;
}) {
  const t = useTranslations('result');
  const [draft, setDraft] = useState(() => toTurns(segments));
  const [version, setVersion] = useState(updatedAt);
  const [dirty, setDirty] = useState(false);
  const [editing, setEditing] = useState(false);
  const [pending, setPending] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  useEffect(() => {
    if (!dirty) return;
    const warn = (event: BeforeUnloadEvent) => event.preventDefault();
    const navigate = (event: MouseEvent) => {
      if (event.target instanceof Element && event.target.closest('a[href]') && !window.confirm(t('discard'))) {
        event.preventDefault(); event.stopPropagation();
      }
    };
    window.addEventListener('beforeunload', warn);
    document.addEventListener('click', navigate, true);
    return () => { window.removeEventListener('beforeunload', warn); document.removeEventListener('click', navigate, true); };
  }, [dirty, t]);
  async function save() {
    setPending(true);
    try {
      const res = await fetch(`/api/jobs/${jobId}`, { method: 'PATCH',
        headers: { 'content-type': 'application/json' }, body: JSON.stringify({ segments: draft, updatedAt: version }) });
      if (res.status === 409) { setToast(t('conflict')); return; }
      if (!res.ok) throw new Error('Save failed');
      const data = await res.json();
      setVersion(data.updatedAt); onSaved?.(draft); setDirty(false); setEditing(false); setToast(t('saved'));
    } catch { setToast(t('saveFailed')); } finally { setPending(false); }
  }
  async function copy() {
    try { await navigator.clipboard.writeText(toPlainText(draft, (n) => t('speaker', { n }))); setToast(t('copied')); }
    catch { setToast(t('copyFailed')); }
  }
  return <>
    <div className="toolbar">
      <button className="btn btn-secondary btn-sm" onClick={copy}>{t('copy')}</button>
      {canEdit && <button className="btn btn-secondary btn-sm" disabled={pending} onClick={() => setEditing(!editing)}>{t(editing ? 'preview' : 'edit')}</button>}
      {canEdit && dirty && <button className="btn btn-primary btn-sm" disabled={pending} onClick={save}>{t(pending ? 'saving' : 'save')}</button>}
      {!dirty && <ExportMenu jobId={jobId} canExport={canEdit} />}
      {dirty && <span role="status" className="small">{t('unsaved')}</span>}
    </div>
    {draft.length === 0 ? <p className="notice">{t('empty')}</p> : editing ?
      <div className="card transcript">{draft.map((turn, i) => <label className="field" key={i}>
        <span className="small">{turn.speaker ? `${turn.speaker} · ` : ''}{Math.floor(turn.startMs / 60000)}:{String(Math.floor(turn.startMs / 1000) % 60).padStart(2, '0')}</span>
        <textarea className="input" rows={3} value={turn.text} disabled={pending}
          onChange={(event) => { setDraft(draft.map((s, n) => n === i ? { ...s, text: event.target.value } : s)); setDirty(true); }} />
      </label>)}</div> : <TranscriptView segments={draft} />}
    {toast && <Toast message={toast} onDismiss={() => setToast(null)} />}
  </>;
}
