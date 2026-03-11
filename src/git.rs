use std::path::PathBuf;

use gix::ObjectId;
use gix::bstr::ByteSlice;
use jiff::civil::Date;

pub struct FileEntry {
    pub date: Date,
    pub blob_hash: ObjectId,
    pub path: PathBuf,
}

/// List files tracked in the git index, with the date of the most recent commit
/// that modified each file. Entries are sorted lexicographically by path.
///
/// `pathspecs` — if non-empty, only files matching at least one spec are included.
/// `excludes`  — files matching any exclude spec are removed.
pub fn list_files(
    repo_path: &std::path::Path,
    pathspecs: &[String],
    excludes: &[String],
) -> eyre::Result<Vec<FileEntry>> {
    let repo = gix::open(repo_path)
        .map_err(|e| eyre::eyre!("not a git repository (or any of the parent directories): {e}"))?;

    // Walk commits from HEAD to build a map: path -> (date, blob_hash)
    // We use gix to traverse the commit graph and track the most recent commit per path.
    let head_id = repo
        .head_id()
        .map_err(|e| eyre::eyre!("failed to resolve HEAD: {e}"))?;

    // Map from path string -> (date, blob_hash) for the most recently seen commit
    let mut file_map: std::collections::HashMap<String, (Date, ObjectId)> =
        std::collections::HashMap::new();

    // Walk commits oldest-to-newest using BFS from HEAD; we record the LAST
    // (most-recent-commit) that touched each path.
    // Strategy: collect all commits in topo order (newest first), then iterate.
    let commits: Vec<_> = repo
        .rev_walk([head_id])
        .all()
        .map_err(|e| eyre::eyre!("rev-walk failed: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| eyre::eyre!("rev-walk iteration failed: {e}"))?;

    // commits are in newest-first order; we iterate newest-first and keep the
    // FIRST occurrence (most recent commit) for each path.
    for info in &commits {
        let commit = repo
            .find_object(info.id)
            .map_err(|e| eyre::eyre!("find commit object: {e}"))?
            .try_into_commit()
            .map_err(|e| eyre::eyre!("not a commit: {e}"))?;

        let commit_time = commit.time().map_err(|e| eyre::eyre!("commit time: {e}"))?;
        let secs = commit_time.seconds;
        let date = jiff::Timestamp::from_second(secs as i64)
            .map_err(|e| eyre::eyre!("timestamp: {e}"))?
            .to_zoned(jiff::tz::TimeZone::UTC)
            .date();

        let tree = commit.tree().map_err(|e| eyre::eyre!("commit tree: {e}"))?;

        // Walk the tree to enumerate all blobs
        let mut stack = vec![(String::new(), tree.id)];
        while let Some((prefix, tree_id)) = stack.pop() {
            let tree_obj = repo
                .find_object(tree_id)
                .map_err(|e| eyre::eyre!("find tree: {e}"))?
                .try_into_tree()
                .map_err(|e| eyre::eyre!("not a tree: {e}"))?;

            for entry in tree_obj.iter() {
                let entry = entry.map_err(|e| eyre::eyre!("tree entry: {e}"))?;
                let name = entry
                    .filename()
                    .to_str()
                    .map_err(|_| eyre::eyre!("non-utf8 filename"))?
                    .to_string();
                let full_path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };

                match entry.mode().kind() {
                    gix::object::tree::EntryKind::Tree => {
                        stack.push((full_path, entry.object_id()));
                    }
                    gix::object::tree::EntryKind::Blob
                    | gix::object::tree::EntryKind::BlobExecutable => {
                        // Only record if not already seen (we want most-recent commit)
                        file_map
                            .entry(full_path)
                            .or_insert_with(|| (date, entry.object_id()));
                    }
                    _ => {}
                }
            }
        }
    }

    // Apply pathspec and exclude filters
    let entries: Vec<FileEntry> = file_map
        .into_iter()
        .filter(|(path, _)| {
            let matches_include =
                pathspecs.is_empty() || pathspecs.iter().any(|spec| matches_glob(spec, path));
            let matches_exclude = excludes.iter().any(|spec| matches_glob(spec, path));
            matches_include && !matches_exclude
        })
        .map(|(path, (date, blob_hash))| FileEntry {
            date,
            blob_hash,
            path: PathBuf::from(path),
        })
        .collect();

    // Sort lexicographically by path
    let mut entries = entries;
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(entries)
}

/// Simple glob matching: `*` matches any sequence of non-`/` chars, `**` matches anything.
/// Matches against the full path or just the filename component.
fn matches_glob(pattern: &str, path: &str) -> bool {
    // Try matching against the full path and also just the filename
    glob_match(pattern, path)
        || PathBuf::from(path)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|name| glob_match(pattern, name))
            .unwrap_or(false)
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    glob_match_inner(&pat, &txt)
}

fn glob_match_inner(pat: &[char], txt: &[char]) -> bool {
    match (pat, txt) {
        ([], []) => true,
        ([], _) => false,
        (['*', '*', rest_pat @ ..], _) => {
            // ** matches any sequence including /
            if glob_match_inner(rest_pat, txt) {
                return true;
            }
            if !txt.is_empty() {
                return glob_match_inner(pat, &txt[1..]);
            }
            false
        }
        (['*', rest_pat @ ..], _) => {
            // * matches any sequence not including /
            if glob_match_inner(rest_pat, txt) {
                return true;
            }
            if !txt.is_empty() && txt[0] != '/' {
                return glob_match_inner(pat, &txt[1..]);
            }
            false
        }
        ([p, rest_pat @ ..], [t, rest_txt @ ..]) if p == t => glob_match_inner(rest_pat, rest_txt),
        _ => false,
    }
}
