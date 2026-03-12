# Issue 02: Division by zero when `cycle_days == 0`

## Problem

`schedule.rs:20` computes:

```rust
let max_per_chunk = ((chunk_days + cycle_days - 1) / cycle_days) as usize;
```

If the user passes `--cycle P0D`, `cycle_days` will be `0` and this panics with an
integer division by zero at runtime. No validation exists upstream in `parse_span_days`
or at the call site.

Similarly, `chunk_days == 0` with `max_per_chunk > 0` would cause the scheduling loop
to spin forever (infinite loop) since `chunk_date` would never advance.

## Fix

Validate both values in `parse_span_days` (or in `run()` before calling
`compute_schedule`) and return a user-friendly error:

```rust
if cycle_days <= 0 {
    eyre::bail!("--cycle must be a positive duration, got '{s}'");
}
if chunk_days <= 0 {
    eyre::bail!("--chunk must be a positive duration, got '{s}'");
}
```

## Test coverage needed

Add an integration test (or two) via `TestHarness` that passes `--cycle P0D` or
`--chunk P0D` and asserts a non-zero exit code with an appropriate error message in
stderr.
