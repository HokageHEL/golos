import { useEffect, useLayoutEffect, useRef } from 'react';

type Props = { selection?: { start: number; end: number; nonce: number } | null; text: string; label: string; disabled: boolean; onChange: (text: string) => void };

/** Fit the actual wrapped text, rather than reserving two lines for every segment. */
export default function TranscriptText({ text, label, disabled, onChange, selection }: Props) {
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
    let width = element.getBoundingClientRect().width;
    const observer = new ResizeObserver(entries => {
      const nextWidth = entries[0]?.contentRect.width;
      if (nextWidth !== width) { width = nextWidth; resize(); }
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [text]);
  return <textarea ref={ref} aria-label={label} value={text} rows={1} disabled={disabled} onChange={e => onChange(e.target.value)}/>;
}
