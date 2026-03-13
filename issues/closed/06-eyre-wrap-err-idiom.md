# Issue 06: Use `.wrap_err()` instead of `map_err(|e| eyre::eyre!(...))`

## Problem

Every error site in `src/git.rs` and `src/bin/cycledit.rs` follows this pattern:

```rust
.map_err(|e| eyre::eyre!("failed to resolve HEAD: {e}"))?
```

This discards the original error chain — the source error is stringified and embedded
in a new root-level message, losing the type and any nested causes. The idiomatic eyre
approach is `.wrap_err("context")` from the `eyre::WrapErr` trait, which preserves the
original error as a linked source:

```rust
// before
repo.head_id()
    .map_err(|e| eyre::eyre!("failed to resolve HEAD: {e}"))?

// after
repo.head_id()
    .wrap_err("failed to resolve HEAD")?
```

The output in error messages becomes:
```
failed to resolve HEAD

Caused by:
  <original gix error>
```

## Scope

All `map_err(|e| eyre::eyre!("...: {e}"))` calls in `src/git.rs` and
`src/bin/cycledit.rs` (roughly 17 sites). Exceptions:

- `map_err(|_| eyre::eyre!("not a commit: {}", info.id))` — intentionally suppresses
  the error to include `info.id` instead; use `.map_err(|_| eyre::eyre!(...))` or
  `wrap_err_with(|| format!(...))` depending on whether the source error is useful.
  - for any cases where the source error will be discarded, interview the user the user first to confirm they approve (e.g. can group many similar cases together for efficient AskUserQuestion use that still touches on all instances)
- `eyre::bail!(...)` calls are already idiomatic.

## Required change

Add `use eyre::WrapErr;` import to each file that uses `.wrap_err()`.

## Notes

- `wrap_err_with(|| format!("invalid duration '{s}'"))` can be used when context
  string needs runtime formatting (avoids allocating when no error occurs).
- The gix error types all implement `std::error::Error`, so `wrap_err` will work on
  all of them.
