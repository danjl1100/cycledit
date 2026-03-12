use crate::common::TestHarness;

#[test]
fn list_single_file() {
    let output = TestHarness::new()
        .init_git(
            "
            2001-05-22:
            +root-file.txt
            ",
        )
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"]);

    assert_eq!(output.status.code(), Some(0));
    insta::assert_snapshot!(output.stdout, @"2001-05-22 root-file.txt\n");
}

#[test]
fn list_multiple_sorted_lexicographically() {
    let output = TestHarness::new()
        .init_git(
            "
            2001-05-22:
            +folder1/file.txt
            +aaa.txt
            +zzz.txt
            ",
        )
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"]);

    assert_eq!(output.status.code(), Some(0));
    insta::assert_snapshot!(output.stdout, @"
    2001-05-22 aaa.txt
    2001-05-22 folder1/file.txt
    2001-05-22 zzz.txt
    ");
}

#[test]
fn list_pathspec_filter() {
    let output = TestHarness::new()
        .init_git(
            "
            2001-05-22:
            +file1.txt
            +file2.txt
            +other.md
            ",
        )
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list", "*.txt"]);

    assert_eq!(output.status.code(), Some(0));
    insta::assert_snapshot!(output.stdout, @"
    2001-05-22 file1.txt
    2001-05-22 file2.txt
    ");
}

#[test]
fn list_exclude_filter() {
    let output = TestHarness::new()
        .init_git(
            "
            2001-05-22:
            +file1.txt
            +file2.txt
            +other.md
            ",
        )
        .run_cli(
            "2026-01-01T00:00:00+00:00[UTC]",
            &["list", "--exclude", "*.md"],
        );

    assert_eq!(output.status.code(), Some(0));
    insta::assert_snapshot!(output.stdout, @"
    2001-05-22 file1.txt
    2001-05-22 file2.txt
    ");
}

#[test]
fn list_error_not_in_git_repo() {
    // Run from a temp dir that is NOT a git repo (no init_git call)
    let output = TestHarness::new().run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"]);

    assert_ne!(output.status.code(), Some(0));
    insta::assert_snapshot!(output.stderr);
}
