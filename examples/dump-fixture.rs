//! Dump a Git repository as a test fixture string.

fn main() -> eyre::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    print!("{}", cycledit::fixture::dump_fixture_string(path.as_ref())?);
    Ok(())
}
