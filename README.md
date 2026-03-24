# `cycledit`

This utility is intended for use with [password-store](https://www.passwordstore.org/), but might be useful in other applications too.

`cycledit` answers questions like:
- "Which files in this repository were modified the longest ago?"
- "If I should be editing all files regularly on a schedule, which files should I edit this week?" (e.g. for repositories storing password entries as individual files, this helps you regularly update your passwords)

## Purpose
It can be a good practice to regularly update your passwords.
Changing all passwords at once can be time consuming, and carries some risk: if you make a mistake when changing multiple passwords, you might be locked out of all recovery methods.
(Admittedly, getting locked out accidently seems less common in the current age of "unlock via SMS" or "unlock via the mobile app", but still good to avoid tempting fate.)

The goal of this project is to help schedule your password updates at a regular interval, e.g. disperse all your password updates across the year.
Git already knows when you last modified each file, so we can query that history to schedule future updates.

## Scope
This project is agnostic to what exactly is contained in target Git repository.
The only actions performed on the Git repository are:
1. List the files tracked by Git
2. Query date of the most recent Git commit that modified each listed file

## Usage

There are a few high-level commands that require a Git repo. (see also [[#Argument Details]])
- `cycledit list [LIST ARGS]`
	- Prints a list of matching filepaths (relative to the Git root) with their Git modification date
		- NOTE: The modification date is printed before the filepath, for a minimal-effort table format, in the pattern: `YYYY-MM-DD FILEPATH`
	- The output is sorted by filepath lexicographically
		- NOTE: The user can trivially sort by date by piping to `sort`, so this functionality is not implemented. Small-scope tools can compose in wonderful ways!
	- Sample output:
		```
		2024-12-31 a_recent_file.txt
		2005-10-12 file1.txt
		2012-01-01 file2.txt
		```
- `cycledit schedule [LIST ARGS] [SCHEDULE ARGS]`
	- (1.) Identifies filepaths in modification order (oldest first) per the list args (see `cycledit list` above)
	- (2.) Assigns "edit schedule" dates to each filepath. See [[#Edit Schedule]]
	- (3.) Lists chunks of filepaths for each schedule date
	- Sample output (assuming a current date of 2026-02-03)
		```
		2026-02-03:
			file1.txt
		2026-06-03:
			file2.txt
		2026-10-03:
			a_recent_file.txt
		```
- `cycledit now [LIST ARGS] [SCHEDULE ARGS]`
	- Identical to `cycledit schedule`, but only shows chunks whose scheduled date is on or before today (filters out future scheduled edit dates)
	- Sample output
		```
		2026-02-03:
			file1.txt
		```
- `cycledit check [LIST ARGS] [SCHEDULE ARGS]`
	- Identical to `cycledit now`, except only prints success (nothing scheduled) or a warning (count scheduled for today)
	- Sample warning output (exit code = 100)
		```
		WARN: Need to update 1 file now (of 3 files total)
		```
	- Sample success output (exit code = 0)
		```
		PASS: All files up to date
		```

When outside of a Git repo,
- `cycledit` fails with a helpful error message to enter a Git repo

### Argument Details
- `[LIST ARGS]` include:
	- `[PATHSPEC]` (optional, may be repeated) which files/directories to include from the Git index. Fileglobs (e.g. `*.c`) can be given to list all matching files.
		(default: the entirety of the closest Git worktree containing the current directory)
	- `--exclude PATHSPEC` (optional, may be repeated) same as above, but excludes the files/directories matching the pattern
- `[SCHEDULE ARGS]` include:
	- `--cycle DURATION` (default 1 year) the total duration to schedule the entries across
	- `--chunk DURATION` (default 7 days) the duration of each scheduling chunk
	- NOTE: Duration arguments are parsed per the format in [jiff span-format docs](https://docs.rs/jiff/latest/jiff/fmt/temporal/index.html#span-format)

## Implementation details

### Stateless (excluding Git history)
The command doesn't store any state, yet consecutive invocations yield the same schedule:
- Git provides the modification date for each file
- For any files modified on the same date, the Git hash for the file entry provides a unique identifier for each unique file
- Modifying a file changes the Git blob hash, but also pushes it later in the schedule
- The result is a deterministic order for "filling" the schedule, where newly modified files are pushed to the end

### Edit Schedule
- The earliest scheduled date is `modified_date + cycle_duration` (see `--cycle DURATION` above)
- Items are scheduled from the oldest first, such that no more than `ceil(entries_count / ceil(cycle_duration / chunk_duration))` items are present in each chunk.
	For example:
	- Suppose all 100 entries were updated today (a diabolical case for scheduling), with a cycle duration of 1 year.
	- The first entry would be scheduled 1 year from today (cannot schedule before the `modified_date + cycle_duration`), and the remaining chunks would be filled accordingly spanning the next 1 year.
	- The result is a schedule ending about 2 years from today


### Notable Dependencies
- dependencies
	- [`jiff`](https://crates.io/crates/jiff) for parsing durations from the CLI and date/time arithmetic
	- [`gix`](https://crates.io/crates/gix) for readonly access to the Git state
- dev dependencies
	- [`insta`](https://crates.io/crates/insta) for snapshot tests

### Test Structure
Integration tests use inline string constants to define each test:
- Input - the only test inputs are the Git state, simulated time, and CLI arguments. For ease of defining and reading tests, all are defined in string constants fed to a common test harness.
	```rust
	let output = TestHarness::new()
		.init_git("<GIT STATE STRING>") // trims each line, ignores empty lines
		.run_cli("YYYY-MM-DDTHH:MM:SS-OFFSET[TZ]", &["<ARG1>", "<ARG2>"]); // verbatim args passed to sub-command, no pre-processing
	```
	- Git state - groups of commit dates with the files updated (+) or removed (-) (NOTE: file contents are not irrelevant)
		```text
		2001-05-22:
		+folder1/sub-folder/file1.txt
		+root-file.txt
		+file2.txt

		2037-11-29:
		-root-file.txt
		+file2.txt

		2037-11-30:
		+file2.txt
		```
	- CLI time - timestamp string passed as-is, as env var `CURRENT_TIME_ZONED`
	- CLI arguments - literal array of string constants to pass to the CLI binary
	- NOTE: Environment variables are generally not used in tests. The test harness automatically sets `TZ=UTC` env var to avoid leaking the local PC's timezone into tests.
- Output - the stdout, stderr, and exit code
	- Most tests use snapshot testing to verify specific stdout output (for success cases) or stderr (for error cases)
