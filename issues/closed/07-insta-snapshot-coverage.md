# Issue 07: Expand insta snapshot coverage; object ID stability concern is unfounded

## Background

Several tests use manual `assert!`/`assert_eq!` instead of `insta::assert_snapshot!`.
A concern was raised that "git object IDs are unstable" — but this is not applicable to
the current output format.

## Why object IDs are not unstable here

- Blob hashes are content-addressed. Test file content is always `"{date}:{path_str}"`
  (written in `init_git`), which is fully deterministic across runs.
- The CLI output (`list`, `schedule`, `now`, `check`) never includes commit hashes or
  blob hashes — only dates and file paths.
- Sort order for same-date files is by blob hash, which is deterministic given the
  above.

## Tests that could be converted to insta snapshots

| Test | Current approach | Snapshot benefit |
|------|-----------------|-----------------|
| `schedule_all_overdue_lands_in_today` | Checks date header count + file presence | Would catch exact chunk assignment and ordering |
| `schedule_overflow_to_next_chunk` | Checks file presence + date header count | Would pin exact chunk dates (2026-01-01, 2026-01-11) |
| `schedule_same_date_deterministic_order` | Compares two runs for equality | Could snapshot one run and use the snapshot as the stability reference |
| `current_time_zoned_drives_today` | `assert!(stdout.contains(...))` | Snapshot would verify exact output format |
| `list_error_not_in_git_repo` | `assert!(stderr.contains(...))` | Snapshot the exact error message |

## Notes

- `schedule_same_date_deterministic_order` currently runs `TestHarness` twice to verify
  determinism. With snapshots, the first run creates the reference; subsequent runs
  compare against it automatically. The second `TestHarness` invocation could be removed.
- Insta filter for the temp dir path (`[TEMPDIR]`) should already be in place from
  other snapshot tests; confirm it covers all cases.
