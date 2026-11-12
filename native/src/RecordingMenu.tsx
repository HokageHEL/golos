import { useId, useLayoutEffect, useRef, useState } from 'react';
import type { Note } from './api';

export type RecordingMenuTarget = { note: Note; x: number; y: number; origin: HTMLElement; workspace?: boolean };
type Submenu = 'export' | 'speakers';
type Props = {
  target: RecordingMenuTarget;
  onClose: () => void;
  onDelete: (note: Note) => void;
  onExport: (format: 'txt' | 'srt') => void;
  onDetect: () => void;
  onEditSpeakers: () => void;
  onSpeakerSettings: () => void;
  canExport: boolean;
  canDetect: boolean;
  canEditSpeakers: boolean;
  canDelete: boolean;
  speakerReady: boolean;
};

export default function RecordingMenu({ target, onClose, onDelete, onExport, onDetect,
  onEditSpeakers, onSpeakerSettings, canExport, canDetect, canEditSpeakers, canDelete, speakerReady }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [submenu, setSubmenu] = useState<Submenu | null>(null);
  const focusSubmenu = useRef(false);
  const id = useId();

  useLayoutEffect(() => {
    ref.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
    const dismiss = (event: PointerEvent) => {
      if (!ref.current?.contains(event.target as Node) && !target.origin.contains(event.target as Node)) onClose();
    };
    const scroll = (event: Event) => { if (!ref.current?.contains(event.target as Node)) onClose(); };
    document.addEventListener('pointerdown', dismiss);
    window.addEventListener('resize', onClose);
    window.addEventListener('scroll', scroll, true);
    return () => {
      document.removeEventListener('pointerdown', dismiss);
      window.removeEventListener('resize', onClose);
      window.removeEventListener('scroll', scroll, true);
    };
  }, [target, onClose]);

  useLayoutEffect(() => {
    const menu = ref.current;
    if (!menu) return;
    const bounds = menu.getBoundingClientRect();
    menu.style.left = `${Math.max(8, Math.min(target.x, window.innerWidth - bounds.width - 8))}px`;
    menu.style.top = `${Math.max(8, Math.min(target.y, window.innerHeight - bounds.height - 8))}px`;
    if (focusSubmenu.current) {
      menu.querySelector<HTMLButtonElement>('[data-submenu] button:not(:disabled)')?.focus();
      focusSubmenu.current = false;
    }
  }, [target, submenu]);

  const run = (action: () => void) => { onClose(); target.origin.focus(); action(); };
  function closeSubmenu() {
    ref.current?.querySelector<HTMLButtonElement>(`[data-trigger="${submenu}"]`)?.focus();
    setSubmenu(null);
  }
  function toggle(value: Submenu) { setSubmenu(current => current === value ? null : value); }

  return <div ref={ref} role="menu" aria-label={`Actions for ${target.note.title}`} className="recording-menu"
    onKeyDown={event => {
      const menu = ref.current;
      if (!menu) return;
      if (event.key === 'Escape') {
        event.preventDefault(); event.stopPropagation();
        if (submenu) closeSubmenu(); else { onClose(); target.origin.focus(); }
      } else if (event.key === 'Tab') {
        target.origin.focus(); onClose();
      } else if (event.key === 'ArrowRight') {
        const value = (document.activeElement as HTMLElement)?.dataset.trigger as Submenu | undefined;
        if (value) {
          event.preventDefault();
          if (submenu === value) menu.querySelector<HTMLButtonElement>('[data-submenu] button:not(:disabled)')?.focus();
          else { focusSubmenu.current = true; setSubmenu(value); }
        }
      } else if (event.key === 'ArrowLeft' && submenu) {
        event.preventDefault(); closeSubmenu();
      } else if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
        event.preventDefault();
        const items = Array.from(menu.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
        const index = items.indexOf(document.activeElement as HTMLButtonElement);
        const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1
          : (index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
        items[next]?.focus();
      }
    }}>
    {target.workspace && <>
      <button type="button" role="menuitem" tabIndex={-1} data-trigger="export"
        aria-haspopup="menu" aria-controls={`${id}-export`} aria-expanded={submenu === 'export'}
        disabled={!canExport} onClick={() => toggle('export')}>Export <span aria-hidden="true">{submenu === 'export' ? '▾' : '▸'}</span></button>
      {submenu === 'export' && <div id={`${id}-export`} className="menu-subgroup" role="menu" aria-label="Export format" data-submenu>
        <button type="button" role="menuitem" tabIndex={-1} onClick={() => run(() => onExport('txt'))}>Text (.txt)</button>
        <button type="button" role="menuitem" tabIndex={-1} onClick={() => run(() => onExport('srt'))}>Subtitles (.srt)</button>
      </div>}
      <button type="button" role="menuitem" tabIndex={-1} data-trigger="speakers"
        aria-haspopup="menu" aria-controls={`${id}-speakers`} aria-expanded={submenu === 'speakers'}
        onClick={() => toggle('speakers')}>Speakers <span aria-hidden="true">{submenu === 'speakers' ? '▾' : '▸'}</span></button>
      {submenu === 'speakers' && <div id={`${id}-speakers`} className="menu-subgroup" role="menu" aria-label="Speaker actions" data-submenu>
        <button type="button" role="menuitem" tabIndex={-1} disabled={!canDetect} onClick={() => run(onDetect)}>{speakerReady ? 'Detect speakers' : 'Set up detection…'}</button>
        <button type="button" role="menuitem" tabIndex={-1} disabled={!canEditSpeakers} onClick={() => run(onEditSpeakers)}>Edit speaker labels</button>
        <button type="button" role="menuitem" tabIndex={-1} onClick={() => run(onSpeakerSettings)}>Speaker settings…</button>
      </div>}
    </>}
    <div className={target.workspace ? 'menu-danger' : undefined}>
      <button type="button" role="menuitem" tabIndex={-1} disabled={!canDelete} className="danger" onClick={() => run(() => onDelete(target.note))}>Delete recording…</button>
    </div>
  </div>;
}
