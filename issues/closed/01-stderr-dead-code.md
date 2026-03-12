# Issue 01: `CommandOutput.stderr` dead code

## Problem

`CommandOutput.stderr` is captured from every `cycledit` invocation in the test harness
but is never read in any test. This produces a `dead_code` compiler warning.

The one test that checks stderr (`list_error_not_in_git_repo`) bypasses `TestHarness`
entirely and reads `output.stderr` directly from its own inline `Command::output()` call.

## Options

**A) Remove `stderr` from `CommandOutput`**
Drop the field and the capture. The inline test in `list_tests.rs` already manages its
own stderr check independently.

**B) Use `TestHarness` in `list_error_not_in_git_repo` and assert stderr there**
Extend `run_cli` (or add a variant) that accepts a non-git directory, and write the
not-in-git-repo test via the harness so `stderr` is actually exercised.

**C) Keep the field, suppress the warning**
Add `#[allow(dead_code)]` to the field. Pragmatic but leaves the underlying issue unaddressed.

## Recommendation

Option B — the not-in-git-repo error path is worth asserting through the harness like
every other test, and it would make `stderr` useful rather than vestigial.
