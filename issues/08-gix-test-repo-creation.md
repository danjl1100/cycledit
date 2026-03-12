# Issue 08: Use `gix` instead of `git` CLI to create integration test repos

## Background

`tests/integration/common.rs` uses `std::process::Command::new("git")` to build test
repositories via `run_git` / `run_git_env`. Each git operation (init, config, add, rm,
commit) spawns a new subprocess. `gix` is already the project's production dependency
(`version = "0.70"`, features `["revision", "index"]`).

## Pros

- **No external dependency** — tests work without a system `git` binary (minimal CI
  containers, unusual environments).
- **Speed** — no process-spawn overhead per git call. For repos with many commits this
  is meaningful.
- **Hermetic** — immune to system git version, global `~/.gitconfig`, and env vars like
  `GIT_AUTHOR_NAME` leaking in from the shell.
- **No `git config` workaround** — current code must set `user.email`/`user.name` per
  repo to suppress git errors; gix lets you pass a `Signature` directly.
- **Skip the filesystem for blobs** — the current approach writes real files to disk
  then stages them. With gix you write blobs directly to the object store; the
  working-tree files are unnecessary since `cycledit` only reads git history.

## Cons / Risks

- **Significantly more code** — building commits via gix requires explicit blob writes,
  recursive tree construction, commit object assembly, and ref updates. Roughly 4–6×
  more lines than the current `run_git` helpers.
- **Additional cargo features** — `features = ["revision", "index"]` is not enough to
  write objects. You will need at minimum `"worktree-mutation"` or equivalent write
  features, increasing compile time.
- **Pre-1.0 write API** — gix's read APIs (used by the main code) are relatively
  stable; the write APIs are less documented and may change between minor versions.
- **Risk of repo divergence** — if gix's write path produces subtly different object
  formats from real git (e.g. tree entry sorting, encoding), the tests could pass
  against gix-created repos but not against real-world repos. Since `cycledit` must
  handle repos created by real git, this is a correctness risk.

## Implementation Sketch

Below is a conceptual walkthrough of the key steps. Exact method names should be
verified against gix 0.70 source/docs.

### 1. Create the repository

```rust
// replaces: run_git(dir, &["init", "-b", "main"])
gix::create::into(dir, gix::create::Kind::WithWorktree, Default::default())?;
let repo = gix::open(dir)?;
// HEAD is "refs/heads/master" by default; rename to "main" via ref transaction or
// write .git/HEAD directly: std::fs::write(dir.join(".git/HEAD"), "ref: refs/heads/main\n")?;
```

### 2. In-memory file state

Instead of writing files to disk and running `git add`, maintain an in-memory map of
the current logical working tree:

```rust
// path string (e.g. "folder1/sub/file.txt") -> blob ObjectId
let mut live_files: BTreeMap<String, gix::ObjectId> = BTreeMap::new();
```

Apply `Add`/`Remove` ops to this map for each date block.

### 3. Write blob objects

```rust
// replaces: fs::write + run_git(dir, &["add", path])
use gix::objs::Blob;

let content = format!("{date}:{path}");
let blob_oid = repo.objects.write_object(&Blob { data: content.into_bytes() })?;
live_files.insert(path.clone(), blob_oid);
```

### 4. Build tree objects recursively (the hard part)

Git trees are hierarchical. A path like `"folder1/sub/file.txt"` requires three tree
objects: one for `sub/`, one for `folder1/`, one for the root. With `git add` the CLI
handles this automatically; with gix you must build it yourself.

