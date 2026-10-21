type Page = { id: string; label: string; hint?: string };

export default function SettingsNavigation({ label, pages, selected, onSelect }: {
  label: string;
  pages: readonly Page[];
  selected: string;
  onSelect: (id: string) => void;
}) {
  return <nav className="settings-navigation" aria-label={label}>
    {pages.map(page => <button type="button" key={page.id}
      aria-label={[page.label, page.hint].filter(Boolean).join(' ')}
      aria-current={selected === page.id ? 'page' : undefined}
      aria-controls={`settings-page-${page.id}`}
      onClick={() => onSelect(page.id)}>
      <span>{page.label}</span>{page.hint && <small>{page.hint}</small>}
    </button>)}
    <span className="sr-only" role="status" aria-live="polite" aria-atomic="true">{pages.find(page => page.id === selected)?.label} settings</span>
  </nav>;
}
