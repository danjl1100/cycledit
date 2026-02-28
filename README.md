# `cycledit`

This helper utility is intended for use with [password-store](https://www.passwordstore.org/) (but might have applications elsewhere, too)

## Purpose
It can be a good practice to regularly update your passwords. Changing all passwords at once can be time consuming, so what if you could magically schedule your password updates across the year?


## Usage

When inside a GIT repo,
- Run `cycledit list` to:
    1. Print a list of matching files with their GIT modification date
- Run `cycledit now` to:
    1. Searches for all files matching a pattern in the current GIT repo
    2. Identifies the modification date based on the most recent commits for each file
