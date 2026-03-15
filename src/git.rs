//! Git repository introspection.

use std::collections::HashMap;
use std::path::PathBuf;

use eyre::WrapErr;
use gix::ObjectId;
use gix::bstr::ByteSlice;
use jiff::civil::Date;

/// File in the Git repository
pub struct FileEntry {
    date: Date,
    blob_hash: ObjectId,
    path: PathBuf,
}
impl FileEntry {
    /// Returns the date of the last modification (commit) for this file
    #[must_use]
    pub fn get_date(&self) -> Date {
        self.date
    }
    /// Returns the path relative to the Git repo root
    #[must_use]
    pub fn get_path(&self) -> &std::path::Path {
        &self.path
    }
    pub(crate) fn get_blob_hash(&self) -> &ObjectId {
        &self.blob_hash
    }
}

/// List files tracked in the git index, with the date of the most recent commit
/// that modified each file. Entries are sorted lexicographically by path.
///
/// `pathspecs` — if non-empty, only files matching at least one spec are included.
/// `excludes`  — files matching any exclude spec are removed.
///
/// # Errors
/// Returns an error if reading the Git repository fails or it contains invalid data.
pub fn list_files(
    directory: &std::path::Path,
    pathspecs: &[String],
    excludes: &[String],
) -> eyre::Result<Vec<FileEntry>> {
    let repo = gix::discover(directory).wrap_err("failed to discover git repository")?;

    let head_id = repo.head_id().wrap_err("failed to resolve HEAD")?;

    // Map from path -> (date, blob_hash) for most recent commit that CHANGED each file.
    let mut file_dates: HashMap<String, (Date, ObjectId)> = HashMap::new();

    // Walk commits newest-first.
    let commits: Vec<_> = repo
        .rev_walk([head_id])
        .all()
        .wrap_err("rev-walk failed")?
        .collect::<Result<Vec<_>, _>>()
        .wrap_err("rev-walk iteration failed")?;

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
        let current_blobs = walk_tree_blobs(&repo, tree_id)?;

        // Get parent's blobs for comparison.
        // decoded.parents contains &BStr hex strings; parse to ObjectId first.
        let parent_id: Option<ObjectId> = {
            let decoded = commit.decode().wrap_err("decode commit")?;
            decoded
                .parents
                .first()
                .map(|hex| gix::ObjectId::from_hex(hex))
                .transpose()
                .wrap_err("parse parent id")?
        };
        let parent_blobs: HashMap<String, ObjectId> = if let Some(pid) = parent_id {
            let parent_commit = repo
                .find_object(pid)
                .wrap_err("find parent")?
                .try_into_commit()
                .wrap_err("parent not a commit")?;
            let parent_tree_id = parent_commit.tree().wrap_err("parent tree")?.id;
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

/// Walk a git tree recursively and return a map of path → blob [`ObjectId`].
pub(crate) fn walk_tree_blobs(
    repo: &gix::Repository,
    tree_id: ObjectId,
) -> eyre::Result<HashMap<String, ObjectId>> {
    let mut blobs = HashMap::new();
    let mut stack = vec![(String::new(), tree_id)];
    while let Some((prefix, tid)) = stack.pop() {
        let tree = repo
            .find_object(tid)
            .wrap_err("find tree obj")?
            .try_into_tree()
            .wrap_err_with(|| format!("not a tree: {tid}"))?;

        for entry in tree.iter() {
            let entry = entry.wrap_err("tree entry")?;
            let name = entry
                .filename()
                .to_str()
                .wrap_err("non-utf8 filename")?
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
    glob_match::glob_match(pattern, path)
        || PathBuf::from(path)
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| glob_match::glob_match(pattern, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_question_mark_matches_single_char() {
        assert!(matches_glob("file?.txt", "file1.txt"));
    }

    #[test]
    fn glob_question_mark_does_not_cross_slash() {
        assert!(!matches_glob("dir?.txt", "dir/a.txt"));
    }

    #[test]
    fn glob_question_mark_does_not_match_empty() {
        assert!(!matches_glob("file?.txt", "file.txt"));
    }

    #[test]
    fn glob_character_class() {
        assert!(matches_glob("file[0-9].txt", "file3.txt"));
        assert!(!matches_glob("file[0-9].txt", "filea.txt"));
    }

    #[test]
    fn glob_star_does_not_cross_slash() {
        assert!(!matches_glob("src/*.rs", "src/foo/bar.rs"));
    }

    #[test]
    fn glob_double_star_crosses_slash() {
        assert!(matches_glob("src/**/*.rs", "src/foo/bar.rs"));
        assert!(matches_glob("src/**/*.rs", "src/foo/baz/qux/bar.rs"));
        assert!(!matches_glob("src/*/*.rs", "src/foo/baz/bar.rs"));
    }

    #[test]
    fn glob_filename_fallback() {
        assert!(matches_glob("*.rs", "src/main.rs"));
    }
}
