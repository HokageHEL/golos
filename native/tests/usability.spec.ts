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


test('batch import reports each failure, retains pending jobs and removes jobs without deleting recordings', async ({ page }, info) => {
  await page.getByRole('button', { name: 'Batch transcription', exact: true }).click();
  const queue = page.getByRole('dialog', { name: 'Batch transcription', exact: true });
  await queue.getByRole('combobox', { name: 'Batch audio language', exact: true }).selectOption('uk');
  await queue.getByRole('button', { name: 'Add files to queue', exact: true }).click();
  await expect(queue.getByText('2 recordings imported and queued.', { exact: true })).toBeVisible();
  await expect(queue.getByText('broken.mp3: Mocked decode failure', { exact: true })).toBeVisible();
  await expect(queue.getByRole('button', { name: 'Start / resume queue', exact: true })).toBeDisabled();
  await queue.getByRole('button', { name: 'Remove Mock interview A from queue', exact: true }).click();
  await expect(queue.getByText('Mock interview A', { exact: true })).toHaveCount(0);
  await queue.getByRole('button', { name: 'Close batch queue', exact: true }).click();
  await expect(page.getByRole('button', { name: /Mock interview A.*Ready/ })).toBeVisible();
  await page.getByRole('button', { name: 'Batch transcription', exact: true }).click();
  await expect(queue.getByText('Mock interview B', { exact: true })).toBeVisible();
  await page.setViewportSize({ width: 760, height: 560 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: info.outputPath('batch-queue.png'), fullPage: true });
});


