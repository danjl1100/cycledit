use crate::common::{CommandOutput, TestHarness};

const ARB_TIME: &str = "2000-01-01T00:00:00+00:00[UTC]";

#[track_caller]
fn assert_success_into_stdout(output: CommandOutput) -> String {
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, "");
    output.stdout
}

#[test]
fn main() -> eyre::Result<()> {
    let stdout = assert_success_into_stdout(TestHarness::new()?.run_cli(ARB_TIME, &["--help"])?);
    insta::assert_snapshot!(stdout);
    Ok(())
}

#[test]
fn subcommand_list() -> eyre::Result<()> {
    let stdout =
        assert_success_into_stdout(TestHarness::new()?.run_cli(ARB_TIME, &["list", "--help"])?);
    insta::assert_snapshot!(stdout);
    Ok(())
}

#[test]
fn subcommand_schedule() -> eyre::Result<()> {
    let stdout =
        assert_success_into_stdout(TestHarness::new()?.run_cli(ARB_TIME, &["schedule", "--help"])?);
    insta::assert_snapshot!(stdout);
    Ok(())
}

#[test]
fn subcommand_now() -> eyre::Result<()> {
    let stdout =
        assert_success_into_stdout(TestHarness::new()?.run_cli(ARB_TIME, &["now", "--help"])?);
    insta::assert_snapshot!(stdout);
    Ok(())
}

#[test]
fn subcommand_check() -> eyre::Result<()> {
    let stdout =
        assert_success_into_stdout(TestHarness::new()?.run_cli(ARB_TIME, &["check", "--help"])?);
    insta::assert_snapshot!(stdout);
    Ok(())
}
