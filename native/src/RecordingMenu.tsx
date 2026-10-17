import { useEffect, useRef } from 'react';
import type { Note } from './api';

export type RecordingMenuTarget = { note: Note; x: number; y: number };

export default function RecordingMenu({ target, onDelete, onClose }: {
  target: RecordingMenuTarget; onDelete: (note: Note) => void; onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.querySelector<HTMLButtonElement>('button')?.focus();
    const dismiss = (event: PointerEvent) => { if (!ref.current?.contains(event.target as Node)) onClose(); };
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape' || event.key === 'Tab') onClose(); };
    document.addEventListener('pointerdown', dismiss);
    document.addEventListener('keydown', key);
    window.addEventListener('resize', onClose);
    window.addEventListener('scroll', onClose, true);
    return () => {
      document.removeEventListener('pointerdown', dismiss);
      document.removeEventListener('keydown', key);
      window.removeEventListener('resize', onClose);
      window.removeEventListener('scroll', onClose, true);
    };
  }, [onClose]);
  return <div ref={ref} role="menu" aria-label={`Actions for ${target.note.title}`} className="recording-menu"
    style={{ left: Math.max(8, Math.min(target.x, window.innerWidth - 224)), top: Math.max(8, Math.min(target.y, window.innerHeight - 62)) }}>
    <button type="button" role="menuitem" className="danger" onClick={() => onDelete(target.note)}>Delete recording…</button>
  </div>;
}
