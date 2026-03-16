use crate::common::TestHarness;

/// Multi-commit fixture with subdirectories and deleted files, used for walk-metrics baseline.
const METRICS_FIXTURE: &str = "
2024-01-01:
+a/file1.txt
+a/file2.txt
+b/sub/file3.txt
+b/sub/file4.txt
+root1.txt
+root2.txt

2024-02-01:
+a/file5.txt
-a/file2.txt
+c/new1.txt
+c/new2.txt

2024-03-01:
+a/file6.txt
-b/sub/file3.txt
-c/new1.txt
";

#[test]
fn metrics_baseline() -> eyre::Result<()> {
    let output = TestHarness::new()?
        .init_git(METRICS_FIXTURE)?
        .with_metrics()
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"])?;
    insta::assert_snapshot!(output.stderr);
    Ok(())
}

#[test]
fn round_trip_subdirectory() -> eyre::Result<()> {
    let fixture = "
2024-06-01:
+README.md
+src/main.rs
+src/util/helper.rs
";
    let harness = TestHarness::new()?.init_git(fixture)?;
    let dumped = harness.dump_fixture()?;
    assert_eq!(dumped, fixture);

    let list_output = TestHarness::new()?
        .init_git(&dumped)?
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"])?;
    insta::assert_snapshot!(
        list_output.stdout,
        @r"
    2024-06-01 README.md
    2024-06-01 src/main.rs
    2024-06-01 src/util/helper.rs
    "
    );
    Ok(())
}

#[test]
fn round_trip_single_add() -> eyre::Result<()> {
    let fixture = "
2024-01-15:
+foo.txt
";
    let harness = TestHarness::new()?.init_git(fixture)?;
    let dumped = harness.dump_fixture()?;
    assert_eq!(dumped, fixture);

    let list_output = TestHarness::new()?
        .init_git(&dumped)?
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"])?;
    insta::assert_snapshot!(
        list_output.stdout,
        @"2024-01-15 foo.txt"
    );
    Ok(())
}

#[test]
fn round_trip_add_and_remove() -> eyre::Result<()> {
    let fixture = "
2024-01-15:
+foo.txt

2024-03-20:
+bar.txt
-foo.txt
";
    let harness = TestHarness::new()?.init_git(fixture)?;
    let dumped = harness.dump_fixture()?;
    assert_eq!(dumped, fixture);

    let harness2 = TestHarness::new()?.init_git(&dumped)?;
    assert_eq!(harness2.commit_count()?, 2);
    let list_output = harness2.run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"])?;
    insta::assert_snapshot!(
        list_output.stdout,
        @r"
    2024-03-20 bar.txt
    2024-01-15 foo.txt
    "
    );
    Ok(())
}

#[test]
fn round_trip_multiple_files() -> eyre::Result<()> {
    let fixture = "
2024-01-15:
+aaa.txt
+mmm.txt
+zzz.txt

2024-03-20:
+aaa2.txt
+bbb.txt
";
    let harness = TestHarness::new()?.init_git(fixture)?;
    let dumped = harness.dump_fixture()?;
    assert_eq!(dumped, fixture);

    let harness2 = TestHarness::new()?.init_git(&dumped)?;
    assert_eq!(harness2.commit_count()?, 2);
    let list_output = harness2.run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"])?;
    insta::assert_snapshot!(
        list_output.stdout,
        @r"
    2024-01-15 aaa.txt
    2024-03-20 aaa2.txt
    2024-03-20 bbb.txt
    2024-01-15 mmm.txt
    2024-01-15 zzz.txt
    "
    );
    Ok(())
}
