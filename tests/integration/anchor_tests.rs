use crate::common::TestHarness;

/// No .cycledit: forward-fill is used and a hint is printed when majority are overdue.
#[test]
fn schedule_no_anchor_hint_when_majority_overdue() -> eyre::Result<()> {
    // 3 overdue, 1 future → 3/4 > 50% → hint expected
    let output = TestHarness::new()?
        .init_git(
            "
            2001-01-01:
            +file1.txt
            2001-01-02:
            +file2.txt
            2001-01-03:
            +file3.txt

            2025-12-31:
            +file4.txt
        ",
        )?
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["schedule"])?;

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.contains("cycledit init"), "{}", output.stderr);
    insta::assert_snapshot!(output.stdout, @r"
    2026-01-01:
    	file1.txt
    2026-01-08:
    	file2.txt
    2026-01-15:
    	file3.txt
    2026-12-31:
    	file4.txt
    ");
    Ok(())
}

/// `cycledit init` writes `cycle_start` to .cycledit with today's date.
#[test]
fn init_writes_cycle_start() -> eyre::Result<()> {
    let harness = TestHarness::new()?.init_git(
        "
        2001-01-01:
        +file1.txt
    ",
    )?;

    let output = harness.run_cli("2026-01-01T00:00:00+00:00[UTC]", &["init"])?;

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.contains("2026-01-01"), "{}", output.stdout);

    let contents = std::fs::read_to_string(harness.git_root().join(".cycledit"))?;
    assert!(contents.contains("cycle_start = 2026-01-01"), "{contents}");
    Ok(())
}

/// With .cycledit present, completing a file in today's chunk leaves future chunks unchanged.
///
/// 5 overdue files, `cycle_start` = today, cycle=P35D, chunk=P7D
///   → `cycle_end` = today + 35d
///   → 5 slots (today, +7, +14, +21, +28), `max_per_slot` = 1
/// Backward-fill (oldest → furthest slot):
///   today+28: file1  today+21: file2  today+14: file3
///   today+7:  file4  today:    file5
/// After committing file5 → only 4 overdue remain → today drops to empty.
#[test]
fn schedule_anchor_stable_after_completing_today() -> eyre::Result<()> {
    let harness = TestHarness::new()?.init_git(
        "
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
    ",
    )?;
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
    let harness = harness.apply_git(
        "
        2026-01-01:
        +file5.txt
    ",
    )?;

    // Future chunks (file1–4) are unchanged; today's slot is empty; file5 is now a
    // future item due at cycle_end (2026-02-05 = 2026-01-01 + 35d).
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
    2026-02-05:
    	file5.txt
    ");
    Ok(())
}

/// Expired anchor falls back to forward-fill with a warning.
#[test]
fn schedule_expired_anchor_falls_back_with_warning() -> eyre::Result<()> {
    let harness = TestHarness::new()?.init_git(
        "
        2001-01-01:
        +file1.txt
    ",
    )?;
    // cycle_start so old that cycle_start + P1Y is well before today (2026-01-01)
    std::fs::write(
        harness.git_root().join(".cycledit"),
        "# cycledit cycle anchor\ncycle_start = 2020-01-01\n",
    )?;

    let output = harness.run_cli("2026-01-01T00:00:00+00:00[UTC]", &["schedule"])?;

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.contains("expired"), "{}", output.stderr);
    assert!(output.stderr.contains("cycledit init"), "{}", output.stderr);
    assert!(!output.stdout.is_empty());
    Ok(())
}

/// Re-init silently overwrites an existing anchor.
#[test]
fn init_overwrites_existing_anchor() -> eyre::Result<()> {
    let harness = TestHarness::new()?.init_git(
        "
        2001-01-01:
        +file1.txt
    ",
    )?;
    std::fs::write(
        harness.git_root().join(".cycledit"),
        "# cycledit cycle anchor\ncycle_start = 2025-06-01\n",
    )?;

    let output = harness.run_cli("2026-01-01T00:00:00+00:00[UTC]", &["init"])?;
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, "");

    let contents = std::fs::read_to_string(harness.git_root().join(".cycledit"))?;
    assert!(contents.contains("cycle_start = 2026-01-01"), "{contents}");
    Ok(())
}
