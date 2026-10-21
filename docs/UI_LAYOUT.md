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

Validation covers 64 frontend tests and 12 mocked-native Chromium flows, including
draft retention across sections and separate header/body/footer bounds at 760×560
and 1180×780. Screenshots are captured in dark mode at both sizes. These checks
exercise browser rendering with mocked native IPC; they do not validate native
file pickers, permission prompts, VoiceOver or macOS capture hardware. Existing
recording, editing, export and local-AI flows remain in the browser regression
suite. Workspace action grouping and native WebKit review remain subsequent work.
