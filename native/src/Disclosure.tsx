import { useEffect, useRef, type ReactNode } from 'react';

type Props = {
  /** Visible trigger text. Plain text keeps one clear label for assistive tools. */
  summary: string;
  children: ReactNode;
  className?: string;
  /**
   * Short state shown next to the label, for example the applied filters or the
   * number of matches. The label text itself stays unchanged, so the trigger
   * keeps one stable name for assistive tools and for tests.
   */
  state?: string;
  /**
   * Opens the disclosure when it becomes true, so required guidance is shown
   * instead of hidden. The panel stays open until the user closes it, which
   * keeps the trigger honest about what is already on screen.
   */
  open?: boolean;
};

/**
 * A native disclosure that expands in the normal document flow.
 *
 * Nothing is positioned over the page, so the disclosure can never cover the
 * player, the transcript or the editor. Escape closes the panel and returns
 * focus to its trigger; the browser keeps the summary keyboard-operable.
 */
export default function Disclosure({ summary, children, className, state, open }: Props) {
  const ref = useRef<HTMLDetailsElement>(null);

  useEffect(() => {
    const element = ref.current;
    if (!element || !open || element.open) return;
    element.open = true;
  }, [open]);

  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || !element.open) return;
      // A field owns its own Escape handling, for example clearing a search
      // box. Closing the panel there would hide the field that still holds the
      // user's work, so the field keeps the key.
      const target = event.target;
      if (target instanceof Element && target.closest('input, textarea, select')) return;
      // The key is consumed here: a panel inside a dialog closes on its own
      // instead of dismissing the dialog and discarding unsaved drafts.
      event.preventDefault();
      event.stopPropagation();
      element.open = false;
      element.querySelector<HTMLElement>('summary')?.focus();
    };
    element.addEventListener('keydown', onKeyDown);
    return () => element.removeEventListener('keydown', onKeyDown);
  }, []);

  return <details ref={ref} className={className}>
    <summary data-state={state || undefined}>{summary}</summary>
    <div className="disclosure-body">{children}</div>
  </details>;
}
