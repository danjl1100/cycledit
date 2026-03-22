use eyre::Context as _;
use std::process::{Command, Output};

use self::git_ops_blocks::GitOpsBlocks;
pub use self::path_and_parent::PathAndParent;

mod git_ops_blocks;
mod path_and_parent;

#[derive(Clone, Copy, Debug)]
pub enum GitOp<'a> {
    Add { parsed: PathAndParent<'a> },
    Remove { path: &'a str },
}

pub struct TestHarness {
    dir: tempfile::TempDir,
    metrics: bool,
}

#[derive(Debug)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub status: std::process::ExitStatus,
}

impl TestHarness {
    pub fn new() -> eyre::Result<Self> {
        Ok(TestHarness {
            dir: tempfile::TempDir::new().wrap_err("failed to create tempdir")?,
            metrics: false,
        })
    }

    /// Enable `CYCLEDIT_LOG_METRICS=1` for the next `run_cli` invocation.
    pub fn with_metrics(mut self) -> Self {
        self.metrics = true;
        self
    }

    /// Parse a git state string and initialize the repo.
    ///
    /// Format:
    /// ```
    /// 2001-05-22:
    /// +folder1/sub/file1.txt
    /// +root-file.txt
    ///
    /// 2037-11-29:
    /// -root-file.txt
    /// +file2.txt
    /// ```
    pub fn init_git(self, state: &str) -> eyre::Result<Self> {
        // parse input, reporting full input on error
        let blocks = GitOpsBlocks::from_str(state).wrap_err_with(|| {
            format!("invalid input for init_git:\n--------\n{state}\n---END---")
        })?;
        self.init_git_from_blocks(&blocks)
    }
    pub fn init_git_from_blocks(self, blocks: impl BlocksIter) -> eyre::Result<Self> {
        // perform I/O
        self.init_git_io(blocks).wrap_err("init_git I/O failed")
    }
}
impl<'a> GitOpsBlocks<'a> {
    fn from_str(state: &'a str) -> eyre::Result<Self> {
        // Collect (date, ops) blocks so we commit once per date block.
        let mut blocks = Self::default();

        let mut next_date = None;
        for raw_line in state.lines() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }
            blocks
                .extend_for_line(line, &mut next_date)
                .wrap_err_with(|| format!("invalid git fixture line {line:?}"))?;
        }
        if let Some(date) = next_date.take() {
            // trailing date specified with no ops following
            return Err(Self::fail_no_ops(date));
        }

        Ok(blocks)
    }
    fn fail_no_ops(date: &str) -> eyre::Report {
        eyre::eyre!("date block {date:?} has no file operations - every commit must change files")
    }
    fn extend_for_line(
        &mut self,
        line: &'a str,
        next_date: &mut Option<&'a str>,
    ) -> eyre::Result<()> {
        if let Some(date) = line.strip_suffix(':') {
            let prev_date = next_date.replace(date);
            if let Some(date) = prev_date {
                // consecutive dates specified with no ops in the middle
                return Err(Self::fail_no_ops(date));
            }
            return Ok(());
        }
        let op = if let Some(path) = line.strip_prefix('+') {
            let Some(parsed) = PathAndParent::new(path) else {
                eyre::bail!("empty filename: {path:?}");
            };
            GitOp::Add { parsed }
        } else if let Some(path) = line.strip_prefix('-') {
            GitOp::Remove { path }
        } else {
            eyre::bail!("expected '+' or '-' filename prefix, or ':' date suffix");
        };
        if let Some(date) = next_date.take() {
            // push date with first operation
            self.push_date(date, op);
            Ok(())
        } else {
            // push additional operation
            match self.push_op_to_last_date(op) {
                Ok(()) => Ok(()),
                Err(()) => eyre::bail!("file entry before date header"),
            }
        }
    }
}

pub trait BlocksVisitor: Sized {
    type Error;
    type BlockVisitor<'a>: BlockVisitor<'a, Self, Error = Self::Error>
    where
        Self: 'a;
    /// Start a new block
    fn start_block(self, date: &str) -> Result<Self::BlockVisitor<'_>, Self::Error>;
}
pub trait BlockVisitor<'a, T> {
    type Error;
    /// Visit an operation in the current block
    fn visit(&mut self, op: GitOp<'_>) -> Result<(), Self::Error>;
    /// Ends the block and returns the original [`BlocksVisitor`]
    fn end(self) -> Result<T, Self::Error>;
}

pub trait BlocksIter {
    fn visit_all<T: BlocksVisitor>(self, visitor: T) -> Result<(), T::Error>;
}
impl BlocksIter for &GitOpsBlocks<'_> {
    fn visit_all<T: BlocksVisitor>(self, visitor: T) -> Result<(), T::Error> {
        let mut visitor = Some(visitor);
        for (date, ops) in GitOpsBlocks::iter(self) {
            let mut block = visitor
                .take()
                .expect("maintain visitor")
                .start_block(date)?;
            for &op in ops {
                block.visit(op)?;
            }
            visitor.replace(block.end()?);
        }
        Ok(())
    }
}

