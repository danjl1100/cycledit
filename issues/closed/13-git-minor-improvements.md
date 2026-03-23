# Issue 13: Minor code-quality improvements in `git.rs`

Two small, independent improvements to `src/git.rs`.  They can be implemented
together in a single commit.

## 1. Use `BTreeMap` for `file_dates` to eliminate the final sort

`file_dates` is currently a `HashMap`, and `entries` is sorted at the end with
`entries.sort_by(…)`.  Switching to a `BTreeMap` keeps entries in sorted order
automatically, removing the need for the explicit sort:

```rust
let mut file_dates: BTreeMap<String, (Date, ObjectId)> = BTreeMap::new();
// …
// No entries.sort_by(…) needed at the end.
```

Note: Issue 12 added a `HashSet` for `remaining`, and that can stay as `HashSet` (order
doesn't matter there).

## 2. Consider caching the parent commit's `walk_tree_blobs` result

For a linear history, commit N-1's current tree is commit N's parent tree.  The current
code calls `walk_tree_blobs` twice per commit: once for the commit's own tree, and once
for the parent's tree — but the parent's tree result is immediately discarded, only to
be recomputed as the current tree in the next iteration.

Evaluate whether caching the previous iteration's blob map and reusing it as the next
iteration's `parent_blobs` is feasible and produces a measurable improvement.
(see metrics added in closed issue 12)
If the history is not always linear (merge commits), handle the multi-parent case gracefully
(e.g. only cache a limited amount of parent commits).

Compare against using caching available in the `gix` crate, if not already enabled

This item is marked as a feasibility investigation; implement only if the approach is
clean and the benefit is clear.

## Files Affected

- `src/git.rs` — `list_files`
