# Issue 14: Stable backward-fill scheduling with cycle anchor

## Problem

When many items are overdue, `compute_schedule` fills chunks forward from today.
As the user commits files (completing them), the schedule shifts: items previously
in future chunks slide back into today's slot.  For a user making incremental
progress — commit one file, re-run, commit another — the schedule appears to
constantly reset rather than drain.

**Example** — 5 overdue files, `cycle=P365D`, `chunk=P7D`, `max_per_chunk=1`:

```
# Run 1 (5 overdue)
2026-01-01: file1.txt      ← today
2026-01-08: file2.txt
2026-01-15: file3.txt
...
```

User commits `file1.txt` → no longer overdue (fresh git date).

```
# Run 2 (4 overdue) — file2 slid from next chunk to today
2026-01-01: file2.txt      ← !! moved back
2026-01-08: file3.txt
...
```

This instability discourages incremental progress.

## Root cause

All overdue items share `days_ahead = 0` after clamping, so their grid slot is
determined solely by the order they are processed.  Removing any item from the
sorted list shifts every subsequent item one slot earlier.

## Proposed solution

### Overview

Introduce a **cycle anchor** — a persisted `cycle_start` date — so that overdue
items can be distributed backward from a known endpoint rather than accumulated
forward from today.  `cycle_end` is always derived at runtime as
`cycle_start + cycle_days`, where `cycle_days` comes from the `--cycle` flag on
`schedule`.  This means the user can freely change `--cycle` mid-cycle —
shrinking it focuses the remaining window, lengthening it gives more breathing
room — without needing to re-init.

Oldest items are pushed toward `cycle_end`; the items completing their cycle
soonest land in today's slot.  Completing today's items shrinks today's bucket
without touching future chunks.

### State file: `.cycledit`

Written to the git repository root (not committed — personal to each contributor).
Format is two lines:

```
# cycledit cycle anchor — run `cycledit init` to reset
cycle_start = 2026-01-01
```

The file is discovered by walking up to the git root at runtime (the same root
`git::list_files` already locates).  It is safe to add to `.gitignore` or
`.git/info/exclude`.

### New subcommand: `cycledit init`

```
cycledit init
```

- Writes `cycle_start = today` to `.cycledit` in the git root, silently
  overwriting any existing file
- Prints to stdout: `Cycle anchor set: started 2026-01-01`

Re-init during an in-progress cycle is intentional and expected (e.g., after a
multi-week gap when the user wants a fresh window), so no warning is needed.

### Modified scheduling algorithm

The CLI reads `.cycledit`, parses `cycle_start`, computes
`cycle_end = cycle_start + cycle_days`, and passes `cycle_end: Option<Date>`
to `compute_schedule` (or a new library entry point).  Two code paths:

#### Path A — no anchor (current behavior + hint)

Used when `.cycledit` is absent or `cycle_start + cycle_days <= today` (expired).

- Forward-fill from today, same as today.
- **Hint**: if more than half of all tracked items are overdue, print to stderr:
  ```
  hint: N of M files due today; run `cycledit init` to stabilize the schedule
  ```
  Output is still written to stdout normally.
- If the cycle has expired (file present but `cycle_start + cycle_days <= today`),
  also warn:
  ```
  hint: cycle anchor expired (started 2026-01-01); run `cycledit init` to set a new one
  ```

#### Path B — anchor present (`cycle_end` supplied by the binary)

1. **Separate** items into:
   - *Overdue*: `earliest_date <= today`  (`earliest_date = git_date + cycle_days`)
   - *Future*: `earliest_date > today` — unchanged, snapped to grid as before

2. **Compute available slots** for overdue items:
   ```
   available_slots = floor((cycle_end - today) / chunk_days) + 1
   ```
   Slots are `today`, `today + chunk_days`, …, up to `cycle_end`.

3. **Compute max per slot** from the overdue pool:
   ```
   max_per_slot = overdue_count.div_ceil(available_slots)
   ```

