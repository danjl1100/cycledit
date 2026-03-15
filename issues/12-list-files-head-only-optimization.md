# Issue 12: `list_files` — HEAD-only file set, pathspec pre-filtering, and walk metrics

## Problems

### 1. Deleted files appear in output

`list_files` walks all commits and records every file it has ever seen, including files
that were deleted before HEAD.  Only files present in the HEAD commit should be
returned.

`round_trip_add_and_remove` in `dump_fixture_tests.rs` currently exposes this:
`foo.txt` is removed in the second commit yet still appears in the `list` snapshot.
That test (and its snapshot) must be updated as part of this work.

### 2. Commit walk never exits early

Once every file in HEAD has been assigned a modification date, further commit
traversal is wasted work.  The walk should exit as soon as the set of "not yet dated"
files is empty.

### 3. Pathspec and exclude filters are applied after the full walk

A user passing `some/single/file.md` include pathspec still triggers a full walk over every
commit in history.  Filters should be applied to the HEAD file list _before_ the commit
walk begins, so only the files that will actually appear in the output are tracked.

### 4. No way to verify walk efficiency

There is no observability into how many `repo.find_object` calls are made during a
walk, making it impossible to verify that the optimizations above actually reduce work
or to catch future regressions.

## Proposed Changes

### Step 0 — ENV var for walk metrics (snapshot-tested)

Add an optional environment variable (e.g. `CYCLEDIT_LOG_METRICS=1`) that, when set,
prints to stderr a summary line such as:

```
metrics: find_object_calls=<N>
```

Write a snapshot test against a controlled fixture (ideally a multi-commit repo with
many files, several of which are deleted) that captures this current count before the change.
TestHarness likely needs a new chainable function to set this env var.
The snapshot acts as a baseline and future regression guard: the count should decrease
after the optimizations in the steps below and must not increase in future changes.

Make sure to commit just this test/env change separarely, to document the baseline metric
in the Git history for comparison in Step 4.


### Step 1 — Build the candidate set from HEAD

After resolving `head_id`, walk only the HEAD tree to collect the initial file set:

```rust
let head_tree_id = /* HEAD commit's tree id */;
let all_head_files: HashMap<String, ObjectId> = walk_tree_blobs(&repo, head_tree_id)?;
```
    NOTE: if 11-walk-tree-blobs-visitor-pattern.md was implemented (in issues/closed/..)
    then it changed `walk_tree_blobs` signature/behavior since this issue was created

Apply pathspec and exclude filters to `all_head_files` immediately.  Only the surviving
paths need modification dates.

### Step 2 — Track "undated" files; exit the commit walk early

```rust
let mut remaining: HashSet<String> = filtered_head_files.keys().cloned().collect();

for info in commits_walk {
    if remaining.is_empty() { break; }
    // ... existing per-commit logic ...
    // When a file's date is found, remove it from `remaining`.
}
```

### Step 3 — Skip the parent-tree walk once a file is already dated

If `file_dates.contains_key(path)` then the file was already assigned a date from a
newer commit; skip it without inspecting the parent tree.

### Step 4 — Update snapshot for ENV var walk metrics

Verify the metrics in the snapshot test from Step 0 have decreased by roughly the
expected amount. If no decrease is seen, re-examine the implementation, and only if
required, improve the prior commit that captures the baseline behavior (in case the Git
fixture test case wasn't diabolical enough).
The implementation commit will capture both the implementation and the metric
improvements via the updated snapshots.


## Test Updates

- `dump_fixture_tests.rs` — `round_trip_add_and_remove` currently expects `foo.txt`
  (a deleted file) to appear in `list` output.  After this fix, `foo.txt` must **not**
  appear.  Update the snapshot accordingly.
- Add a new fixture in `dump_fixture_tests.rs` (or a dedicated test module) that has a
  meaningful number of files, some deleted, to serve as the metrics baseline.

## Files Affected

- `src/git.rs` — `list_files`
- `tests/integration/dump_fixture_tests.rs` — snapshot update + new metrics fixture
