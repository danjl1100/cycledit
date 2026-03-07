# `cycledit`

This utility is intended for use with [password-store](https://www.passwordstore.org/), but might be useful in other applications too.
`cycledit` answers questions like:
- "Which files in this repository were modified the longest ago?"
- "If I should be editing all files regularly, which files should I edit this week?" (e.g. for repositories storing password entries as individual files, this helps you regularly update your passwords)

## Purpose
It can be a good practice to regularly update your passwords.
Changing all passwords at once can be time consuming, and carries some risk: if you make a mistake when changing multiple passwords, you might be locked out of all recovery methods.
Admittedly, getting locked out accidently seems less common in the current age of "unlock via SMS" or "unlock via the mobile app", but still good to avoid tempting fate.

The goal of this project is to help schedule your password updates at a regular interval, e.g. disperse all your password updates across the year.
Git already knows when you last modified each file, so we can query that history to schedule future updates.

## Scope
This project is agnostic to what exactly is contained in target Git repository.
The only actions performed on the Git repository are:
1. Determine whether the Git worktree is dirty or has untracked files, to report warnings to the user
2. List the files tracked by Git
3. Query date of the most recent Git commit that modified each listed file

## Usage

There are a few high-level commands that require a Git repo. (see also, detailed argument descriptions further below this list)
- `cycledit list [LIST ARGS]`
    - Prints a list of matching filepaths (relative to the Git root) with their Git modification date
        - NOTE: The modification date is printed before the filepath, for a minimal-effort table format, in the pattern: `YYYY-MM-DD FILEPATH`
    - Default order is sorted by filepath lexicographically
    - Sort by modification date using either `--oldest-first` or `--newest-first`
- `cycledit schedule [LIST ARGS] [SCHEDULE ARGS]`
    1. Identifies filepaths in modification order (oldest first) per the list args (see `cycledit list` above)
    2. Assigns "edit schedule" dates to each filepath. See  [[#Edit Schedule]]
    3. Lists groups of filepaths for each schedule date
- `cycledit now [LIST ARGS] [SCHEDULE ARGS]`
    - Identical to `cycledit schedule`, but only shows the first chunk (see `--chunk DURATION` below, equivalently excludes future scheduled edit dates, only showing entries to edit on the current date)
- `cycledit check [LIST ARGS] [SCHEDULE ARGS]`
    - Identical to `cycledit now`, except only prints success (nothing scheduled)

When outside of a Git repo,
- `cycledit` fails with a helpful error message to enter a Git repo

### Overview of specific arguments
- `[LIST ARGS]` include:
    - `[PATHSPEC]` (optional, may be repeated) which files/directories to include from the Git index. Fileglobs (e.g. *.c) can be given to list all matching files.
        (default: the closest Git worktree containing the current directory)
    - `--exclude PATHSPEC` (optional, may be repeated) same as above, but excludes the files/directories matching the pattern
- `[SCHEDULE ARGS]` include:
    - `--cycle DURATION` (default 1 year) the total duration to schedule the entries across
    - `--chunk DURATION` (default 7 days) the duration

## Implementation details

### Edit Schedule
- The earliest scheduled date is `modified_date + cycle_duration` (see `--cycle DURATION` below)
- Items are scheduled from the oldest first, such that no more than `ceil(chunk_duration / cycle_duration)` items are present in each chunk.
	For example:
	- Suppose all 100 entries were updated today (a diabolical case for scheduling), with a cycle duration of 1 year.
	- The first entry is scheduled 1 year from today (cannot scheduling before the `modified_date + cycle_duration`), and the remaining chunks would be filled accordingly spanning the next 1 year.
	- The result is a schedule ending about 2 years from today

### Stateless (excluding Git history)
The command doesn't store any state, yet consecutive invocations yield the same schedule:
- Git provides the modification date for each file
- For any files modified on the same date, the Git hash for the file entry provides a unique identifier for each unique file
- Modifying a file changes the Git blob hash, but also pushes it later in the schedule
- The result is a deterministic order for "filling" the schedule, where newly modified files are pushed to the end
