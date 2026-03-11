# cycledit Implementation Plan

## Context
`cycledit` is a stateless CLI tool that queries a Git repository's commit history to
schedule regular file edits (e.g. password rotations). The README defines the full spec;
this plan implements it using TDD phasing — integration tests committed before the
implementation that makes them pass. `clap` is used for CLI argument parsing (not listed
in the README but standard; confirmed by user). Overdue files are clamped to today as
their earliest chunk date (confirmed by user).

---

## Dependencies to add to `Cargo.toml`

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
jiff = "0.2"
gix = { version = "0.70", default-features = false, features = ["revision", "index"] }

[dev-dependencies]
insta = { version = "1", features = ["filters"] }
```

> Exact gix feature set may need adjustment during implementation; start minimal and add
> as needed.

---

## Module / File Structure

```
src/
  lib.rs          # pub mod re-exports; no logic
  git.rs          # Git operations via gix (readonly)
  schedule.rs     # Scheduling algorithm (pure, no I/O)
  bin/
    cycledit.rs   # CLI entry point using clap

tests/
  common/
    mod.rs        # TestHarness struct (git CLI + process::Command)
  list_tests.rs
  schedule_tests.rs
  now_check_tests.rs
```

---

## Key Design Details

### `FileEntry` (in `src/git.rs`)
```rust
pub struct FileEntry {
    pub date: jiff::civil::Date,
    pub blob_hash: gix::ObjectId,
    pub path: std::path::PathBuf,
}
```

### `list` output (sorted lexicographically by path)
```
YYYY-MM-DD FILEPATH
```

### Scheduling algorithm (in `src/schedule.rs`)
```rust
// max_per_chunk = ceil(chunk_days / cycle_days), relative to a reference date
// items sorted by (date ASC, blob_hash ASC)
// for each item:
//   earliest = item.date + cycle_duration
//   chunk_date = max(earliest, today)   // clamp: overdue → today
//   while chunk_map[chunk_date].len() >= max_per_chunk:
//       chunk_date += chunk_duration
//   chunk_map[chunk_date].push(item)
```

Chunk size: compute both durations in calendar days (from a fixed reference point such as
today) then do integer ceiling division.

### `schedule` output
```
YYYY-MM-DD:
\tFILEPATH
\tFILEPATH
```
(tab-indented filepaths under each chunk date)

### `now` = schedule filtered to chunk_date ≤ today
### `check` output & exit codes
- Exit 0 → `PASS: All files up to date`
- Exit 100 → `WARN: Need to update N file(s) now (of M files total)`

### `CURRENT_TIME_ZONED` env var
Binary reads this env var (if set) to override "now" using `jiff::Zoned::parse()`.
Format from README: `YYYY-MM-DDTHH:MM:SS-OFFSET[TZ]`.

### "Not in a git repo" error
`gix::open()` failure → print helpful error to stderr, exit non-zero.

---

## TestHarness (tests/common/mod.rs)

Uses `std::process::Command` to drive the `git` binary (simpler and more reliable than
gix write API for test setup):

```rust
pub struct TestHarness {
    dir: tempfile::TempDir,
}

impl TestHarness {
    pub fn new() -> Self { /* create tempdir */ }

    // Parse "YYYY-MM-DD:\n+path\n-path\n..." blocks and run:
    //   git init / git add / git rm / GIT_COMMITTER_DATE=... git commit
    pub fn init_git(mut self, state: &str) -> Self { ... }

    // Run the cycledit binary with:
    //   TZ=UTC, CURRENT_TIME_ZONED=<time>, args as given
    // Returns CommandOutput { stdout, stderr, status }
    pub fn run_cli(self, time: &str, args: &[&str]) -> CommandOutput { ... }
}
```

Test git state format (from README):
```
2001-05-22:
+folder1/sub/file1.txt
+root-file.txt

2037-11-29:
-root-file.txt
+file2.txt
```

Insta snapshots used for stdout/stderr assertions. Set `insta` filter to normalize the
temp dir path if it appears in output.

---

## Implementation Phases (Git commits)

### Phase 1 — Project setup (1 commit)
- Add dependencies to `Cargo.toml`
- Remove placeholder code from `src/lib.rs` and `src/bin/cycledit.rs`
- Create module stubs (`git.rs`, `schedule.rs`, module declarations in `lib.rs`)
- Create `tests/common/mod.rs` with the `TestHarness` + `CommandOutput` types

### Phase 2a — `list` tests (1 commit, tests expected to fail)
**File:** `tests/list_tests.rs`
- Basic: single file → correct date + path
- Multiple files sorted lexicographically
- Pathspec filtering (include subset)
- `--exclude` filtering
- Error: not in a git repo

### Phase 2b — `list` implementation (1 commit, tests pass)
**Files:** `src/git.rs`, `src/bin/cycledit.rs`
- `git.rs`: open repo with gix, iterate index entries matching pathspecs/excludes,
  walk commits from HEAD to find most-recent-commit date for each file
- `bin/cycledit.rs`: clap struct with `list` subcommand + LIST_ARGS; format and print

### Phase 3a — `schedule` tests (1 commit, failing)
**File:** `tests/schedule_tests.rs`
- All files overdue → all land in today's first available chunks
- Files with future modification+cycle date → scheduled in future
- Custom `--cycle` and `--chunk` args
- Same-date tiebreaking by blob hash (deterministic order)

### Phase 3b — `schedule` implementation (1 commit, passing)
**Files:** `src/schedule.rs`, `src/bin/cycledit.rs`
- `schedule.rs`: `compute_schedule(entries, cycle, chunk, today)` → `BTreeMap<Date, Vec<FileEntry>>`
- `bin/cycledit.rs`: `schedule` subcommand; parse `--cycle`/`--chunk` with jiff span
  parser; format output

### Phase 4a — `now` and `check` tests (1 commit, failing)
**File:** `tests/now_check_tests.rs`
- `now`: only past/today chunks shown
- `now`: nothing due → empty output
- `check`: files due → WARN + exit 100
- `check`: nothing due → PASS + exit 0
- Confirm `CURRENT_TIME_ZONED` drives "today"

### Phase 4b — `now` and `check` implementation (1 commit, passing)
**File:** `src/bin/cycledit.rs`
- `now` subcommand: run schedule, filter to `chunk_date ≤ today`, print same format
- `check` subcommand: collect due files, print WARN/PASS, set exit code

---

## Verification
```bash
cargo test                          # all integration tests pass
cargo clippy -- -D warnings         # no warnings
cargo fmt --check                   # formatted
cargo run -- list                   # smoke test in a real git repo
cargo run -- schedule               # smoke test
cargo run -- now                    # smoke test
cargo run -- check; echo "exit: $?" # smoke test with exit code
```
