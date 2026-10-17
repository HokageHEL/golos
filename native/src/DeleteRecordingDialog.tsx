import { useEffect, useRef } from 'react';

export default function DeleteRecordingDialog({ title, busy, error, onDelete, onClose }: {
  title: string; busy: boolean; error: string | null; onDelete: () => void; onClose: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    return () => dialog?.close();
  }, []);
  return <dialog ref={ref} className="delete-dialog" aria-labelledby="delete-recording-title" onCancel={event => { event.preventDefault(); if (!busy) onClose(); }}>
    <h2 id="delete-recording-title">Delete “{title}”?</h2>
    <p>The recording and transcript will be removed from Golos. Your original imported file stays where it was.</p>
    {error && <p className="error-text" role="alert">{error}</p>}
    <div className="actions"><button autoFocus className="secondary" disabled={busy} onClick={onClose}>Keep recording</button>
      <button className="danger-button" disabled={busy} onClick={onDelete}>{busy ? 'Deleting…' : 'Delete recording'}</button></div>
  </dialog>;
}