```rust
use gix::objs::{Tree, tree::Entry};
use gix::objs::tree::EntryKind;

/// Recursively build tree objects from a flat path->blob map.
/// Returns the ObjectId of the root tree.
fn write_tree(
    odb: &impl gix::odb::Write,
    files: &BTreeMap<String, gix::ObjectId>,
) -> gix::ObjectId {
    // Group entries by their first path component, recursing into sub-directories.
    // e.g. {"folder1/sub/file.txt": b1, "root.txt": b2}
    // -> { "folder1" -> {"sub/file.txt": b1}, "root.txt" -> leaf(b2) }
    // -> { "folder1" -> { "sub" -> { "file.txt": b1 } }, "root.txt" -> leaf(b2) }
    let mut subtrees: BTreeMap<&str, BTreeMap<String, gix::ObjectId>> = BTreeMap::new();
    let mut direct_blobs: Vec<(&str, gix::ObjectId)> = vec![];

    for (path, oid) in files {
        if let Some((head, tail)) = path.split_once('/') {
            subtrees.entry(head).or_default().insert(tail.to_string(), *oid);
        } else {
            direct_blobs.push((path.as_str(), *oid));
        }
    }

    let mut entries: Vec<Entry> = vec![];

    // Recurse into sub-directories first, then add blobs.
    for (dir_name, sub_files) in &subtrees {
        let sub_tree_oid = write_tree(odb, sub_files);
        entries.push(Entry {
            mode: EntryKind::Tree.into(),
            filename: dir_name.into(),
            oid: sub_tree_oid,
        });
    }
    for (filename, oid) in direct_blobs {
        entries.push(Entry {
            mode: EntryKind::Blob.into(),
            filename: filename.into(),
            oid,
        });
    }

    // Git requires tree entries to be sorted by name (directories sort as if
    // they end with '/').  gix may do this automatically on encode; verify.
    entries.sort_by(|a, b| a.filename.cmp(&b.filename));

    odb.write_object(&Tree { entries }).expect("write tree")
}
```

> **Note:** git's actual tree sort order is more subtle — a directory named `foo` sorts
> as if it were `foo/`, so it may interleave differently with files named `foobar`.
> Verify that gix handles this on encode, or replicate the sort manually.

### 5. Build and write the commit object

```rust
use gix::actor::{Signature, Time};
use gix::date::time::Sign;
use gix::objs::Commit;

// replaces: run_git_env(dir, &["commit", "--allow-empty", "-m", ...], &[("GIT_COMMITTER_DATE", ...), ...])

// Parse date string "2001-05-22" to Unix timestamp (seconds since epoch).
// The project uses `jiff`, so:
let zoned = jiff::civil::Date::strptime("%Y-%m-%d", date)?
    .to_zoned(jiff::tz::TimeZone::UTC)?;
let unix_seconds = zoned.timestamp().as_second();

let time = Time { seconds: unix_seconds, offset: 0, sign: Sign::Plus };
let sig = Signature {
    name: "Test".into(),
    email: "test@example.com".into(),
    time,
};

let parents: gix::smallvec::SmallVec<[_; 1]> = match parent_oid {
    Some(oid) => gix::smallvec::smallvec![oid],
    None => gix::smallvec::SmallVec::new(),
};

let commit = Commit {
    tree: tree_oid,
    parents,
    author: sig.clone(),
    committer: sig,
    encoding: None,
    message: format!("commit on {date}").into(),
    extra_headers: vec![],
};

let commit_oid = repo.objects.write_object(&commit)?;
parent_oid = Some(commit_oid);
```

### 6. Update the branch reference

After the loop, point `refs/heads/main` at the final commit:

```rust
use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};

repo.edit_reference(RefEdit {
    change: Change::Update {
        log: LogChange {
            mode: RefLog::AndReference,
            force_create_reflog: false,
            message: "init".into(),
        },
        expected: PreviousValue::Any,
        new: gix::refs::Target::Peeled(commit_oid),
    },
    name: "refs/heads/main".try_into()?,
    deref: false,
})?;
```

### 7. `commit_count()` via gix

```rust
// replaces: git rev-list --count HEAD
let repo = gix::open(self.dir.path())?;
let count = repo
    .head_id()?
    .ancestors()
    .all()?
    .count();
```

### Additional cargo features needed

The current `Cargo.toml` dev-dependencies section has no `gix` entry (it's a
`[dependencies]` item). Using the write API requires adding gix to `[dev-dependencies]`
with write features, or expanding the existing dependency's feature set. Features likely
needed (verify against 0.70 changelog):

- `"object-store-dynamic"` or similar for mutable ODB access
- `"refs"` / `"refs-packed"` for ref transactions (may already be implied)

Alternatively, since `gix` is already in `[dependencies]`, the test code can use it
directly — no separate dev-dependency needed.

## Open Questions

- Does gix 0.70 sort tree entries correctly on encode, or must the caller sort them?
- Is `repo.objects.write_object(...)` the correct method, or is it `repo.write_object()`
  / `repo.odb.write(...)`? Verify against 0.70 source.
- What is the exact set of additional features required?

## Relationship to Other Issues

- Issue 09 eliminates `--allow-empty` commits (date blocks with no file ops), removing
  one special case from the gix commit path (same-tree parent commits).
