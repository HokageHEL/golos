# Interface consistency

Settings and automation use the same navigation, spacing, page area and footer.
Each task has a focused page rather than a single long form:

- Settings: Appearance, Transcription, Speakers.
- Automation: Watched folders, Local exports, AI rules, AI model, AI tasks.

Only the selected page is shown. Pages remain mounted, so switching sections
preserves unsaved rule, model, watched-folder and export choices. Polling still
refreshes task status without replacing drafts. Selecting a page resets its scroll
position; it does not save settings or trigger model work.

The header, close control, navigation and footer remain outside the scrolling
content. Long paths and service instructions wrap inside the page. Narrow layouts
use a horizontally scrolling section bar. Buttons change theme colors immediately,
avoiding transient low-contrast text while backgrounds animate between themes.

## Workspace

The library and the open recording follow the same rule: what people use often
stays on screen, and everything else opens when it is needed.

Always visible:

- Sidebar: **Record audio**, **Import recording** and the recording source.
- Recording: the title, **Rename & organize**, playback, the transcript, undo/redo
  and the unsaved-edit notice with **Discard edits** and **Save edits**.
- Library: the search box and the recording list.

In-flow disclosures (native `<details>`, so the browser's own keyboard and
focus behaviour applies):

- **Export** — transcript formats, the other-format selector, audio, project
  backups and copy. It replaces the row of eight export buttons that used to sit
  in the transcript toolbar.
- **Find & replace** — the transcript search, match navigation and replacement.
- **Speaker detection** — the speaker count and **Detect speakers**.
- **More tools** — batch transcription, folders & automation and project import.
- **Filters** — status, folder and tag filters.
- **System audio & permissions** — only shown for a system or mixed source, and
  opened automatically, so permission recovery is never hidden.

Every disclosure expands in the normal document flow. Nothing is positioned over
the page, so a panel can never cover the player, the transcript or the editor.
Escape closes a disclosure and returns focus to its summary. Sections keep their
contents mounted, so a search query, a replacement text, a speaker count and the
recording source survive closing and reopening a section, and drafts in other
recordings are untouched. Unsaved-edit warnings and **Save edits** are never
hidden behind a disclosure.

The recording actions menu (right-click, Shift+F10 or the **⋯** button) opens
inside its own library row instead of floating over the workspace. It closes with
Escape and returns focus to the button that opened it.

## Validation

Validation covers 69 frontend tests and 13 mocked-native Chromium flows, including
draft retention across sections and separate header/body/footer bounds at 760×560
and 1180×780. Screenshots are captured in dark mode at both sizes. These checks
exercise browser rendering with mocked native IPC; they do not validate native
file pickers, permission prompts, VoiceOver or macOS capture hardware. Existing
recording, editing, export and local-AI flows remain in the browser regression
suite. Browser fixtures use a stable per-checkout loopback port, strict binding
and no server reuse, so parallel worktrees cannot silently validate another
checkout. Set `GOLOS_UI_PORT` to choose a different unused port explicitly.

The packaged macOS app's Settings navigation, speaker page and automation pages
were also inspected through native accessibility and screenshots at the normal
1180×780 window size, using an isolated app identifier and public fixtures. Page
switching kept the navigation, close control and footer clear. Minimum-size
geometry is covered by Chromium above; native minimum-size/VoiceOver and native workspace review remain subsequent work.

Review follow-up adds dialog-boundary and navigation/content separation checks,
including the 480-pixel browser-only layout (below the native minimum width).
Navigation names stay stable when visual hints are hidden. Named page regions and
a polite page-title announcement expose section changes without moving keyboard
focus away from navigation. A regression check waits for a real polling cycle and
confirms rule/model/folder/export drafts still survive switching sections. Pause
guidance names the footer's Resume control explicitly.
