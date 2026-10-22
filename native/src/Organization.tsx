import { useState } from 'react';
import type { Note } from './api';

export default function Organization({ note, disabled, onSave }: { note: Note; disabled: boolean; onSave: (title: string, folder: string, tags: string[]) => Promise<void> }) {
  const [title, setTitle] = useState(note.title);
  const [folder, setFolder] = useState(note.folder ?? '');
  const [tags, setTags] = useState((note.tags ?? []).join(', '));
  return <details className="workspace-panel"><summary>Rename &amp; organize</summary><form className="organization" onSubmit={event => { event.preventDefault(); void onSave(title, folder, [...new Set(tags.split(',').map(t => t.trim()).filter(Boolean))]); }}>
    <label>Title<input required maxLength={200} value={title} onChange={e => setTitle(e.target.value)}/></label>
    <label>Folder<input maxLength={200} placeholder="e.g. Interviews" value={folder} onChange={e => setFolder(e.target.value)}/></label>
    <label>Tags (comma separated)<input value={tags} onChange={e => setTags(e.target.value)}/></label>
    <button className="secondary" disabled={disabled}>Save organization</button>
  </form></details>;
}
