# Issue 13: Minor code-quality improvements in `git.rs`

Three small, independent improvements to `src/git.rs`.  They can be implemented
together in a single commit.

## 1. Remove the intermediate `Vec` in the commit walk

`list_files` currently collects the entire rev-walk into a `Vec` before iterating:

```rust
let commits: Vec<_> = repo
    .rev_walk([head_id])
    .all()?
    .collect::<Result<Vec<_>, _>>()?;

for info in &commits { … }
```

The `Vec` is only used as an iterator; it allocates O(n) memory for all commit
metadata upfront.  Use the `Walk` iterator directly:

```rust
let commits = repo.rev_walk([head_id]).all()?;
for info in commits { … }
```

Propagate the per-item `Result` inline.

## 2. Use `BTreeMap` for `file_dates` to eliminate the final sort

`file_dates` is currently a `HashMap`, and `entries` is sorted at the end with
`entries.sort_by(…)`.  Switching to a `BTreeMap` keeps entries in sorted order
automatically, removing the need for the explicit sort:

```rust
let mut file_dates: BTreeMap<String, (Date, ObjectId)> = BTreeMap::new();
// …
// No entries.sort_by(…) needed at the end.
```

Note: if Issue 12 adds a `HashSet` for `remaining`, that can stay as `HashSet` (order
doesn't matter there).

## 3. Consider caching the parent commit's `walk_tree_blobs` result

For a linear history, commit N-1's current tree is commit N's parent tree.  The current
code calls `walk_tree_blobs` twice per commit: once for the commit's own tree, and once
for the parent's tree — but the parent's tree result is immediately discarded, only to
be recomputed as the current tree in the next iteration.

Evaluate whether caching the previous iteration's blob map and reusing it as the next
iteration's `parent_blobs` is feasible and produces a measurable improvement.  If the
history is not always linear (merge commits), handle the multi-parent case gracefully
(e.g. only cache for single-parent commits).

This item is marked as a feasibility investigation; implement only if the approach is
clean and the benefit is clear.

## Files Affected

- `src/git.rs` — `list_files`
