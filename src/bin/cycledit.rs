//! `cycledit` CLI binary.

use clap::{Args, Parser, Subcommand};
use cycledit::{git, schedule::ScheduleParams};
use eyre::WrapErr;
use jiff::civil::Date;
use std::num::NonZeroU16;

#[derive(Parser)]
#[command(name = "cycledit")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Arguments to list entries from [`git`]
#[derive(Args)]
struct ListArgs {
    /// Paths/globs to include (default: all)
    pathspecs: Vec<String>,
    /// Paths/globs to exclude
    #[arg(long = "exclude")]
    excludes: Vec<String>,
}

/// Arguments to create the schedule
#[derive(Args)]
struct ScheduleArgs {
    /// Total cycle duration (default: 1 year)
    #[arg(long, default_value = "P1Y")]
    cycle: String,
    /// Chunk duration (default: 7 days)
    #[arg(long, default_value = "P7D")]
    chunk: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Set a new cycle anchor (writes `.cycledit` in the repo root)
    Init,
    /// List files with their last-modified date
    List {
        #[clap(flatten)]
        list: ListArgs,
    },
    /// Show the full edit schedule
    Schedule {
        #[clap(flatten)]
        list: ListArgs,
        #[clap(flatten)]
        schedule: ScheduleArgs,
    },
    /// Show only files due now (`chunk_date <= today`)
    Now {
        #[clap(flatten)]
        list: ListArgs,
        #[clap(flatten)]
        schedule: ScheduleArgs,
    },
    /// Check if any files are due; exits 100 if so
    Check {
        #[clap(flatten)]
        list: ListArgs,
        #[clap(flatten)]
        schedule: ScheduleArgs,
    },
}

fn today() -> eyre::Result<jiff::civil::Date> {
    if let Ok(val) = std::env::var("CURRENT_TIME_ZONED") {
        let zoned: jiff::Zoned = val.parse().wrap_err("invalid CURRENT_TIME_ZONED")?;
        Ok(zoned.date())
    } else {
        Ok(jiff::Zoned::now().date())
    }
}

fn parse_span_days(s: &str, ref_date: jiff::civil::Date) -> eyre::Result<NonZeroU16> {
    let span: jiff::Span = s
        .parse()
        .wrap_err_with(|| format!("invalid duration '{s}'"))?;
    let end = ref_date
        .checked_add(span)
        .wrap_err("duration too large (overflow)")?;
    let days: i32 = ref_date
        .until(end)
        .wrap_err("duration conversion")?
        .get_days();

    let range_err = |msg| format!("{msg}: {days} days"); // NOTE: no plural, "1" is valid

    let days = u32::try_from(days).wrap_err_with(|| range_err("negative duration"))?;
    let days = u16::try_from(days)
        .wrap_err_with(|| range_err("duration too large (u16::MAX = 65535 days)"))?;
    let Some(days) = NonZeroU16::new(days) else {
        eyre::bail!(range_err("zero duration"))
    };

    Ok(days)
}

fn parse_positive_span_days(
    s: &str,
    arg: &str,
    ref_date: jiff::civil::Date,
) -> eyre::Result<NonZeroU16> {
    let days = parse_span_days(s, ref_date).wrap_err_with(|| {
        format!(
            "--{arg} must one day or longer (e.g. period 1 day = \"P1D\" or \"1d\") and in range, got '{s}'"
        )
    })?;
    Ok(days)
}
impl ScheduleArgs {
    fn parse_span_days(&self, today: jiff::civil::Date) -> eyre::Result<ScheduleParams> {
        let Self { cycle, chunk } = self;
        let cycle_days = parse_positive_span_days(cycle, "cycle", today)?;
        let chunk_days = parse_positive_span_days(chunk, "chunk", today)?;

        ScheduleParams::builder()
            .cycle_days(cycle_days)
            .chunk_days(chunk_days)
            .wrap_err_with(|| format!("invalid chunk duration {chunk:?}"))
    }
}

impl ListArgs {
    fn list_files(&self, cwd: &std::path::Path) -> eyre::Result<Vec<git::FileEntry>> {
        let Self {
            pathspecs,
            excludes,
        } = self;
        git::list_files(cwd, pathspecs, excludes)
    }
}

fn print_schedule_chunk(date: jiff::civil::Date, files: &[git::FileEntry]) {
    println!("{date}:");
    for f in files {
        println!("\t{}", f.get_path().display());
    }
}

