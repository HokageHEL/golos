import { useEffect, useRef, type ReactNode } from 'react';

type Props = {
  /** Visible trigger text. Plain text keeps one clear label for assistive tools. */
  summary: string;
  children: ReactNode;
  className?: string;
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
export default function Disclosure({ summary, children, className, open }: Props) {
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
      event.stopPropagation();
      element.open = false;
      element.querySelector<HTMLElement>('summary')?.focus();
    };
    element.addEventListener('keydown', onKeyDown);
    return () => element.removeEventListener('keydown', onKeyDown);
  }, []);

  return <details ref={ref} className={className}>
    <summary>{summary}</summary>
    <div className="disclosure-body">{children}</div>
  </details>;
}
