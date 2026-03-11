# Issue 04: `find_repo_root` doesn't handle bare repositories

## Problem

`cycledit.rs::find_repo_root` walks up the directory tree looking for a `.git` entry:

```rust
if dir.join(".git").exists() {
    return Ok(dir.to_path_buf());
}
```

This misses:
- **Bare repos** (no `.git` subdirectory; the repo *is* the directory)
- **Git worktrees** (`.git` is a file, not a directory, pointing at the worktree metadata)
- **`$GIT_DIR` overrides** (arbitrary location set by environment variable)

`gix::open()` already handles all of these cases correctly. The custom traversal is
redundant and more restrictive than gix's own discovery.

## Fix

Replace `find_repo_root` entirely with `gix::discover(cwd)`, which returns the
discovered repository and handles all the above cases. Then pass `repo.path()` (or
`repo.work_dir()`) to `list_files`, or restructure `list_files` to accept a
pre-opened `gix::Repository` instead of a path.

## Tradeoffs

- Removing `find_repo_root` simplifies the binary but requires a small signature change
  to `list_files` (or introducing a separate discovery call in each subcommand arm).
- The error message from `gix::discover` differs from the current hand-rolled message;
  should verify it still matches the assertion in `list_error_not_in_git_repo`.
