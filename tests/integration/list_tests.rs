use crate::common::TestHarness;

#[test]
fn list_single_file() -> eyre::Result<()> {
    let output = TestHarness::new()?
        .init_git(
            "
            2001-05-22:
            +root-file.txt
            ",
        )?
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"])?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, "");
    insta::assert_snapshot!(output.stdout, @"2001-05-22 root-file.txt\n");
    Ok(())
}

#[test]
fn list_multiple_sorted_lexicographically() -> eyre::Result<()> {
    let output = TestHarness::new()?
        .init_git(
            "
            2001-05-22:
            +folder1/file.txt
            +aaa.txt
            +zzz.txt
            ",
        )?
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"])?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, "");
    insta::assert_snapshot!(output.stdout, @"
    2001-05-22 aaa.txt
    2001-05-22 folder1/file.txt
    2001-05-22 zzz.txt
    ");
    Ok(())
}

#[test]
fn list_pathspec_filter() -> eyre::Result<()> {
    let output = TestHarness::new()?
        .init_git(
            "
            2001-05-22:
            +file1.txt
            +file2.txt
            +other.md
            ",
        )?
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list", "*.txt"])?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, "");
    insta::assert_snapshot!(output.stdout, @"
    2001-05-22 file1.txt
    2001-05-22 file2.txt
    ");
    Ok(())
}

#[test]
fn list_exclude_filter() -> eyre::Result<()> {
    let output = TestHarness::new()?
        .init_git(
            "
            2001-05-22:
            +file1.txt
            +file2.txt
            +other.md
            ",
        )?
        .run_cli(
            "2026-01-01T00:00:00+00:00[UTC]",
            &["list", "--exclude", "*.md"],
        )?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, "");
    insta::assert_snapshot!(output.stdout, @"
    2001-05-22 file1.txt
    2001-05-22 file2.txt
    ");
    Ok(())
}

#[test]
fn init_git_commits_once_per_date_block() -> eyre::Result<()> {
    // Three files in one date block must produce exactly one commit.
    let harness = TestHarness::new()?.init_git(
        "
        2001-05-22:
        +file1.txt
        +file2.txt
        +file3.txt
        ",
    )?;
    assert_eq!(harness.commit_count()?, 1);
    Ok(())
}

#[test]
fn list_error_not_in_git_repo() -> eyre::Result<()> {
    // Run from a temp dir that is NOT a git repo (no init_git call)
    let output = TestHarness::new()?.run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"])?;

    assert_eq!(output.status.code(), Some(1));
    // Filter the temp path which varies per run.
    insta::with_settings!({
        filters => vec![(r"'/[^']+'", "'[PATH]'")]
    }, {
        insta::assert_snapshot!(output.stderr);
    });
    Ok(())
}
