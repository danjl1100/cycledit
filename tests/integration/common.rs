use std::process::{Command, Output};

pub struct TestHarness {
    dir: tempfile::TempDir,
}

pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub status: std::process::ExitStatus,
}

impl TestHarness {
    pub fn new() -> Self {
        TestHarness {
            dir: tempfile::TempDir::new().expect("create tempdir"),
        }
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
    pub fn init_git(self, state: &str) -> Self {
        let dir = self.dir.path();

        // git init
        run_git(dir, &["init", "-b", "main"]);
        run_git(dir, &["config", "user.email", "test@example.com"]);
        run_git(dir, &["config", "user.name", "Test"]);

        let mut current_date: Option<String> = None;

        for raw_line in state.lines() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }

            if let Some(date) = line.strip_suffix(':') {
                // validate it looks like a date
                current_date = Some(date.to_string());
                continue;
            }

            let date = current_date
                .as_ref()
                .expect("file entry before date header");

            if let Some(path_str) = line.strip_prefix('+') {
                let file_path = dir.join(path_str);
                std::fs::create_dir_all(file_path.parent().unwrap()).expect("create dirs");
                // Write unique content so each file gets a unique blob hash
                std::fs::write(&file_path, format!("{date}:{path_str}")).expect("write file");
                run_git(dir, &["add", path_str]);
            } else if let Some(path_str) = line.strip_prefix('-') {
                run_git(dir, &["rm", "--force", path_str]);
            } else {
                panic!("unexpected line in git state: {line}");
            }

            let datetime = format!("{date}T12:00:00+00:00");
            run_git_env(
                dir,
                &[
                    "commit",
                    "--allow-empty",
                    "-m",
                    &format!("commit on {date}"),
                ],
                &[
                    ("GIT_COMMITTER_DATE", datetime.as_str()),
                    ("GIT_AUTHOR_DATE", datetime.as_str()),
                ],
            );
        }

        self
    }

    /// Run the cycledit binary with TZ=UTC, CURRENT_TIME_ZONED=<time>, and the given args.
    pub fn run_cli(self, time: &str, args: &[&str]) -> CommandOutput {
        let binary = env!("CARGO_BIN_EXE_cycledit");
        let output: Output = Command::new(binary)
            .args(args)
            .current_dir(self.dir.path())
            .env_clear()
            .env("TZ", "UTC")
            .env("CURRENT_TIME_ZONED", time)
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", std::env::var("HOME").unwrap_or_default())
            .output()
            .expect("run cycledit binary");

        CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            status: output.status,
        }
    }
}

fn run_git(dir: &std::path::Path, args: &[&str]) {
    run_git_env(dir, args, &[]);
}

fn run_git_env(dir: &std::path::Path, args: &[&str], env: &[(&str, &str)]) {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let status = cmd.status().expect("run git");
    assert!(status.success(), "git {args:?} failed in {dir:?}");
}
