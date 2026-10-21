import { useEffect, useRef } from 'react';
import type { Note } from './api';

export type RecordingMenuTarget = { note: Note; trigger: HTMLElement | null };

/**
 * Actions for one library recording.
 *
 * The menu renders inside the recording row, so it pushes the list instead of
 * floating over the player, the transcript or the editor. Escape closes it and
 * the caller returns focus to the trigger that opened it.
 */
export default function RecordingMenu({ target, onDelete, onClose }: {
  target: RecordingMenuTarget; onDelete: (note: Note) => void; onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.querySelector<HTMLButtonElement>('button')?.focus();
    const dismiss = (event: PointerEvent) => {
      const node = event.target as Node | null;
      if (!node) return;
      if (ref.current?.contains(node)) return;
      // The trigger toggles the menu itself, so it must not also dismiss it.
      if (target.trigger?.contains(node)) return;
      onClose();
    };
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') { event.stopPropagation(); onClose(); } };
    document.addEventListener('pointerdown', dismiss);
    document.addEventListener('keydown', key);
    return () => {
      document.removeEventListener('pointerdown', dismiss);
      document.removeEventListener('keydown', key);
    };
  }, [onClose, target.trigger]);
  return <div ref={ref} role="menu" aria-label={`Actions for ${target.note.title}`} className="recording-menu">
    <button type="button" role="menuitem" className="danger" onClick={() => onDelete(target.note)}>Delete recording…</button>
  </div>;
}
