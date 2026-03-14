use crate::common::TestHarness;

#[test]
fn schedule_error_zero_cycle() -> eyre::Result<()> {
    let output = TestHarness::new()
        .init_git(
            "
            2001-05-22:
            +file1.txt
            ",
        )?
        .run_cli(
            "2026-01-01T00:00:00+00:00[UTC]",
            &["schedule", "--cycle", "P0D"],
        )?;

    assert_eq!(output.status.code(), Some(1));
    insta::assert_snapshot!(output.stderr);
    Ok(())
}

#[test]
fn schedule_error_zero_chunk() -> eyre::Result<()> {
    let output = TestHarness::new()
        .init_git(
            "
            2001-05-22:
            +file1.txt
            ",
        )?
        .run_cli(
            "2026-01-01T00:00:00+00:00[UTC]",
            &["schedule", "--chunk", "P0D"],
        )?;

    assert_eq!(output.status.code(), Some(1));
    insta::assert_snapshot!(output.stderr);
    Ok(())
}

/// All files modified far in the past → all overdue → clamp to today,
/// then schedule across chunks starting from today.
#[test]
fn schedule_all_overdue_lands_in_today() -> eyre::Result<()> {
    // 3 files all committed 2001-05-22, cycle=1yr, chunk=7d
    // max_per_chunk = ceil(7/365) = 1, so each file gets its own chunk
    let output = TestHarness::new()
        .init_git(
            "
            2001-05-22:
            +file1.txt
            +file2.txt
            +file3.txt
            ",
        )?
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["schedule"])?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, "");
    insta::assert_snapshot!(output.stdout, @"
    2026-01-01:
    \tfile2.txt
    2026-01-08:
    \tfile1.txt
    2026-01-15:
    \tfile3.txt
    ");
    Ok(())
}

/// Files with future modification+cycle dates → scheduled in the future.
#[test]
fn schedule_future_dates() -> eyre::Result<()> {
    // file1 modified 2025-01-01, file2 modified 2025-07-01
    // cycle=1yr → file1 due 2026-01-01, file2 due 2026-07-01
    // today = 2025-06-01 (both in future)
    let output = TestHarness::new()
        .init_git(
            "
            2025-01-01:
            +file1.txt

            2025-07-01:
            +file2.txt
            ",
        )?
        .run_cli("2025-06-01T00:00:00+00:00[UTC]", &["schedule"])?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, "");
    insta::assert_snapshot!(output.stdout, @"
    2026-01-01:
    \tfile1.txt
    2026-07-01:
    \tfile2.txt
    ");
    Ok(())
}

/// Custom --cycle and --chunk args.
#[test]
fn schedule_custom_cycle_and_chunk() -> eyre::Result<()> {
    // file1 modified 2024-01-01, cycle=P30D, chunk=P10D → due 2024-01-31
    // today = 2023-06-01 (future)
    let output = TestHarness::new()
        .init_git(
            "
            2024-01-01:
            +file1.txt
            ",
        )?
        .run_cli(
            "2023-06-01T00:00:00+00:00[UTC]",
            &["schedule", "--cycle", "P30D", "--chunk", "P10D"],
        )?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, "");
    insta::assert_snapshot!(output.stdout, @"
    2024-01-31:
    \tfile1.txt
    ");
    Ok(())
}

/// Same-date tiebreaking by blob hash produces deterministic order.
/// Two files committed same date → order must be stable run-to-run (deterministic).
#[test]
fn schedule_same_date_deterministic_order() -> eyre::Result<()> {
    let output = TestHarness::new()
        .init_git(
            "
            2001-05-22:
            +aaa.txt
            +zzz.txt
            ",
        )?
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["schedule"])?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, "");
    insta::assert_snapshot!(output.stdout, @"
    2026-01-01:
    \tzzz.txt
    2026-01-08:
    \taaa.txt
    ");
    Ok(())
}

/// When chunk capacity is exceeded, overflow spills into next chunk.
#[test]
fn schedule_overflow_to_next_chunk() -> eyre::Result<()> {
    // cycle=P10D, chunk=P10D → max_per_chunk = ceil(10/10) = 1
    // 2 overdue files → each gets its own chunk date
    // today = 2026-01-01
    // file1 chunk: 2026-01-01, file2 chunk: 2026-01-11
    let output = TestHarness::new()
        .init_git(
            "
            2001-05-22:
            +file1.txt
            +file2.txt
            ",
        )?
        .run_cli(
            "2026-01-01T00:00:00+00:00[UTC]",
            &["schedule", "--cycle", "P10D", "--chunk", "P10D"],
        )?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, "");
    insta::assert_snapshot!(output.stdout, @"
    2026-01-01:
    \tfile2.txt
    2026-01-11:
    \tfile1.txt
    ");
    Ok(())
}
