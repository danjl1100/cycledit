# Issue 03: `init_git` commits once per file line instead of once per date block

## Problem

In `tests/common/mod.rs`, `git commit` is called inside the file-operation loop, so
each `+path` or `-path` line produces its own commit. A date block with three files:

```
2001-05-22:
+file1.txt
+file2.txt
+file3.txt
```

produces **3 separate commits** rather than 1 commit containing all three files.

Current tests pass because all commits share the same date, so `list_files` still
returns the right dates. However:

- The semantics diverge from the documented format (a date block = one snapshot in time).
- A test that checks commit count or commit messages would fail unexpectedly.
- Staging all changes for a date and committing once is the natural git workflow and
  closer to real-world usage this tool is designed for.

## Fix

Restructure `init_git` to accumulate file operations within a date block, then call
`git add`/`git rm` for all of them and commit once at the end of the block (triggered
by the next date header or end of input).

## Test coverage

Existing tests should continue to pass unchanged. No new tests are required, but it
would be reasonable to add a test asserting that a two-file date block produces exactly
one commit (via `git log --oneline`) to prevent regression.
