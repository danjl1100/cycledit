# Issue 05: Glob matching is incomplete — consider replacing with a library

## Problem

`git.rs::glob_match_inner` is a custom recursive char-walker (~30 lines) that handles
`*` and `**` but is missing:

- `?` — match any single non-`/` character
- `[...]` — character classes

These are standard glob features users will expect in pathspecs and `--exclude` patterns.

## Option A: Patch the custom implementation

Add a `?` arm to `glob_match_inner`:

```rust
(['?', rest_pat @ ..], [t, rest_txt @ ..]) if *t != '/' => {
    glob_match_inner(rest_pat, rest_txt)
}
```

Character class support (`[abc]`, `[a-z]`) would need additional parsing logic.

## Option B: Replace with the `glob-match` crate (recommended)

`glob-match` is a pure string matcher (no filesystem access) that supports `**`, `*`,
`?`, and `[...]` with correct path-separator semantics — exactly what is needed here.
It would replace the entire `glob_match` / `glob_match_inner` / `matches_glob` block.

```toml
glob-match = "0.2"
```

```rust
fn matches_glob(pattern: &str, path: &str) -> bool {
    glob_match::glob_match(pattern, path)
        || PathBuf::from(path)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|name| glob_match::glob_match(pattern, name))
            .unwrap_or(false)
}
```

## Test coverage needed

Add unit tests for `matches_glob` covering (whichever option is chosen):
- `?` matching a single character: `"file?.txt"` matches `"file1.txt"`
- `?` not crossing `/`: `"dir?.txt"` does not match `"dir/a.txt"`
- `?` not matching empty: `"file?.txt"` does not match `"file.txt"`
- `[...]` character class if implemented
