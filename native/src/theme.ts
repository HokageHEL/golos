export type Theme = 'system' | 'light' | 'dark';
const storageKey = 'golos.theme';

export function readTheme(): Theme {
  try {
    const saved = localStorage.getItem(storageKey);
    if (saved === 'light' || saved === 'dark') return saved;
  } catch { /* Appearance still works when storage is unavailable. */ }
  return 'system';
}

export function saveTheme(theme: Theme) {
  try { localStorage.setItem(storageKey, theme); }
  catch { /* Keep the current session's choice even if it cannot be persisted. */ }
}

export function applyTheme(theme: Theme) {
  const resolved = theme === 'system'
    ? (window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')
    : theme;
  document.documentElement.dataset.theme = resolved;
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', resolved === 'dark' ? '#191f1c' : '#f7f6f2');
}
