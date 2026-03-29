use crate::common::TestHarness;

/// `now` shows only chunks whose date <= today.
#[test]
fn now_shows_only_past_and_today_chunks() -> eyre::Result<()> {
    // file1 modified 2024-01-01 (overdue → clamped to today 2025-06-01)
    // file2 modified 2025-07-01 (future → due 2026-07-01, not shown in `now`)
    // today = 2025-06-01
    let output = TestHarness::new()?
        .init_git(
            "
            2024-01-01:
            +file1.txt

            2025-07-01:
            +file2.txt
            ",
        )?
        .run_cli("2025-06-01T00:00:00+00:00[UTC]", &["now"])?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, "");
    insta::assert_snapshot!(output.stdout, @r"
    2025-06-01:
    	file1.txt
    ");
    Ok(())
}

/// `now` produces empty output when nothing is due.
#[test]
fn now_empty_when_nothing_due() -> eyre::Result<()> {
    // file1 modified recently → due in future
    let output = TestHarness::new()?
        .init_git(
            "
            2025-07-01:
            +file1.txt
            ",
        )?
        .run_cli("2025-06-01T00:00:00+00:00[UTC]", &["now"])?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, "");
    assert_eq!(output.stdout, "");
    Ok(())
}

/// `check` exits 100 and prints WARN when files are due.
#[test]
fn check_warn_when_files_due() -> eyre::Result<()> {
    // file1 overdue (committed 2001-05-22), today = 2026-01-01
    let output = TestHarness::new()?
        .init_git(
            "
            2001-05-22:
            +file1.txt
            +file2.txt
            +file3.txt
            ",
        )?
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["check"])?;

    assert_eq!(output.status.code(), Some(100));
    insta::assert_snapshot!(output.stderr, @"hint: 3 of 3 files due today; run `cycledit init` to stabilize the schedule");
    // ceil(7/365) = 1 per chunk; only the first chunk falls on today (1 of 3 files)
    insta::assert_snapshot!(output.stdout, @"WARN: Need to update 1 file(s) now (of 3 files total)");
    Ok(())
}

/// `check` exits 0 and prints PASS when nothing is due.
#[test]
fn check_pass_when_nothing_due() -> eyre::Result<()> {
    // file1 modified recently → due in future
    let output = TestHarness::new()?
        .init_git(
            "
            2025-07-01:
            +file1.txt
            ",
        )?
        .run_cli("2025-06-01T00:00:00+00:00[UTC]", &["check"])?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, "");
    insta::assert_snapshot!(output.stdout, @"PASS: All files up to date");
    Ok(())
}

/// `CURRENT_TIME_ZONED` drives the "today" used for scheduling.
#[test]
fn current_time_zoned_drives_today() -> eyre::Result<()> {
    // file1 modified 2024-01-01, cycle=P10D (10 days) → due 2024-01-11
    // With time = 2024-01-10 → not due yet (now shows nothing)
    // With time = 2024-01-11 → due exactly today (now shows file1)
    let state = "
        2024-01-01:
        +file1.txt
    ";

    let output_before = TestHarness::new()?.init_git(state)?.run_cli(
        "2024-01-10T00:00:00+00:00[UTC]",
        &["now", "--cycle", "P10D"],
    )?;
    assert_eq!(output_before.status.code(), Some(0));
    assert_eq!(output_before.stderr, "");
    assert_eq!(output_before.stdout, "", "should be empty before due date");

    let output_on = TestHarness::new()?.init_git(state)?.run_cli(
        "2024-01-11T00:00:00+00:00[UTC]",
        &["now", "--cycle", "P10D"],
    )?;
    assert_eq!(output_on.status.code(), Some(0));
    assert_eq!(output_on.stderr, "");
    insta::assert_snapshot!(output_on.stdout, @r"
    2024-01-11:
    	file1.txt
    ");
    Ok(())
}
