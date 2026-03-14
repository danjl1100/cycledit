use clap::{Args, Parser, Subcommand};
use cycledit::git;
use eyre::WrapErr;

#[derive(Parser)]
#[command(name = "cycledit")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Arguments shared by schedule, now, and check subcommands.
#[derive(Args)]
struct ScheduleArgs {
    pathspecs: Vec<String>,
    #[arg(long = "exclude")]
    excludes: Vec<String>,
    /// Total cycle duration (default: 1 year)
    #[arg(long, default_value = "P1Y")]
    cycle: String,
    /// Chunk duration (default: 7 days)
    #[arg(long, default_value = "P7D")]
    chunk: String,
}

#[derive(Subcommand)]
enum Commands {
    /// List files with their last-modified date
    List {
        /// Paths/globs to include (default: all)
        pathspecs: Vec<String>,
        /// Paths/globs to exclude
        #[arg(long = "exclude")]
        excludes: Vec<String>,
    },
    /// Show the full edit schedule
    Schedule(ScheduleArgs),
    /// Show only files due now (chunk_date <= today)
    Now(ScheduleArgs),
    /// Check if any files are due; exits 100 if so
    Check(ScheduleArgs),
}

fn today() -> eyre::Result<jiff::civil::Date> {
    if let Ok(val) = std::env::var("CURRENT_TIME_ZONED") {
        let zoned: jiff::Zoned = val.parse().wrap_err("invalid CURRENT_TIME_ZONED")?;
        Ok(zoned.date())
    } else {
        Ok(jiff::Zoned::now().date())
    }
}

fn parse_span_days(s: &str, ref_date: jiff::civil::Date) -> eyre::Result<i64> {
    let span: jiff::Span = s
        .parse()
        .wrap_err_with(|| format!("invalid duration '{s}'"))?;
    let end = ref_date.checked_add(span).wrap_err("duration overflow")?;
    let days = ref_date
        .until(end)
        .wrap_err("duration conversion")?
        .get_days()
        .into();
    Ok(days)
}

fn parse_positive_span_days(s: &str, arg: &str, ref_date: jiff::civil::Date) -> eyre::Result<i64> {
    let days = parse_span_days(s, ref_date)?;
    if days <= 0 {
        eyre::bail!("--{arg} must be a positive duration, got '{s}'");
    }
    Ok(days)
}

fn print_schedule_chunk(date: jiff::civil::Date, files: &[git::FileEntry]) {
    println!("{date}:");
    for f in files {
        println!("\t{}", f.get_path().display());
    }
}

fn run() -> eyre::Result<i32> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;

    match cli.command {
        Commands::List {
            pathspecs,
            excludes,
        } => {
            let entries = git::list_files(&cwd, &pathspecs, &excludes)?;
            for entry in &entries {
                println!("{} {}", entry.get_date(), entry.get_path().display());
            }
        }

        Commands::Schedule(ScheduleArgs {
            pathspecs,
            excludes,
            cycle,
            chunk,
        }) => {
            let today = today()?;
            let entries = git::list_files(&cwd, &pathspecs, &excludes)?;
            let cycle_days = parse_positive_span_days(&cycle, "cycle", today)?;
            let chunk_days = parse_positive_span_days(&chunk, "chunk", today)?;
            let schedule =
                cycledit::schedule::compute_schedule(entries, cycle_days, chunk_days, today);
            for (date, files) in &schedule {
                print_schedule_chunk(*date, files);
            }
        }

        Commands::Now(ScheduleArgs {
            pathspecs,
            excludes,
            cycle,
            chunk,
        }) => {
            let today = today()?;
            let entries = git::list_files(&cwd, &pathspecs, &excludes)?;
            let cycle_days = parse_positive_span_days(&cycle, "cycle", today)?;
            let chunk_days = parse_positive_span_days(&chunk, "chunk", today)?;
            let schedule =
                cycledit::schedule::compute_schedule(entries, cycle_days, chunk_days, today);
            for (date, files) in schedule.iter().filter(|(d, _)| **d <= today) {
                print_schedule_chunk(*date, files);
            }
        }

        Commands::Check(ScheduleArgs {
            pathspecs,
            excludes,
            cycle,
            chunk,
        }) => {
            let today = today()?;
            let entries = git::list_files(&cwd, &pathspecs, &excludes)?;
            let total = entries.len();
            let cycle_days = parse_positive_span_days(&cycle, "cycle", today)?;
            let chunk_days = parse_positive_span_days(&chunk, "chunk", today)?;
            let schedule =
                cycledit::schedule::compute_schedule(entries, cycle_days, chunk_days, today);
            let due: usize = schedule
                .iter()
                .filter(|(d, _)| **d <= today)
                .map(|(_, files)| files.len())
                .sum();
            if due > 0 {
                println!("WARN: Need to update {due} file(s) now (of {total} files total)");
                return Ok(100);
            } else {
                println!("PASS: All files up to date");
            }
        }
    }

    Ok(0)
}

fn main() -> eyre::Result<std::convert::Infallible> {
    let exit_code = run()?;

    std::process::exit(exit_code)
}
