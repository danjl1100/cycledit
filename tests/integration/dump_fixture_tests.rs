use crate::common::TestHarness;

#[test]
fn round_trip_single_add() {
    let fixture = "2024-01-15:\n+foo.txt\n\n";
    let harness = TestHarness::new().init_git(fixture);
    let dumped = harness.dump_fixture();
    assert_eq!(dumped, fixture);

    let list_output = TestHarness::new()
        .init_git(&dumped)
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"]);
    assert_eq!(list_output.stdout, "2024-01-15 foo.txt\n");
}

#[test]
fn round_trip_add_and_remove() {
    let fixture = "2024-01-15:\n+foo.txt\n\n2024-03-20:\n+bar.txt\n-foo.txt\n\n";
    let harness = TestHarness::new().init_git(fixture);
    let dumped = harness.dump_fixture();
    assert_eq!(dumped, fixture);

    let harness2 = TestHarness::new().init_git(&dumped);
    assert_eq!(harness2.commit_count(), 2);
    let list_output = harness2.run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"]);
    assert_eq!(
        list_output.stdout,
        "2024-03-20 bar.txt\n2024-01-15 foo.txt\n"
    );
}

#[test]
fn round_trip_multiple_files() {
    let fixture =
        "2024-01-15:\n+aaa.txt\n+mmm.txt\n+zzz.txt\n\n2024-03-20:\n+aaa2.txt\n+bbb.txt\n\n";
    let harness = TestHarness::new().init_git(fixture);
    let dumped = harness.dump_fixture();
    assert_eq!(dumped, fixture);

    let harness2 = TestHarness::new().init_git(&dumped);
    assert_eq!(harness2.commit_count(), 2);
    let list_output = harness2.run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"]);
    assert_eq!(
        list_output.stdout,
        "2024-01-15 aaa.txt\n2024-03-20 aaa2.txt\n2024-03-20 bbb.txt\n2024-01-15 mmm.txt\n2024-01-15 zzz.txt\n"
    );
}
