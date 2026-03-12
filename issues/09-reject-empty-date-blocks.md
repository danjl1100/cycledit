# Issue 09: Reject empty date blocks in `init_git`

## Background

`TestHarness::init_git` currently passes `--allow-empty` to `git commit`, which allows
a date block with no file operations to silently produce a commit that changes nothing:

```
2001-05-22:
+file.txt

2037-11-29:
              ← no ops; still creates a commit
```

Such commits are meaningless for `cycledit`, which only cares about which files changed
on which dates. An empty block is almost certainly a mistake in the test's state string.

## Fix

After parsing blocks in `init_git`, assert that every block has at least one operation
before doing any git work:

```rust
for (date, ops) in &blocks {
    assert!(
        !ops.is_empty(),
        "init_git: date block {date:?} has no file operations — every commit must change files"
    );
}
```

## Scope

- One assertion added in `tests/integration/common.rs` after the parsing loop.
- Remove the `--allow-empty` flag from the `git commit` call (it becomes unreachable
  and its presence is misleading).
- Audit existing test state strings to confirm none rely on empty blocks.

## Notes

- This also simplifies the future gix migration (issue 08): same-tree commits
  (empty blocks) become an impossible case and need not be handled.
