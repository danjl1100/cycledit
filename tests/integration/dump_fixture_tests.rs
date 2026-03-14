use crate::common::TestHarness;

#[test]
fn round_trip_single_add() {
    let fixture = "
2024-01-15:
+foo.txt
";
    let harness = TestHarness::new().init_git(fixture);
    let dumped = harness.dump_fixture();
    assert_eq!(dumped, fixture);

    let list_output = TestHarness::new()
        .init_git(&dumped)
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"]);
    assert_eq!(
        list_output.stdout,
        "2024-01-15 foo.txt
"
    );
}

#[test]
fn round_trip_add_and_remove() {
    let fixture = "
2024-01-15:
+foo.txt

2024-03-20:
+bar.txt
-foo.txt
";
    let harness = TestHarness::new().init_git(fixture);
    let dumped = harness.dump_fixture();
    assert_eq!(dumped, fixture);

    let harness2 = TestHarness::new().init_git(&dumped);
    assert_eq!(harness2.commit_count(), 2);
    let list_output = harness2.run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"]);
    assert_eq!(
        list_output.stdout,
        "2024-03-20 bar.txt
2024-01-15 foo.txt
"
    );
}

#[test]
fn round_trip_multiple_files() {
    let fixture = "
2024-01-15:
+aaa.txt
+mmm.txt
+zzz.txt

2024-03-20:
+aaa2.txt
+bbb.txt
";
    let harness = TestHarness::new().init_git(fixture);
    let dumped = harness.dump_fixture();
    assert_eq!(dumped, fixture);

    let harness2 = TestHarness::new().init_git(&dumped);
    assert_eq!(harness2.commit_count(), 2);
    let list_output = harness2.run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"]);
    assert_eq!(
        list_output.stdout,
        "2024-01-15 aaa.txt
2024-03-20 aaa2.txt
2024-03-20 bbb.txt
2024-01-15 mmm.txt
2024-01-15 zzz.txt
"
    );
}
