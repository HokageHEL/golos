// @vitest-environment jsdom
// These checks cover the shared settings primitives in a browser DOM. They do
// not exercise native commands, recording hardware or inference.
import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import Disclosure from '../Disclosure';
import SettingsNavigation from '../SettingsNavigation';

afterEach(cleanup);

function summary(label: string) {
  return screen.getByText(label, { selector: 'summary' });
}

function panel(label: string) {
  return summary(label).closest('details') as HTMLDetailsElement;
}

/** A field that clears itself with Escape, like the transcript search box. */
function SelfClearingField({ label }: { label: string }) {
  const [value, setValue] = useState('saved');
  return <input aria-label={label} value={value} onChange={event => setValue(event.target.value)}
    onKeyDown={event => { if (event.key === 'Escape') setValue(''); }}/>;
}

it('leaves Escape to a field that handles it and keeps the panel open', () => {
  render(<Disclosure summary="Find &amp; replace"><SelfClearingField label="Find in transcript"/></Disclosure>);
  const field = screen.getByRole('textbox', { name: 'Find in transcript' });
  panel('Find & replace').open = true;
  fireEvent.keyDown(field, { key: 'Escape' });
  expect((field as HTMLInputElement).value).toBe('');
  expect(panel('Find & replace').open).toBe(true);
});

it('ignores Escape from a textarea and from a select', () => {
  render(<Disclosure summary="Speaker detection">
    <textarea aria-label="Rule custom prompt" defaultValue="draft"/>
    <select aria-label="Speakers" defaultValue="3"><option value="3">3</option></select>
  </Disclosure>);
  panel('Speaker detection').open = true;
  fireEvent.keyDown(screen.getByRole('textbox', { name: 'Rule custom prompt' }), { key: 'Escape' });
  expect(panel('Speaker detection').open).toBe(true);
  fireEvent.keyDown(screen.getByRole('combobox', { name: 'Speakers' }), { key: 'Escape' });
  expect(panel('Speaker detection').open).toBe(true);
});

it('closes the panel with Escape, returns focus to the summary and consumes the key', () => {
  render(<Disclosure summary="Export"><button type="button">Export TXT</button></Disclosure>);
  const details = panel('Export');
  details.open = true;
  const button = screen.getByRole('button', { name: 'Export TXT' });
  button.focus();
  // The key is consumed, so a dialog around this panel cannot be dismissed.
  expect(fireEvent.keyDown(button, { key: 'Escape' })).toBe(false);
  expect(details.open).toBe(false);
  expect(document.activeElement).toBe(summary('Export'));
});

it('keeps the trigger label unchanged and exposes the active state beside it', () => {
  render(<Disclosure summary="Filters" state="Transcribed · Work"><input aria-label="Filter folder"/></Disclosure>);
  expect(summary('Filters').textContent).toBe('Filters');
  expect(summary('Filters').getAttribute('data-state')).toBe('Transcribed · Work');
});

it('announces the section noun the caller provides', () => {
  const pages = [{ id: 'folders', label: 'Watched folders', hint: 'Import & transcribe' }];
  const { rerender } = render(<SettingsNavigation label="Automation sections" section="automation" pages={pages} selected="folders" onSelect={() => {}}/>);
  expect(screen.getByRole('status').textContent).toBe('Watched folders automation');
  rerender(<SettingsNavigation label="Settings sections" pages={pages} selected="folders" onSelect={() => {}}/>);
  expect(screen.getByRole('status').textContent).toBe('Watched folders settings');
});

it('points every navigation entry at a region that exists', () => {
  const pages = [{ id: 'folders', label: 'Watched folders' }, { id: 'exports', label: 'Local exports' }];
  render(<SettingsNavigation label="Settings sections" pages={pages} selected="folders" onSelect={() => {}}/>);
  const navigation = screen.getByRole('navigation', { name: 'Settings sections' });
  const targets = [...navigation.querySelectorAll('[aria-controls]')].map(button => button.getAttribute('aria-controls'));
  expect(targets).toEqual(['settings-page-folders', 'settings-page-exports']);
});

it('keeps a selected page in the navigation marked as the current one', () => {
  const onSelect = vi.fn();
  render(<SettingsNavigation label="Settings sections" pages={[{ id: 'model', label: 'Transcription', hint: 'Offline model' }]} selected="model" onSelect={onSelect}/>);
  const button = screen.getByRole('button', { name: 'Transcription Offline model' });
  expect(button.getAttribute('aria-current')).toBe('page');
  fireEvent.click(button);
  expect(onSelect).toHaveBeenCalledWith('model');
});