test('mocked native drop previews names, discards explicitly and imports only after Add', async ({ page }, info) => {
  await page.getByRole('button', { name: 'Batch transcription', exact: true }).click();
  const queue = page.getByRole('dialog', { name: 'Batch transcription', exact: true });
  // Explicitly mocked native drag/drop event: no Finder, WebKit drag or file read.
  await page.evaluate(async () => {
    const bridge = window as unknown as { mockNativeBatchDrop: (id: string, names: string[]) => Promise<void> };
    await bridge.mockNativeBatchDrop('drop-1', ['Dropped A.wav', 'Dropped B.wav']);
  });
  await expect(queue.getByText('2 dropped files ready.', { exact: true })).toBeVisible();
  await expect(queue.getByText('Dropped A.wav', { exact: true })).toBeVisible();
  await expect(queue.getByText('Dropped B.wav', { exact: true })).toBeVisible();
  await expect(queue.getByText(/0 queued/)).toBeVisible();
  await queue.getByRole('button', { name: 'Discard selection', exact: true }).click();
  await expect(queue.getByText('Dropped A.wav', { exact: true })).toHaveCount(0);
  await page.evaluate(async () => {
    const bridge = window as unknown as { mockNativeBatchDrop: (id: string, names: string[]) => Promise<void> };
    await bridge.mockNativeBatchDrop('drop-2', ['Dropped A.wav', 'Dropped C.wav']);
  });
  await queue.getByRole('combobox', { name: 'Batch audio language', exact: true }).selectOption('uk');
  await queue.getByRole('button', { name: 'Add dropped files', exact: true }).click();
  await expect(queue.getByText('2 recordings imported and queued. 1 duplicate file skipped within this selection.', { exact: true })).toBeVisible();
  await expect(queue.getByText('broken-drop.mp3: Mocked decode failure', { exact: true })).toBeVisible();
  await expect(queue.getByText('Mock dropped interview A', { exact: true })).toBeVisible();
  await page.setViewportSize({ width: 760, height: 560 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.screenshot({ path: info.outputPath('batch-drop-import.png'), fullPage: true });
});


test('watched-folder and local export controls preserve opt-in choices and retry failed output', async ({ page }, info) => {
  await page.getByRole('button', { name: 'Folders & automation', exact: true }).click();
  const automation = page.getByRole('dialog', { name: 'Folders & automation', exact: true });
  await automation.getByRole('combobox', { name: 'New folder language', exact: true }).selectOption('uk');
  await automation.getByRole('checkbox', { name: 'Transcribe automatically', exact: true }).check();
  await expect(automation.getByRole('checkbox', { name: 'Include files already in the folder', exact: true })).not.toBeChecked();
  await automation.getByRole('button', { name: 'Add watched folder', exact: true }).click();
  await expect(automation.getByText('/mock/interviews', { exact: true })).toBeVisible();
  await automation.getByRole('button', { name: 'Pause folder', exact: true }).click();
  await expect(automation.getByRole('button', { name: 'Resume folder', exact: true })).toBeVisible();
  await automation.getByRole('button', { name: 'Pause automation', exact: true }).click();
  await expect(automation.getByRole('button', { name: 'Resume automation', exact: true })).toBeVisible();
  await automation.getByRole('button', { name: 'Local exports Saved transcripts', exact: true }).click();
  await automation.getByRole('button', { name: 'Choose export folder', exact: true }).click();
  await automation.getByRole('checkbox', { name: 'VTT', exact: true }).check();
  await automation.getByRole('checkbox', { name: 'Export automatically after transcription', exact: true }).check();
  await automation.getByRole('button', { name: 'Save export settings', exact: true }).click();
  await expect(automation.getByText('Export settings saved on this device.', { exact: true })).toBeVisible();
  await automation.getByRole('button', { name: 'Export completed queue results', exact: true }).click();
  await expect(automation.getByText('Mocked export permission error. Restore access and retry.', { exact: true })).toBeVisible();
  await automation.getByRole('button', { name: 'Retry failed exports', exact: true }).click();
  await expect(automation.getByText('pending', { exact: true })).toBeVisible();
  await page.setViewportSize({ width: 760, height: 560 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await automation.evaluate(element => { element.scrollTop = 0; });
  await page.screenshot({ path: info.outputPath('automation-controls.png'), fullPage: true });
  await automation.getByRole('button', { name: 'Save export settings', exact: true }).scrollIntoViewIfNeeded();
  await page.screenshot({ path: info.outputPath('automation-exports.png'), fullPage: true });
});

test('automatic AI submenus retain drafts while saving rules and controlling queued work', async ({ page }, info) => {
  await page.getByRole('button', { name: 'Folders & automation', exact: true }).click();
  const automation = page.getByRole('dialog', { name: 'Folders & automation', exact: true });
  await automation.getByRole('button', { name: 'AI rules Opt-in processing', exact: true }).click();
  await expect(automation.getByRole('heading', { name: 'Automatic AI rules', exact: true })).toBeVisible();
  await expect(automation.getByText(/No rules are configured/)).toBeVisible();
  await automation.getByRole('button', { name: 'Add automatic rule', exact: true }).click();
  await automation.getByLabel('Rule name', { exact: true }).fill('Browser meeting summary');
  await automation.getByRole('checkbox', { name: 'Single recording', exact: true }).check();
  await automation.getByRole('checkbox', { name: 'Enable rule 1', exact: true }).check();
  await automation.getByRole('button', { name: 'Save automatic AI rules', exact: true }).click();
  await expect(automation.getByText(/Automatic AI rules saved/)).toBeVisible();
  await automation.getByLabel('Rule name', { exact: true }).fill('Unsaved browser rule draft');
  await automation.getByRole('button', { name: 'AI model Local service', exact: true }).click();
  await automation.getByLabel('Context window', { exact: true }).selectOption('16384');
  await automation.getByRole('button', { name: 'Refresh installed models', exact: true }).click();
  await expect(automation.getByText('Installed local models loaded.', { exact: true })).toBeVisible();
  await automation.getByRole('button', { name: 'AI tasks Queue & results', exact: true }).click();
  await expect(automation.getByLabel('Status of Research interview', { exact: true })).toHaveText('failed');
  await automation.getByRole('button', { name: 'Retry automatic task Research interview', exact: true }).click();
  await expect(automation.getByLabel('Status of Research interview', { exact: true })).toHaveText('queued');
  await automation.getByRole('button', { name: 'Cancel automatic AI and pause', exact: true }).click();
  await expect(automation.getByRole('button', { name: 'Resume automation', exact: true })).toBeVisible();
  await automation.getByRole('button', { name: 'Skip all queued automatic tasks', exact: true }).click();
  await expect(automation.getByLabel('Status of Research interview', { exact: true })).toHaveText('skipped');
  await expect(automation.getByText(/1 queued automatic task skipped, including tasks outside this list/)).toBeVisible();
  await expect(automation.getByRole('button', { name: 'Skip all queued automatic tasks', exact: true })).toBeDisabled();
  await automation.getByRole('button', { name: 'AI rules Opt-in processing', exact: true }).click();
  await expect(automation.getByLabel('Rule name', { exact: true })).toHaveValue('Unsaved browser rule draft');
  await automation.getByRole('button', { name: 'AI model Local service', exact: true }).click();
  await expect(automation.getByLabel('Context window', { exact: true })).toHaveValue('16384');
  await page.setViewportSize({ width: 760, height: 560 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.screenshot({ path: info.outputPath('automatic-ai-model.png'), fullPage: true });
});

test('local AI keeps summaries separate, exports results and retains saved history', async ({ page }, info) => {
  await page.getByRole('button', {name:'Local AI & translation',exact:true}).click();
  const dialog=page.getByRole('dialog',{name:'Local AI & translation'});
  await expect(dialog).toBeVisible();
  await dialog.getByRole('button',{name:'Generate locally',exact:true}).click();
  await expect(dialog.getByText('Local summary [s1].',{exact:true})).toBeVisible();
  await expect(dialog.getByRole('button',{name:'Use as transcript draft'})).toBeDisabled();
  await dialog.getByRole('button',{name:'Export result',exact:true}).click();
  await expect(dialog.getByText('Generated result exported.',{exact:true})).toBeVisible();
  await dialog.getByRole('button',{name:'Export result DOCX',exact:true}).click();
  await expect(dialog.getByText('Generated DOCX exported.',{exact:true})).toBeVisible();
  await dialog.getByRole('button',{name:'Close local AI'}).click();
  await expect(page.getByRole('textbox',{name:'Transcript at 0:00',exact:true})).toHaveValue('A saved transcript for library search and editing.');
  await page.getByRole('button',{name:'Local AI & translation',exact:true}).click();
  await dialog.getByRole('button',{name:'Summary',exact:true}).click();
  await expect(dialog.getByText('Local summary [s1].',{exact:true})).toBeVisible();
  await page.setViewportSize({width:760,height:560});
  await expect(dialog).toBeVisible();
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth)).toBe(true);
  await page.screenshot({path:info.outputPath('local-ai-summary.png'),fullPage:true});
});

test('local cleanup creates an undoable unsaved draft and dirty drafts block generation', async ({ page }) => {
  const editor=page.getByRole('textbox',{name:'Transcript at 0:00',exact:true});
  await editor.fill('My private unsaved changes.');
  await page.getByRole('button',{name:'Local AI & translation',exact:true}).click();
  let dialog=page.getByRole('dialog',{name:'Local AI & translation'});
  await expect(dialog.getByRole('button',{name:'Generate locally',exact:true})).toBeDisabled();
  await expect(dialog.getByText(/Save your transcript edits before generating/)).toBeVisible();
  await dialog.getByRole('button',{name:'Close local AI'}).click();
  await expect(editor).toHaveValue('My private unsaved changes.');
  await page.getByRole('button',{name:'Discard edits',exact:true}).click();
  await page.getByRole('button',{name:'Local AI & translation',exact:true}).click();
  dialog=page.getByRole('dialog',{name:'Local AI & translation'});
  await dialog.getByRole('combobox',{name:'Local task',exact:true}).selectOption('cleanup');
  await dialog.getByRole('button',{name:'Generate locally',exact:true}).click();
  await expect(dialog.getByRole('button',{name:'Use as transcript draft'})).toBeEnabled();
  await dialog.getByRole('button',{name:'Use as transcript draft'}).click();
  await expect(editor).toHaveValue('Cleaned browser transcript.');
  await expect(page.getByRole('button',{name:'Save edits',exact:true})).toBeEnabled();
  await page.getByRole('button',{name:'Undo',exact:true}).click();
  await expect(editor).toHaveValue('A saved transcript for library search and editing.');
});

test('translation preview retains metadata and creates a separate recording without altering the source',async({page})=>{
  await page.getByRole('button',{name:'Local AI & translation',exact:true}).click();
  const dialog=page.getByRole('dialog',{name:'Local AI & translation'});
  await dialog.getByRole('combobox',{name:'Local task'}).selectOption('translate');
  await expect(dialog.getByRole('button',{name:'Generate locally',exact:true})).toBeDisabled();
  await dialog.getByRole('textbox',{name:'Target language (required)'}).fill('Ukrainian');
  await dialog.getByRole('button',{name:'Generate locally',exact:true}).click();
  const preview=dialog.getByLabel('Generated paragraphs');
  await expect(preview.getByText('0:00–0:10 · Alice',{exact:true})).toBeVisible();
  await dialog.getByRole('combobox',{name:'Result export format'}).selectOption('vtt');
  await dialog.getByRole('button',{name:'Export result',exact:true}).click();
  await expect(dialog.getByText('Generated result exported.',{exact:true})).toBeVisible();
  await dialog.getByRole('button',{name:'Create separate recording',exact:true}).click();
  await expect(page.getByRole('heading',{name:'[Ukrainian] Research interview',exact:true})).toBeVisible();
  await expect(page.getByRole('textbox',{name:'Transcript at 0:00',exact:true})).toHaveValue('Cleaned browser transcript.');
  await page.getByRole('button',{name:/^Research interview.*Transcribed/}).click();
  await expect(page.getByRole('textbox',{name:'Transcript at 0:00',exact:true})).toHaveValue('A saved transcript for library search and editing.');
  await page.getByRole('button',{name:'Local AI & translation',exact:true}).click();
  await dialog.getByRole('button',{name:'Translate transcript · Ukrainian',exact:true}).click();
  await dialog.getByRole('button',{name:'Retry captured source',exact:true}).click();
  await expect(dialog.getByRole('button',{name:'Translate transcript · Ukrainian',exact:true})).toHaveCount(2);
  await dialog.getByRole('button',{name:'Delete result',exact:true}).click();
  await dialog.getByRole('button',{name:'Keep result',exact:true}).click();
  await expect(dialog.getByRole('button',{name:'Translate transcript · Ukrainian',exact:true})).toHaveCount(2);
});

test('settings submenus keep navigation and footer clear at minimum and full window sizes', async ({ page }, info) => {
  async function checkShell(dialog: ReturnType<typeof page.getByRole>) {
    const header = await dialog.locator('.settings-header').boundingBox();
    const body = await dialog.locator('.settings-workspace-body').boundingBox();
    const footer = await dialog.locator('.settings-footer').boundingBox();
    expect(header).not.toBeNull(); expect(body).not.toBeNull(); expect(footer).not.toBeNull();
    expect(header!.y + header!.height).toBeLessThanOrEqual(body!.y + 1);
    expect(body!.y + body!.height).toBeLessThanOrEqual(footer!.y + 1);
    expect(footer!.y + footer!.height).toBeLessThanOrEqual(page.viewportSize()!.height);
    expect(await dialog.locator('.settings-page-content').evaluate(element => element.scrollWidth <= element.clientWidth + 1)).toBe(true);
  }
  for (const size of [{ width: 760, height: 560 }, { width: 1180, height: 780 }]) {
    await page.setViewportSize(size);
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    const settings = page.getByRole('dialog', { name: 'Settings', exact: true });
    for (const label of ['Appearance Theme', 'Transcription Offline model', 'Speakers Optional models']) {
      const navigation = settings.getByRole('button', { name: label, exact: true });
      await navigation.click(); await expect(navigation).toHaveAttribute('aria-current', 'page');
      await checkShell(settings);
    }
    await settings.getByRole('button', { name: 'Appearance Theme', exact: true }).click();
    await settings.getByRole('radio', { name: 'Dark Easy on the eyes', exact: true }).check();
    await checkShell(settings);
    await page.screenshot({ path: info.outputPath(`settings-dark-${size.width}.png`), fullPage: true });
    await settings.getByRole('button', { name: 'Done', exact: true }).click();
    await page.getByRole('button', { name: 'Folders & automation', exact: true }).click();
    const automation = page.getByRole('dialog', { name: 'Folders & automation', exact: true });
    for (const label of ['Watched folders Import & transcribe', 'Local exports Saved transcripts', 'AI rules Opt-in processing', 'AI model Local service', 'AI tasks Queue & results']) {
      const navigation = automation.getByRole('button', { name: label, exact: true });
      await navigation.click(); await expect(navigation).toHaveAttribute('aria-current', 'page');
      await checkShell(automation);
    }
    await automation.getByRole('button', { name: 'AI rules Opt-in processing', exact: true }).click();
    await page.screenshot({ path: info.outputPath(`automation-dark-${size.width}.png`), fullPage: true });
    await automation.getByRole('button', { name: 'Close automation settings', exact: true }).click();
  }
});
