use crate::common::TestHarness;

const ARB_FIXTURE: &str = "
2001-05-22:
+file1.txt
";

#[test]
fn schedule_error_zero_cycle() -> eyre::Result<()> {
    let output = TestHarness::new()?.init_git(ARB_FIXTURE)?.run_cli(
        "2026-01-01T00:00:00+00:00[UTC]",
        &["schedule", "--cycle", "P0D"],
    )?;

    assert_eq!(output.status.code(), Some(1));
    insta::assert_snapshot!(output.stderr);
    Ok(())
}
#[test]
fn schedule_error_negative_cycle() -> eyre::Result<()> {
    let output = TestHarness::new()?.init_git(ARB_FIXTURE)?.run_cli(
        "2026-01-01T00:00:00+00:00[UTC]",
        &["schedule", "--cycle=-P1D"],
    )?;

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    insta::assert_snapshot!(output.stderr);
    Ok(())
}
#[test]
fn schedule_error_large_cycle() -> eyre::Result<()> {
    let output = TestHarness::new()?.init_git(ARB_FIXTURE)?.run_cli(
        "2026-01-01T00:00:00+00:00[UTC]",
        &["schedule", "--cycle=P65536D"],
    )?;

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    insta::assert_snapshot!(output.stderr);
    Ok(())
}

#[test]
fn schedule_error_zero_chunk() -> eyre::Result<()> {
    let output = TestHarness::new()?.init_git(ARB_FIXTURE)?.run_cli(
        "2026-01-01T00:00:00+00:00[UTC]",
        &["schedule", "--chunk", "P0D"],
    )?;

    assert_eq!(output.status.code(), Some(1));
    insta::assert_snapshot!(output.stderr);
    Ok(())
}
#[test]
fn schedule_error_large_chunk() -> eyre::Result<()> {
    let output = TestHarness::new()?.init_git(ARB_FIXTURE)?.run_cli(
        "2026-01-01T00:00:00+00:00[UTC]",
        &["schedule", "--chunk", "P19998Y"],
    )?;

    assert_eq!(output.status.code(), Some(1));
    insta::assert_snapshot!(output.stderr);
    Ok(())
}
#[test]
fn schedule_error_invalid_chunk() -> eyre::Result<()> {
    let output = TestHarness::new()?.init_git(ARB_FIXTURE)?.run_cli(
        "2026-01-01T00:00:00+00:00[UTC]",
        &["schedule", "--chunk", "NOT-a-duration"],
    )?;

    assert_eq!(output.status.code(), Some(1));
    insta::assert_snapshot!(output.stderr);
    Ok(())
}

#[test]
fn schedule_error_chunk_exceeds_cycle() -> eyre::Result<()> {
    let output = TestHarness::new()?.init_git(ARB_FIXTURE)?.run_cli(
        "2026-01-01T00:00:00+00:00[UTC]",
        &["schedule", "--chunk", "P7D", "--cycle", "P6D"],
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
    let output = TestHarness::new()?
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
    insta::assert_snapshot!(output.stdout, @r"
    2026-01-01:
    	file2.txt
    2026-01-08:
    	file1.txt
    2026-01-15:
    	file3.txt
    ");
    Ok(())
}

/// Files with future modification+cycle dates → scheduled in the future.
#[test]
fn schedule_future_dates() -> eyre::Result<()> {
    // file1 modified 2025-01-01, file2 modified 2025-07-01
    // cycle=1yr → earliest 2026-01-01 and 2026-07-01; today = 2025-06-01, chunk=7d
    // 2026-01-01 is 214d from today → k=ceil(214/7)=31 → grid slot 2026-01-04
    // 2026-07-01 is 395d from today → k=ceil(395/7)=57 → grid slot 2026-07-05
    let output = TestHarness::new()?
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
    insta::assert_snapshot!(output.stdout, @r"
    2026-01-04:
    	file1.txt
    2026-07-05:
    	file2.txt
    ");
    Ok(())
}

/// Custom --cycle and --chunk args.
#[test]
fn schedule_custom_cycle_and_chunk() -> eyre::Result<()> {
    // file1 modified 2024-01-01, cycle=P30D, chunk=P10D → earliest 2024-01-31
    // today = 2023-06-01; days_ahead=244 → k=ceil(244/10)=25 → grid slot 2024-02-06
    let output = TestHarness::new()?
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
    insta::assert_snapshot!(output.stdout, @r"
    2024-02-06:
    	file1.txt
    ");
    Ok(())
}

/// Same-date tiebreaking by blob hash produces deterministic order.
/// Two files committed same date → order must be stable run-to-run (deterministic).
#[test]
fn schedule_same_date_deterministic_order() -> eyre::Result<()> {
    let output = TestHarness::new()?
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
    insta::assert_snapshot!(output.stdout, @r"
    2026-01-01:
    	zzz.txt
    2026-01-08:
    	aaa.txt
    ");
    Ok(())
}

/// Files whose earliest date falls between grid points are snapped to the next grid point,
/// not placed directly at earliest.
#[test]
fn schedule_chunk_alignment() -> eyre::Result<()> {
    // cycle=P7D, chunk=P7D → max_per_chunk = 1
    // today = 2026-01-01
    // file_a: committed 2025-12-25 → earliest = 2026-01-01 (= today) → grid slot 2026-01-01
    // file_b: committed 2025-12-26 → earliest = 2026-01-02 → desired: snap to 2026-01-08
    let output = TestHarness::new()?
        .init_git(
            "
            2025-12-25:
            +file_a.txt

            2025-12-26:
            +file_b.txt
            ",
        )?
        .run_cli(
            "2026-01-01T00:00:00+00:00[UTC]",
            &["schedule", "--cycle", "P7D", "--chunk", "P7D"],
        )?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, "");
    insta::assert_snapshot!(output.stdout, @r"
    2026-01-01:
    	file_a.txt
    2026-01-08:
    	file_b.txt
    ");
    Ok(())
}

/// Variant with chunk shorter than cycle: files whose earliest falls between grid points
/// are snapped to the next grid point.
#[test]
fn schedule_chunk_alignment_short_chunk() -> eyre::Result<()> {
    // cycle=P7D, chunk=P3D → max_per_chunk = ceil(3/7) = 1
    // grid: 2026-01-01, 2026-01-04, 2026-01-07, …
    // file_a: committed 2025-12-22 → earliest 2025-12-29 (overdue) → slot 2026-01-01
    // file_b: committed 2025-12-24 → earliest 2025-12-31 (overdue) → slot 2026-01-01 full → 2026-01-04
    // file_c: committed 2025-12-26 → earliest 2026-01-02 → snap to 2026-01-04 full → 2026-01-07
    let output = TestHarness::new()?
        .init_git(
            "
            2025-12-22:
            +file_a.txt

            2025-12-24:
            +file_b.txt

            2025-12-26:
            +file_c.txt
            ",
        )?
        .run_cli(
            "2026-01-01T00:00:00+00:00[UTC]",
            &["schedule", "--cycle", "P7D", "--chunk", "P3D"],
        )?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, "");
    insta::assert_snapshot!(output.stdout, @r"
    2026-01-01:
    	file_a.txt
    2026-01-04:
    	file_b.txt
    2026-01-07:
    	file_c.txt
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
    let output = TestHarness::new()?
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
    insta::assert_snapshot!(output.stdout, @r"
    2026-01-01:
    	file2.txt
    2026-01-11:
    	file1.txt
    ");
    Ok(())
}
