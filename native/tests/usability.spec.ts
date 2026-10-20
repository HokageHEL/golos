import { test, expect, type Page } from '@playwright/test';

// These are real browser interactions against mocked native IPC. They do not
// request OS access, record sound, run inference or write the native library.
const browserErrors = new WeakMap<Page, string[]>();
test.beforeEach(async ({ page }) => {
  const errors: string[] = []; browserErrors.set(page, errors);
  page.on('pageerror', error => errors.push(error.message));
  await page.goto('/tests/ui.html');
  await expect(page.getByRole('heading', { name: 'Research interview', exact: true })).toBeVisible();
});

test.afterEach(async ({ page }) => {
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  expect(browserErrors.get(page)).toEqual([]);
});

test('search and navigation retain drafts, and editing/export tools remain usable', async ({ page }) => {
  const first = page.getByRole('textbox', { name: 'Transcript at 0:00', exact: true });
  await first.fill('An unsaved browser draft.');
  await page.getByRole('searchbox', { name: 'Search library', exact: true }).fill('Weekly');
  await page.getByRole('button', { name: /Weekly planning.*Transcribed/ }).click();
  await expect(page.getByRole('heading', { name: 'Weekly planning', exact: true })).toBeVisible();
  await page.getByRole('searchbox', { name: 'Search library', exact: true }).fill('');
  await page.getByRole('button', { name: /Research interview.*Transcribed/ }).click();
  await expect(first).toHaveValue('An unsaved browser draft.');
  await first.fill('Another edit.');
  await page.getByRole('button', { name: 'Undo', exact: true }).click();
  await expect(first).toHaveValue('An unsaved browser draft.');
  await page.getByRole('button', { name: 'Redo', exact: true }).click();
  await expect(first).toHaveValue('Another edit.');
  await expect(page.getByRole('button', { name: 'Export DOCX', exact: true })).toBeDisabled();
  await page.getByRole('button', { name: 'Discard edits', exact: true }).click();
  await page.getByRole('searchbox', { name: 'Find in transcript', exact: true }).fill('saved');
  await page.getByRole('textbox', { name: 'Replacement text', exact: true }).fill('reviewed');
  await page.getByRole('button', { name: 'Replace all (case sensitive)', exact: true }).click();
  await expect(first).toHaveValue('A reviewed transcript for library search and editing.');
  await first.focus();
  await first.evaluate((element: HTMLTextAreaElement) => element.setSelectionRange(10, 10));
  await first.press('Control+Enter');
  await expect(page.locator('.transcript textarea')).toHaveCount(3);
  await page.getByRole('button', { name: 'Merge with next', exact: true }).first().click();
  await expect(page.locator('.transcript textarea')).toHaveCount(2);
  await page.getByRole('button', { name: 'Save edits', exact: true }).click();
  await expect(page.getByText('Transcript saved on this device.', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Export VTT', exact: true }).click();
  await expect(page.getByText('Transcript exported.', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Export DOCX', exact: true }).click();
  await expect(page.getByText('Exported.', { exact: true })).toBeVisible();
});

test('rename and organization controls render at the minimum window size', async ({ page }, info) => {
  await page.setViewportSize({ width: 760, height: 560 });
  await page.getByText('Rename & organize', { exact: true }).click();
  await page.getByRole('textbox', { name: 'Title', exact: true }).fill('Renamed interview');
  await page.getByRole('textbox', { name: 'Folder', exact: true }).fill('Interviews');
  await page.getByRole('textbox', { name: 'Tags (comma separated)', exact: true }).fill('research, checked');
  await page.getByRole('button', { name: 'Save organization', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Renamed interview', exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.screenshot({ path: info.outputPath('minimum-window.png'), fullPage: true });
});

test('model setup explains size/space and offers recovery after a mocked network failure', async ({ page }, info) => {
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const settings = page.getByRole('dialog', { name: 'Settings', exact: true });
  await expect(settings.getByText(/190 MB download · 200 MB free disk space/)).toBeVisible();
  await settings.getByRole('button', { name: 'Download recommended model', exact: true }).click();
  await expect(settings.getByText('Mocked network failure. Retry or import a model.', { exact: true })).toBeVisible();
  await expect(settings.getByRole('button', { name: 'Retry recommended model', exact: true })).toBeEnabled();
  await expect(settings.getByRole('button', { name: 'Import model', exact: true })).toBeEnabled();
  await page.screenshot({ path: info.outputPath('model-setup.png'), fullPage: true });
});

test('mixed-source capture exposes pause/resume, confirmed discard and stop/save', async ({ page }, info) => {
  await page.getByRole('combobox', { name: 'Recording source', exact: true }).selectOption('mixed');
  await page.getByRole('button', { name: 'Refresh applications / grant access', exact: true }).click();
  await page.getByRole('combobox', { name: 'Application to capture', exact: true }).selectOption('123');
  expect(await page.locator('.sidebar').evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  await page.getByRole('button', { name: '● Record audio', exact: true }).click();
  await page.getByRole('button', { name: 'Pause', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Recording paused', exact: true })).toBeVisible();
  await page.screenshot({ path: info.outputPath('paused-recording.png'), fullPage: true });
  await page.getByRole('button', { name: 'Resume', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Recording microphone + application audio', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Discard…', exact: true }).click();
  await expect(page.getByRole('group', { name: 'Discard this recording?', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Keep recording', exact: true }).click();
  await page.getByRole('button', { name: 'Stop and save', exact: true }).click();
  await expect(page.getByText('Recording saved on this device. Ready to transcribe.', { exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Transcribe offline', exact: true })).toBeVisible();
});

test('structured export selection and copying an unsaved draft work in the browser', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.getByRole('combobox', { name: 'Other export format', exact: true }).selectOption('json');
  await page.getByRole('button', { name: 'Export JSON', exact: true }).click();
  await expect(page.getByText('Transcript exported.', { exact: true })).toBeVisible();
  await page.getByRole('textbox', { name: 'Transcript at 0:00', exact: true }).fill('Unstored clipboard draft');
  await expect(page.getByRole('button', { name: 'Export JSON', exact: true })).toBeDisabled();
  await page.getByRole('button', { name: 'Copy transcript', exact: true }).click();
  await expect(page.getByText('Draft copied to clipboard. Your edits are still unsaved.', { exact: true })).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toContain('Alice: Unstored clipboard draft');
  await expect(page.getByRole('button', { name: 'Save edits', exact: true })).toBeEnabled();
});
