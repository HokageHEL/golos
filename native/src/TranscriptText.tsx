import { useEffect, useLayoutEffect, useRef } from 'react';

type Props = { onSplit?: (offset: number) => void; selection?: { start: number; end: number; nonce: number } | null; text: string; label: string; disabled: boolean; onChange: (text: string) => void };

/** Fit the actual wrapped text, rather than reserving two lines for every segment. */
export default function TranscriptText({ text, label, disabled, onChange, selection, onSplit }: Props) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    const element = ref.current;
    if (!element || !selection) return;
    element.scrollIntoView({ block: 'center' });
    element.focus({ preventScroll: true });
    element.setSelectionRange(selection.start, selection.end);
  }, [selection]);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const resize = () => { element.style.height = '0px'; element.style.height = `${element.scrollHeight + 2}px`; };
    resize();
    // Sidebar/window size changes can wrap text without changing its content.
    let width: number | undefined;
    let frame: number | null = null;
    const observer = new ResizeObserver(entries => {
      const nextWidth = entries[0]?.contentRect.width;
      if (nextWidth === undefined || nextWidth === width) return;
      width = nextWidth;
      // Mutating observed layout in the delivery callback can trigger a
      // ResizeObserver loop when window resizing also changes scrollbars.
      if (frame !== null) cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => { frame = null; resize(); });
    });
    observer.observe(element);
    return () => { observer.disconnect(); if (frame !== null) cancelAnimationFrame(frame); };
  }, [text]);
  return <textarea ref={ref} aria-label={label} value={text} rows={1} disabled={disabled} onChange={e => onChange(e.target.value)} onKeyDown={e => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey) && onSplit) { e.preventDefault(); onSplit(e.currentTarget.selectionStart); } }} title="⌘/Ctrl+Enter splits at the cursor"/>;
}
