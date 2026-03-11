use std::collections::HashMap;
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

    let head_id = repo
        .head_id()
        .map_err(|e| eyre::eyre!("failed to resolve HEAD: {e}"))?;

    // Map from path -> (date, blob_hash) for most recent commit that CHANGED each file.
    let mut file_dates: HashMap<String, (Date, ObjectId)> = HashMap::new();

    // Walk commits newest-first.
    let commits: Vec<_> = repo
        .rev_walk([head_id])
        .all()
        .map_err(|e| eyre::eyre!("rev-walk failed: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| eyre::eyre!("rev-walk iteration failed: {e}"))?;

    for info in &commits {
        let commit = repo
            .find_object(info.id)
            .map_err(|e| eyre::eyre!("find commit: {e}"))?
            .try_into_commit()
            .map_err(|_| eyre::eyre!("not a commit: {}", info.id))?;

        let commit_time = commit.time().map_err(|e| eyre::eyre!("commit time: {e}"))?;
        let date = jiff::Timestamp::from_second(commit_time.seconds as i64)
            .map_err(|e| eyre::eyre!("timestamp: {e}"))?
            .to_zoned(jiff::tz::TimeZone::UTC)
            .date();

        let tree_id = commit
            .tree()
            .map_err(|e| eyre::eyre!("commit tree: {e}"))?
            .id;
        let current_blobs = walk_tree_blobs(&repo, tree_id)?;

        // Get parent's blobs for comparison.
        // decoded.parents contains &BStr hex strings; parse to ObjectId first.
        let parent_id: Option<ObjectId> = {
            let decoded = commit
                .decode()
                .map_err(|e| eyre::eyre!("decode commit: {e}"))?;
            decoded
                .parents
                .first()
                .map(|hex| gix::ObjectId::from_hex(hex))
                .transpose()
                .map_err(|e| eyre::eyre!("parse parent id: {e}"))?
        };
        let parent_blobs: HashMap<String, ObjectId> = if let Some(pid) = parent_id {
            let parent_commit = repo
                .find_object(pid)
                .map_err(|e| eyre::eyre!("find parent: {e}"))?
                .try_into_commit()
                .map_err(|_| eyre::eyre!("parent not a commit"))?;
            let parent_tree_id = parent_commit
                .tree()
                .map_err(|e| eyre::eyre!("parent tree: {e}"))?
                .id;
            walk_tree_blobs(&repo, parent_tree_id)?
        } else {
            HashMap::new()
        };

        // Record files that changed in this commit (not yet seen in a newer commit).
        for (path, blob_hash) in &current_blobs {
            if !file_dates.contains_key(path) && parent_blobs.get(path) != Some(blob_hash) {
                file_dates.insert(path.clone(), (date, *blob_hash));
            }
        }
    }

    // Apply pathspec and exclude filters, build result.
    let mut entries: Vec<FileEntry> = file_dates
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

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

/// Walk a git tree recursively and return a map of path → blob ObjectId.
fn walk_tree_blobs(
    repo: &gix::Repository,
    tree_id: ObjectId,
) -> eyre::Result<HashMap<String, ObjectId>> {
    let mut blobs = HashMap::new();
    let mut stack = vec![(String::new(), tree_id)];
    while let Some((prefix, tid)) = stack.pop() {
        let tree = repo
            .find_object(tid)
            .map_err(|e| eyre::eyre!("find tree obj: {e}"))?
            .try_into_tree()
            .map_err(|_| eyre::eyre!("not a tree: {tid}"))?;

        for entry in tree.iter() {
            let entry = entry.map_err(|e| eyre::eyre!("tree entry: {e}"))?;
            let name = entry
                .filename()
                .to_str()
                .map_err(|_| eyre::eyre!("non-utf8 filename"))?
                .to_string();
            let full_path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };

            match entry.mode().kind() {
                gix::object::tree::EntryKind::Tree => {
                    stack.push((full_path, entry.object_id()));
                }
                gix::object::tree::EntryKind::Blob
                | gix::object::tree::EntryKind::BlobExecutable => {
                    blobs.insert(full_path, entry.object_id());
                }
                _ => {}
            }
        }
    }
    Ok(blobs)
}

/// Simple glob matching against file path or filename.
fn matches_glob(pattern: &str, path: &str) -> bool {
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
            if glob_match_inner(rest_pat, txt) {
                return true;
            }
            if !txt.is_empty() {
                return glob_match_inner(pat, &txt[1..]);
            }
            false
        }
        (['*', rest_pat @ ..], _) => {
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
