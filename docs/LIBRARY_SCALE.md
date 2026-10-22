# Library query scale validation

Measured October 20, 2026, using the opt-in `large_library_query_benchmark`
Rust test. This validates database queries and pagination, not end-to-end UI
responsiveness or a maximum supported library size.

## Reproduce

```sh
cargo test --manifest-path native/src-tauri/Cargo.toml large_library_query_benchmark -- --ignored --nocapture
```

The test creates and removes its own temporary on-disk SQLite WAL library. It
never opens the user's library. It inserts 10,000 synthetic recordings with
80,000 saved transcript paragraphs, title/folder/tag/status metadata, Unicode
text, and selective search markers. It asserts query counts, empty transcript
payloads in list summaries, full selected-recording text and speaker retrieval,
and stable traversal of 100 pages without duplicate or missing IDs.

No audio files are generated. Fixture population took 54.13 seconds; the database
occupied 83.65 MiB. Each query case ran 50 times after fixture creation, with a
warm filesystem/database cache. Timings are diagnostic output, not flaky CI
pass/fail thresholds. The default test suite skips this benchmark.

## Observed results

Apple M3, 8 GiB RAM, arm64 macOS 27.0.1; Rust 1.99.1 debug test profile.

| Query | p50 | p95 | Maximum |
| --- | ---: | ---: | ---: |
| First 100 recordings | 0.316 ms | 0.388 ms | 0.825 ms |
| Last page, offset 9,900 | 4.834 ms | 5.039 ms | 6.420 ms |
| Broad transcript search: discussion | 20.606 ms | 20.965 ms | 21.240 ms |
| Selective transcript search: marker09999 | 1.805 ms | 1.870 ms | 1.928 ms |
| Unicode search + status, folder and tag filters | 22.450 ms | 23.198 ms | 24.279 ms |

This closes a reproducible database-query evidence gap at this fixture size.
Native IPC, WebKit rendering, scrolling, draft preservation during search,
concurrent background transcription, cold-cache behavior, larger/longer
transcripts, audio disk usage, mutation throughput and slower devices remain
separate validation work. Do not infer a UI latency or capacity guarantee from
these numbers.
