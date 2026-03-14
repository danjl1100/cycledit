use std::collections::HashMap;
use std::path::Path;

use eyre::WrapErr;
use gix::ObjectId;

/// Introspect a git repository and emit a fixture string compatible with
/// `TestHarness::init_git`. Commits are emitted oldest-first; merge commits
/// (empty diff against first parent) are skipped.
pub fn dump_fixture_string(path: &Path) -> eyre::Result<String> {
    let repo = gix::discover(path).wrap_err("failed to discover git repository")?;

    let head_id = repo.head_id().wrap_err("failed to resolve HEAD")?;

    let mut commits: Vec<_> = repo
        .rev_walk([head_id])
        .all()
        .wrap_err("rev-walk failed")?
        .collect::<Result<Vec<_>, _>>()
        .wrap_err("rev-walk iteration failed")?;
    commits.reverse(); // oldest-first

    let mut output = String::new();

    for info in &commits {
        let commit = repo
            .find_object(info.id)
            .wrap_err("find commit")?
            .try_into_commit()
            .wrap_err_with(|| format!("not a commit: {}", info.id))?;

        let commit_time = commit.time().wrap_err("commit time")?;
        let date = jiff::Timestamp::from_second(commit_time.seconds as i64)
            .wrap_err("timestamp")?
            .to_zoned(jiff::tz::TimeZone::UTC)
            .date();

        let tree_id = commit.tree().wrap_err("commit tree")?.id;
        let current_blobs = crate::git::walk_tree_blobs(&repo, tree_id)?;

        let parent_blobs: HashMap<String, ObjectId> = {
            let decoded = commit.decode().wrap_err("decode commit")?;
            if let Some(hex) = decoded.parents.first() {
                let pid = gix::ObjectId::from_hex(hex).wrap_err("parse parent id")?;
                let parent_commit = repo
                    .find_object(pid)
                    .wrap_err("find parent")?
                    .try_into_commit()
                    .wrap_err("parent not a commit")?;
                let parent_tree_id = parent_commit.tree().wrap_err("parent tree")?.id;
                crate::git::walk_tree_blobs(&repo, parent_tree_id)?
            } else {
                HashMap::new()
            }
        };

        let mut diff_lines: Vec<(char, &str)> = vec![];

        for (p, blob) in &current_blobs {
            if parent_blobs.get(p) != Some(blob) {
                diff_lines.push(('+', p));
            }
        }
        for p in parent_blobs.keys() {
            if !current_blobs.contains_key(p) {
                diff_lines.push(('-', p));
            }
        }

        if diff_lines.is_empty() {
            continue;
        }

        diff_lines.sort_by_key(|(_, line)| *line);

        output.push_str(&format!("\n{date}:\n"));
        for (symbol, line) in &diff_lines {
            output.push(*symbol);
            output.push_str(line);
            output.push('\n');
        }
    }

    Ok(output)
}
