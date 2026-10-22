type Page = { id: string; label: string; hint?: string };

export default function SettingsNavigation({ label, pages, selected, onSelect, section = 'settings' }: {
  label: string;
  pages: readonly Page[];
  selected: string;
  onSelect: (id: string) => void;
  /**
   * Noun for the announced page name. Settings announces "Appearance
   * settings", automation announces "Watched folders automation", so the
   * caller decides what the pages are called.
   */
  section?: string;
}) {
  return <nav className="settings-navigation" aria-label={label}>
    {pages.map(page => <button type="button" key={page.id}
      aria-label={[page.label, page.hint].filter(Boolean).join(' ')}
      aria-current={selected === page.id ? 'page' : undefined}
      aria-controls={`settings-page-${page.id}`}
      onClick={() => onSelect(page.id)}>
      <span>{page.label}</span>{page.hint && <small>{page.hint}</small>}
    </button>)}
    <span className="sr-only" role="status" aria-live="polite" aria-atomic="true">{pages.find(page => page.id === selected)?.label} {section}</span>
  </nav>;
}
