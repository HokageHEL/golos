# Local audio recovery

Audio operations use owned UUID directories under `audio/.operations/`. This
does not change SQLite schema version 1 or rewrite existing recordings.

For import, the decoder writes `import-<id>/audio.wav`. Golos flushes that file
and its directory, commits the note to SQLite, then publishes the WAV under its
normal name with a hard link that cannot overwrite an existing destination.
Before the database commit, restart discards staged audio. After the commit,
restart publishes the completed file if necessary and removes staging.

For deletion, Golos moves the WAV to `delete-<id>/audio.wav` before deleting the
database row. If that transaction fails or the process stops before it commits,
recovery restores the audio. Once the row is gone, recovery removes staged audio.
An existing transcript whose audio was already missing remains usable; an
explicit deletion can still remove that entry.

Startup processes only recognized operation names. It does not sweep arbitrary
audio files. Recovery rejects links and refuses to overwrite a destination whose
contents differ from staging. If recovery cannot safely finish, startup reports
the error and retains the data for repair; do not delete the journal to silence
an error without first backing up the library directory.

The tests reopen libraries at import/delete boundaries, repeat recovery, inject
write and SQLite failures, and check preservation of unrelated files and links.
They simulate process interruption rather than physically cutting power.
Directory flushing is implemented on Unix; Windows uses flushed file contents
and unique operation names. The protocol requires hard-link support on the
application data filesystem. Actual Windows/Linux crash, disk-full, and device
validation remain release checks.
