# Issue 10: Align schedule chunks to `today + i*chunk_days`

## Problem

`compute_schedule` in `src/schedule.rs` places each file into a chunk whose date is
derived from the file's `earliest` date:

```rust
let mut chunk_date = earliest.max(today);
loop {
    if count < max_per_chunk { break; }
    chunk_date += chunk_days;
}
```

Because `earliest` varies per file (`entry.date + cycle_days`), overdue files that
clamp to different values of `today.max(earliest)` can land in chunks that overlap or
are irregularly spaced. The desired behaviour is that all chunk boundaries are aligned
to the fixed grid `today`, `today + chunk_days`, `today + 2*chunk_days`, …, so chunks
are always separated by exactly `chunk_days`.

## Desired Behaviour

For any file whose earliest date ≤ today (overdue), the file is placed into the first
non-full slot of the grid starting at today. For files whose earliest date > today, the
file is placed into the first non-full grid slot at or after `earliest`, where the grid
origin is still `today`.

That is, a valid chunk date must satisfy:

```
chunk_date = today + k * chunk_days   (k = 0, 1, 2, …)
chunk_date >= earliest
```

## Implementation Steps

1. **Write a snapshot test that exposes the current behaviour.**  Design a fixture with
   multiple files at different past dates such that the current algorithm produces
   non-grid-aligned chunk boundaries (e.g. two files overdue from different past dates
   that currently fall into slightly offset chunks).

2. **Update the snapshot output to reflect the desired behaviour** — all chunks on the
   `today + k*chunk_days` grid.

3. **Show the test fixture and updated snapshot to the user and confirm they approve the
   desired behaviour** before touching the implementation.

4. **Update `compute_schedule`** to snap each candidate chunk date to the grid:

   ```rust
   // Snap chunk_date up to the next grid point >= earliest
   let offset = (earliest - today).total::<jiff::Unit::Day>().max(0.0).ceil() as u64;
   let k = offset.div_ceil(chunk_days);
   let mut chunk_date = today + k * chunk_days;
   ```

   (Exact arithmetic should use `jiff` span/date operations; the snippet above is
   illustrative.)

5. Run all existing schedule tests; update any snapshots that reflect the old behaviour.

## Files Affected

- `src/schedule.rs` — `compute_schedule`
- `tests/integration/schedule_tests.rs` — new test, possible snapshot updates
