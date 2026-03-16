# Issue 11: Refactor `walk_tree_blobs` to a generic visitor pattern

## Background

`walk_tree_blobs` in `src/git.rs` currently always builds and returns a full
`HashMap<String, ObjectId>`.  Every caller then iterates that map.  Two problems arise
from this design:

1. There is no way for a caller to short-circuit the walk (e.g. stop once all known
   paths have been matched), so unnecessary `repo.find_object` calls are made.
2. There is no hook to skip entire sub-trees, so a caller filtering to a specific
   directory still pays the full traversal cost.

## Proposed Change

Replace the concrete function with a generic visitor-based API.

### `TreeVisitor` trait

Two design options are presented; choose one during implementation.

**Option A — `Break` carries no data:**

```rust
trait TreeVisitor {
    /// Return `Continue(true)` to descend into the directory,
    /// `Continue(false)` to skip it, or `Break(())` to abort the walk.
    fn is_include_dir(&mut self, prefix: &str, name: &str) -> ControlFlow<(), bool>;

    /// Called for each blob entry.  Return `Break(())` to abort the walk.
    fn visit_blob(&mut self, prefix: &str, name: &str, object_id: ObjectId) -> ControlFlow<()>;
}
```

`walk_tree_blobs` return type becomes `eyre::Result<()>`.

Early exit pattern inside the walk:

```rust
let ControlFlow::Continue(include) = visitor.is_include_dir(prefix, name) else {
    return Ok(());
};
```

**Option B — `Break` may carry caller-defined data:**

```rust
trait TreeVisitor {
    type Break;
    fn is_include_dir(&mut self, prefix: &str, name: &str)
        -> ControlFlow<Self::Break, bool>;
    fn visit_blob(&mut self, prefix: &str, name: &str, object_id: ObjectId)
        -> ControlFlow<Self::Break>;
}
```

`walk_tree_blobs` return type becomes `eyre::Result<ControlFlow<(), T::Break>>`.

Early exit pattern:

```rust
let include = match visitor.is_include_dir(prefix, name) {
    ControlFlow::Continue(v) => v,
    ControlFlow::Break(b) => return Ok(ControlFlow::Break(b)),
};
```

Option B provides more flexibility (callers can return data on early exit) at the cost
of a more complex signature.  Verify which option is the better fit for the callers
inside `list_files` before committing to one.

### Walk behaviour changes

Inside the walk loop:

- Only push a sub-tree onto the stack when `visitor.is_include_dir(prefix, name)`
  returns `Continue(true)`.  This avoids allocating a new path string for skipped trees.
- Call `visitor.visit_blob(prefix, name, entry.object_id())` for each blob entry.

### Existing callers

Two functions each call `walk_tree_blobs` twice per commit (once for the commit's tree,
once for the parent's tree):

- `list_files` in `src/git.rs`
- `dump_fixture_string` in `src/fixture.rs`

Wrap a concrete visitor struct that collects into a `HashMap<String, ObjectId>` to
preserve the existing semantics for both callers while enabling future callers to be
more selective.

When issue 12 (`list_files` HEAD-only optimization) is implemented, `list_files` will
gain a third call site — walking only the HEAD tree to build the initial candidate set
before the commit walk begins.  The visitor API introduced here is what makes that
call efficient (the caller can skip sub-trees or exit early instead of collecting
everything into a `HashMap`).

## Test Coverage

- Add at least one test (or adapt an existing one in `dump_fixture_tests.rs`) that
  exercises sub-directory scanning — a fixture with files nested under one or more
  sub-directories.  This provides regression coverage for the visitor's `is_include_dir`
  path and validates that deep paths are reported correctly.

## Files Affected

- `src/git.rs` — `walk_tree_blobs`, `list_files`
- `src/fixture.rs` — `dump_fixture_string`
- `tests/integration/dump_fixture_tests.rs` — new/adapted sub-tree test