4. **Backward-fill**: sort overdue items `(git_date ASC, blob_hash ASC)` (same
   existing sort — oldest first).  Assign them to slots starting from the *last*
   available slot and working backward toward today:
   - Slots `available_slots-1` down to `1`: fill `max_per_slot` items each
     (oldest items go into the furthest slots)
   - Slot `0` (today): all remaining items (the newest overdue items, possibly
     more than `max_per_slot` if overflow)

5. **Merge** the overdue chunk map with the future items (future items continue
   using the existing forward-snap logic).

**Stability property**: completing an item removes it from today's bucket
(slot 0).  The backward assignment of older items to slots 1…N is unaffected
because those assignments are filled from the tail of the sorted list, which
does not change when a "newer" item is removed.

## Integration tests

### Prerequisite: `TestHarness::apply_git`

The stability test (below) needs to add a commit to an already-initialized repo.
`init_git` currently calls `git init` and cannot be called twice.  A new harness
method is needed:

```rust
/// Apply additional fixture blocks to an existing repo (no `git init`).
pub fn apply_git(self, state: &str) -> eyre::Result<Self>
```

This re-uses the existing `BlocksIter` / `init_git_from_blocks` visitor logic
but skips the `git init` step.

### Test A: No anchor — forward-fill + hint when >50% overdue

```rust
/// No .cycledit: forward-fill is used and a hint is printed when majority are overdue.
#[test]
fn schedule_no_anchor_hint_when_majority_overdue() -> eyre::Result<()> {
    // 3 overdue, 1 future → 3/4 > 50% → hint expected
    let output = TestHarness::new()?
        .init_git("
            2001-01-01:
            +file1.txt
            2001-01-02:
            +file2.txt
            2001-01-03:
            +file3.txt

            2025-12-31:
            +file4.txt
        ")?
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["schedule"])?;

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.contains("cycledit init"), "{}", output.stderr);
    insta::assert_snapshot!(output.stdout);    // forward-fill
    Ok(())
}
```

### Test B: `cycledit init` creates state file with correct date

```rust
#[test]
fn init_writes_cycle_start() -> eyre::Result<()> {
    let harness = TestHarness::new()?.init_git("
        2001-01-01:
        +file1.txt
    ")?;

    let output = harness.run_cli(
        "2026-01-01T00:00:00+00:00[UTC]",
        &["init"],
    )?;

    assert_eq!(output.status.code(), Some(0));
    // stdout confirms start date
    assert!(output.stdout.contains("2026-01-01"), "{}", output.stdout);

    // .cycledit stores cycle_start, not cycle_end
    let contents = std::fs::read_to_string(harness.git_root().join(".cycledit"))?;
    assert!(contents.contains("cycle_start = 2026-01-01"), "{contents}");
    Ok(())
}
```

*(Requires a `harness.git_root() -> &Path` accessor, or test reads the file via
a known temp path.)*

### Test C: Backward-fill with anchor — core stability (new failing test)

This is the primary test that should **fail** before the implementation and
**pass** after.

```rust
/// With .cycledit present, completing a file in today's chunk leaves
/// future chunks unchanged.
///
/// 5 overdue files, cycle_start = today, cycle=P35D, chunk=P7D
///   → cycle_end = today + 35d
///   → 5 slots (today, +7, +14, +21, +28), max_per_slot = 1
/// Backward-fill (oldest → furthest slot):
///   today+28: file1  today+21: file2  today+14: file3
///   today+7:  file4  today:    file5
/// After committing file5 → only 4 overdue remain → today drops to empty.
#[test]
fn schedule_anchor_stable_after_completing_today() -> eyre::Result<()> {
    let harness = TestHarness::new()?.init_git("
        2001-01-01:
        +file1.txt
        2001-01-02:
        +file2.txt
        2001-01-03:
        +file3.txt
        2001-01-04:
        +file4.txt
        2001-01-05:
        +file5.txt
    ")?;
    let today = "2026-01-01T00:00:00+00:00[UTC]";

    // Init: writes cycle_start = 2026-01-01
    let init_out = harness.run_cli(today, &["init"])?;
    assert_eq!(init_out.status.code(), Some(0));

    // Backward-fill schedule (cycle_end = 2026-01-01 + 35d = 2026-02-05)
    let out1 = harness.run_cli(today, &["schedule", "--cycle", "P35D"])?;
    assert_eq!(out1.status.code(), Some(0));
    assert_eq!(out1.stderr, "");
    insta::assert_snapshot!(out1.stdout, @r"
    2026-01-01:
    	file5.txt
    2026-01-08:
    	file4.txt
    2026-01-15:
    	file3.txt
    2026-01-22:
    	file2.txt
    2026-01-29:
    	file1.txt
    ");

    // Simulate committing file5 (today → no longer overdue)
    let harness = harness.apply_git("
        2026-01-01:
        +file5.txt
    ")?;

    // Future chunks are unchanged; today's slot is now empty (absent from output)
    let out2 = harness.run_cli(today, &["schedule", "--cycle", "P35D"])?;
    assert_eq!(out2.status.code(), Some(0));
    assert_eq!(out2.stderr, "");
    insta::assert_snapshot!(out2.stdout, @r"
    2026-01-08:
    	file4.txt
    2026-01-15:
    	file3.txt
    2026-01-22:
    	file2.txt
    2026-01-29:
    	file1.txt
    ");
    Ok(())
}
```