impl TestHarness {
    fn init_git_io(self, blocks: impl BlocksIter) -> eyre::Result<Self> {
        struct Visitor<'a> {
            dir: &'a std::path::Path,
        }
        struct VisitorInner<'a, 'b> {
            outer: Visitor<'a>,
            date: &'b str,
        }
        impl<'a> BlocksVisitor for Visitor<'a> {
            type Error = eyre::Report;
            type BlockVisitor<'b>
                = VisitorInner<'a, 'b>
            where
                Self: 'b;

            fn start_block<'b>(self, date: &'b str) -> Result<VisitorInner<'a, 'b>, Self::Error> {
                Ok(VisitorInner { outer: self, date })
            }
        }
        impl<'a, 'b> BlockVisitor<'b, Visitor<'a>> for VisitorInner<'a, 'b> {
            type Error = eyre::Report;

            fn visit(&mut self, op: GitOp<'_>) -> Result<(), Self::Error> {
                let Self {
                    outer: Visitor { dir },
                    date,
                } = self;
                match op {
                    GitOp::Add { parsed } => {
                        let parent_path = parsed.get_parent_path();
                        let name = parsed.get_name();

                        let parent_path_abs = dir.join(parent_path);
                        std::fs::create_dir_all(&parent_path_abs).wrap_err_with(|| {
                            format!(
                                "failed create dirs {} (for path {parent_path:?}, name {name:?})",
                                parent_path_abs.display()
                            )
                        })?;

                        let path = parsed.get_path();
                        // Write unique content so each file gets a unique blob hash
                        let contents = format!("{date}:{path}");

                        let file_path_abs = parent_path_abs.join(name);
                        std::fs::write(&file_path_abs, contents).wrap_err_with(|| {
                            format!("failed to write file {}", file_path_abs.display())
                        })?;

                        run_git(dir, &["add", path])?;
                    }
                    GitOp::Remove { path } => {
                        run_git(dir, &["rm", "--force", path])?;
                    }
                }
                Ok(())
            }

            fn end(self) -> Result<Visitor<'a>, Self::Error> {
                let Self {
                    outer: Visitor { dir },
                    date,
                } = self;

                let datetime = format!("{date}T12:00:00+00:00");
                run_git_env(
                    dir,
                    &["commit", "-m", &format!("commit on {date}")],
                    &[
                        ("GIT_COMMITTER_DATE", datetime.as_str()),
                        ("GIT_AUTHOR_DATE", datetime.as_str()),
                    ],
                )?;

                Ok(self.outer)
            }
        }

        let dir = self.dir.path();

        run_git(dir, &["init", "-b", "main"])?;
        run_git(dir, &["config", "user.email", "test@example.com"])?;
        run_git(dir, &["config", "user.name", "Test"])?;

        blocks.visit_all(Visitor { dir })?;

        Ok(self)
    }

    pub fn dump_fixture(&self) -> eyre::Result<String> {
        cycledit::fixture::dump_fixture_string(self.dir.path())
            .wrap_err("dump_fixture_string failed")
    }

    /// Returns the number of commits in the repo (for regression testing).
    pub fn commit_count(&self) -> eyre::Result<usize> {
        let output = Command::new("git")
            .args(["rev-list", "--count", "HEAD"])
            .current_dir(self.dir.path())
            .output()
            .wrap_err("failed to run git rev-list")?;
        assert!(output.status.success(), "git rev-list failed");
        let output = String::from_utf8_lossy(&output.stdout);
        let count: usize = output
            .trim()
            .parse()
            .wrap_err_with(|| format!("invalid git rev-list output: {output:?}"))?;
        Ok(count)
    }

    /// Run the cycledit binary with `TZ=UTC`, `CURRENT_TIME_ZONED=<time>`, and the given args.
    pub fn run_cli(self, time: &str, args: &[&str]) -> eyre::Result<CommandOutput> {
        let binary = env!("CARGO_BIN_EXE_cycledit");
        let mut cmd = Command::new(binary);
        cmd.args(args)
            .current_dir(self.dir.path())
            .env_clear()
            .env("TZ", "UTC")
            .env("CURRENT_TIME_ZONED", time)
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", std::env::var("HOME").unwrap_or_default());
        if self.metrics {
            cmd.env("CYCLEDIT_LOG_METRICS", "1");
        }
        let output: Output = cmd.output().wrap_err("failed to run cycledit binary")?;

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            status: output.status,
        })
    }
}

fn run_git(dir: &std::path::Path, args: &[&str]) -> eyre::Result<()> {
    run_git_env(dir, args, &[])
}

fn run_git_env(dir: &std::path::Path, args: &[&str], env: &[(&str, &str)]) -> eyre::Result<()> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let output = cmd
        .output()
        .wrap_err("failed to run git {args:?} in {dir:?}")?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    if output.status.success() {
        Ok(())
    } else {
        eyre::bail!("git {args:?} failed in {dir:?}: {:?}", output.status)
    }
}