/// Reads `.cycledit` from `git_root` and returns a valid `cycle_end`, or `None` on
/// Path A (absent / expired / malformed). Warnings are printed to stderr.
fn read_cycle_end(
    git_root: &std::path::Path,
    cycle_days: NonZeroU16,
    today: Date,
) -> eyre::Result<Option<Date>> {
    let path = git_root.join(".cycledit");
    let contents = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(eyre::eyre!(e).wrap_err("failed to read .cycledit")),
    };

    // Parse `cycle_start = YYYY-MM-DD`
    let cycle_start: Date = if let Some(Ok(date)) = contents.lines().find_map(|line| {
        let val = line.trim().strip_prefix("cycle_start =")?;
        Some(val.trim().parse::<Date>())
    }) {
        date
    } else {
        eprintln!("hint: .cycledit is malformed; run `cycledit init` to set a new one");
        return Ok(None);
    };

    let cycle_end = cycle_start
        .checked_add(jiff::Span::new().days(cycle_days.get()))
        .wrap_err("cycle_end overflow")?;

    if cycle_end <= today {
        eprintln!(
            "hint: cycle anchor expired (started {cycle_start}, ended {cycle_end}); run `cycledit init` to set a new one"
        );
        return Ok(None);
    }

    Ok(Some(cycle_end))
}

fn run() -> eyre::Result<i32> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;

    match cli.command {
        Commands::Init => {
            let today = today()?;
            let git_root = git::find_git_root(&cwd)?;
            let path = git_root.join(".cycledit");
            let contents = format!(
                "# cycledit cycle anchor — run `cycledit init` to reset\ncycle_start = {today}\n"
            );
            std::fs::write(&path, contents).wrap_err("failed to write .cycledit")?;
            println!("Cycle anchor set: started {today}");
        }

        Commands::List { list } => {
            let entries = list.list_files(&cwd)?;
            for entry in &entries {
                println!("{} {}", entry.get_date(), entry.get_path().display());
            }
        }

        Commands::Schedule { list, schedule } => {
            let today = today()?;
            let schedule_params = schedule.parse_span_days(today)?;
            let cycle_days = schedule_params.get_cycle_days();

            let git_root = git::find_git_root(&cwd)?;
            let cycle_end = read_cycle_end(&git_root, cycle_days, today)?;

            let entries = list.list_files(&cwd)?;
            let total = entries.len();

            // Path A hint: >50% of all items are overdue and no anchor is active.
            if cycle_end.is_none() {
                let overdue = entries
                    .iter()
                    .filter(|e| {
                        e.get_date()
                            .checked_add(jiff::Span::new().days(cycle_days.get()))
                            .is_ok_and(|earliest| earliest <= today)
                    })
                    .count();
                if overdue > total / 2 {
                    eprintln!(
                        "hint: {overdue} of {total} files due today; run `cycledit init` to stabilize the schedule"
                    );
                }
            }

            let schedule =
                cycledit::schedule::compute_schedule(entries, schedule_params, today, cycle_end)
                    .wrap_err("failed to compute schedule")?;
            for (date, files) in &schedule {
                print_schedule_chunk(*date, files);
            }
        }

        Commands::Now { list, schedule } => {
            let today = today()?;
            let schedule_params = schedule.parse_span_days(today)?;
            let cycle_days = schedule_params.get_cycle_days();

            let git_root = git::find_git_root(&cwd)?;
            let cycle_end = read_cycle_end(&git_root, cycle_days, today)?;

            let entries = list.list_files(&cwd)?;
            let schedule =
                cycledit::schedule::compute_schedule(entries, schedule_params, today, cycle_end)
                    .wrap_err("failed to compute schedule")?;
            for (date, files) in schedule.iter().filter(|(d, _)| **d <= today) {
                print_schedule_chunk(*date, files);
            }
        }

        Commands::Check { list, schedule } => {
            let today = today()?;
            let schedule_params = schedule.parse_span_days(today)?;
            let cycle_days = schedule_params.get_cycle_days();

            let git_root = git::find_git_root(&cwd)?;
            let cycle_end = read_cycle_end(&git_root, cycle_days, today)?;

            let entries = list.list_files(&cwd)?;
            let total = entries.len();

            let schedule =
                cycledit::schedule::compute_schedule(entries, schedule_params, today, cycle_end)
                    .wrap_err("failed to compute schedule")?;
            let due: usize = schedule
                .iter()
                .filter(|(d, _)| **d <= today)
                .map(|(_, files)| files.len())
                .sum();
            if due > 0 {
                println!("WARN: Need to update {due} file(s) now (of {total} files total)");
                return Ok(100);
            }
            println!("PASS: All files up to date");
        }
    }

    Ok(0)
}

fn main() -> eyre::Result<std::convert::Infallible> {
    let exit_code = run()?;

    std::process::exit(exit_code)
}