### Test D: Expired anchor falls back gracefully

```rust
#[test]
fn schedule_expired_anchor_falls_back_with_warning() -> eyre::Result<()> {
    let harness = TestHarness::new()?.init_git("
        2001-01-01:
        +file1.txt
    ")?;
    // cycle_start so old that cycle_start + P1Y is well before today (2026-01-01)
    std::fs::write(
        harness.git_root().join(".cycledit"),
        "# cycledit cycle anchor\ncycle_start = 2020-01-01\n",
    )?;

    let output = harness.run_cli("2026-01-01T00:00:00+00:00[UTC]", &["schedule"])?;

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.contains("expired"), "{}", output.stderr);
    assert!(output.stderr.contains("cycledit init"), "{}", output.stderr);
    // stdout still contains the forward-fill schedule
    assert!(!output.stdout.is_empty());
    Ok(())
}
```

### Test E: Re-init silently overwrites existing anchor

```rust
#[test]
fn init_overwrites_existing_anchor() -> eyre::Result<()> {
    let harness = TestHarness::new()?.init_git("
        2001-01-01:
        +file1.txt
    ")?;
    std::fs::write(
        harness.git_root().join(".cycledit"),
        "# cycledit cycle anchor\ncycle_start = 2025-06-01\n",
    )?;

    let output = harness.run_cli(
        "2026-01-01T00:00:00+00:00[UTC]",
        &["init"],
    )?;
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, "");

    let contents = std::fs::read_to_string(harness.git_root().join(".cycledit"))?;
    assert!(contents.contains("cycle_start = 2026-01-01"), "{contents}");
    Ok(())
}
```

## Edge cases

| Situation | Behaviour |
|---|---|
| `.cycledit` absent, ≤50% overdue | Forward-fill, no hint |
| `.cycledit` absent, >50% overdue | Forward-fill + init hint on stderr |
| `cycle_start + cycle_days` = today | Single slot; all overdue items land today (catch-up) |
| `cycle_start + cycle_days` in the past | Warn on stderr, fall back to forward-fill |
| Malformed `.cycledit` | Warn on stderr, fall back to forward-fill |
| Future items beyond `cycle_end` | Snapped to their natural grid slot (unchanged) |

## Implementation notes

- The anchor concept belongs in the binary (`src/bin/cycledit.rs`): read
  `.cycledit`, parse `cycle_start`, compute `cycle_end = cycle_start + cycle_days`,
  then pass `anchor: Option<Date>` to the library.  Keep I/O out of
  `src/schedule.rs`.
- `compute_schedule` signature change (or a new sibling function) to accept
  `cycle_end: Option<Date>`.  The binary resolves the `cycle_start` → `cycle_end`
  conversion so the library remains concerned only with dates, not file I/O.
- The `init` subcommand does not need access to the entry list; it only needs `today`.
- `TestHarness` needs `apply_git` (without `git init`) and `git_root() -> &Path`
  before the integration tests can be written.
