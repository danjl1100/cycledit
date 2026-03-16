//! Git repository introspection.

use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static FIND_OBJECT_CALLS: AtomicU64 = AtomicU64::new(0);

fn inc_find_object() {
    FIND_OBJECT_CALLS.fetch_add(1, Ordering::Relaxed);
}

fn get_find_object_count() -> u64 {
    FIND_OBJECT_CALLS.load(Ordering::Relaxed)
}

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
#[allow(clippy::too_many_lines)]
pub fn list_files(
    directory: &std::path::Path,
    pathspecs: &[String],
    excludes: &[String],
) -> eyre::Result<Vec<FileEntry>> {
    let repo = gix::discover(directory).wrap_err("failed to discover git repository")?;

    let head_id = repo.head_id().wrap_err("failed to resolve HEAD")?;

    // Walk commits newest-first.
    let commits: Vec<_> = repo
        .rev_walk([head_id])
        .all()
        .wrap_err("rev-walk failed")?
        .collect::<Result<Vec<_>, _>>()
        .wrap_err("rev-walk iteration failed")?;

    let mut commits_iter = commits.iter();

    // Step 1: Process HEAD to build the candidate set.  We reuse the HEAD commit
    // lookup that the loop would do anyway, so no extra find_object calls.
    let Some(head_info) = commits_iter.next() else {
        return Ok(vec![]);
    };

    inc_find_object();
    let head_commit = repo
        .find_object(head_info.id)
        .wrap_err("find HEAD commit")?
        .try_into_commit()
        .wrap_err("HEAD is not a commit")?;

    let head_date = {
        let t = head_commit.time().wrap_err("HEAD commit time")?;
        jiff::Timestamp::from_second(t.seconds as i64)
            .wrap_err("timestamp")?
            .to_zoned(jiff::tz::TimeZone::UTC)
            .date()
    };

    let head_tree_id = head_commit.tree().wrap_err("HEAD tree")?.id;
    let all_head_files: HashMap<String, ObjectId> = {
        let mut c = HashMapCollector::new();
        walk_tree_blobs(&repo, head_tree_id, &mut c)?;
        c.into_map()
    };

    // Apply pathspec and exclude filters immediately (Step 1).
    let head_filtered: HashMap<String, ObjectId> = all_head_files
        .into_iter()
        .filter(|(path, _)| {
            let matches_include =
                pathspecs.is_empty() || pathspecs.iter().any(|spec| matches_glob(spec, path));
            let matches_exclude = excludes.iter().any(|spec| matches_glob(spec, path));
            matches_include && !matches_exclude
        })
        .collect();

    // Step 2: Track files that still need a date; exit early once all are dated.
    let mut remaining: HashSet<String> = head_filtered.keys().cloned().collect();
    let mut file_dates: HashMap<String, (Date, ObjectId)> = HashMap::new();

    // Date HEAD's changes against its parent.
    {
        let head_parent_id: Option<ObjectId> = {
            let decoded = head_commit.decode().wrap_err("decode HEAD commit")?;
            decoded
                .parents
                .first()
                .map(|hex| gix::ObjectId::from_hex(hex))
                .transpose()
                .wrap_err("parse HEAD parent id")?
        };
        let head_parent_subset: HashMap<String, ObjectId> = if let Some(pid) = head_parent_id {
            inc_find_object();
            let parent_commit = repo
                .find_object(pid)
                .wrap_err("find HEAD parent")?
                .try_into_commit()
                .wrap_err("HEAD parent not a commit")?;
            let parent_tree_id = parent_commit.tree().wrap_err("HEAD parent tree")?.id;
            // Step 3: filtered visitor prunes subtrees with no remaining files.
            let mut v = RemainingFilteredCollector::new(&remaining);
            walk_tree_blobs(&repo, parent_tree_id, &mut v)?;
            v.into_map()
        } else {
            HashMap::new()
        };

        for (path, blob_hash) in &head_filtered {
            if head_parent_subset.get(path) != Some(blob_hash) {
                file_dates.insert(path.clone(), (head_date, *blob_hash));
                remaining.remove(path);
            }
        }
    }

    // Walk older commits with the filtered visitor.
    for info in commits_iter {
        if remaining.is_empty() {
            break;
        }

        inc_find_object();
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
        // Step 3: filtered visitor prunes subtrees with no remaining files.
        let current_subset: HashMap<String, ObjectId> = {
            let mut v = RemainingFilteredCollector::new(&remaining);
            walk_tree_blobs(&repo, tree_id, &mut v)?;
            v.into_map()
        };

        let parent_id: Option<ObjectId> = {
            let decoded = commit.decode().wrap_err("decode commit")?;
            decoded
                .parents
                .first()
                .map(|hex| gix::ObjectId::from_hex(hex))
                .transpose()
                .wrap_err("parse parent id")?
        };
        let parent_subset: HashMap<String, ObjectId> = if let Some(pid) = parent_id {
            inc_find_object();
            let parent_commit = repo
                .find_object(pid)
                .wrap_err("find parent")?
                .try_into_commit()
                .wrap_err("parent not a commit")?;
            let parent_tree_id = parent_commit.tree().wrap_err("parent tree")?.id;
            let mut v = RemainingFilteredCollector::new(&remaining);
            walk_tree_blobs(&repo, parent_tree_id, &mut v)?;
            v.into_map()
        } else {
            HashMap::new()
        };

        for (path, blob_hash) in &current_subset {
            if parent_subset.get(path) != Some(blob_hash) {
                file_dates.insert(path.clone(), (date, *blob_hash));
                remaining.remove(path);
            }
        }
    }

    let mut entries: Vec<FileEntry> = file_dates
        .into_iter()
        .map(|(path, (date, blob_hash))| FileEntry {
            date,
            blob_hash,
            path: PathBuf::from(path),
        })
        .collect();

    entries.sort_by(|a, b| a.path.cmp(&b.path));

    if std::env::var("CYCLEDIT_LOG_METRICS").as_deref() == Ok("1") {
        eprintln!("metrics: find_object_calls={}", get_find_object_count());
    }

    Ok(entries)
}

/// Visitor called by [`walk_tree_blobs`] for each tree entry.
pub(crate) trait TreeVisitor {
    /// Called for each directory entry.  Return `Continue(true)` to descend,
    /// `Continue(false)` to skip, or `Break(())` to abort the walk.
    fn is_include_dir(&mut self, prefix: &str, name: &str) -> ControlFlow<(), bool>;

    /// Called for each blob entry.  Return `Break(())` to abort the walk.
    fn visit_blob(&mut self, prefix: &str, name: &str, object_id: ObjectId) -> ControlFlow<()>;
}

/// [`TreeVisitor`] that collects all blob entries into a [`HashMap`].
pub(crate) struct HashMapCollector(HashMap<String, ObjectId>);

impl HashMapCollector {
    pub(crate) fn new() -> Self {
        Self(HashMap::new())
    }

    pub(crate) fn into_map(self) -> HashMap<String, ObjectId> {
        self.0
    }
}

impl TreeVisitor for HashMapCollector {
    fn is_include_dir(&mut self, _prefix: &str, _name: &str) -> ControlFlow<(), bool> {
        ControlFlow::Continue(true)
    }

    fn visit_blob(&mut self, prefix: &str, name: &str, object_id: ObjectId) -> ControlFlow<()> {
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        self.0.insert(path, object_id);
        ControlFlow::Continue(())
    }
}

/// [`TreeVisitor`] that collects only blob entries whose path is in `remaining`,
/// pruning subtrees that contain no remaining files.
struct RemainingFilteredCollector<'a> {
    remaining: &'a HashSet<String>,
    dir_prefixes: HashSet<String>,
    result: HashMap<String, ObjectId>,
}

impl<'a> RemainingFilteredCollector<'a> {
    fn new(remaining: &'a HashSet<String>) -> Self {
        let mut dir_prefixes = HashSet::new();
        for path in remaining {
            // "a/b/c.txt" → insert "a", then "a/b"
            let parts: Vec<&str> = path.split('/').collect();
            for i in 1..parts.len() {
                dir_prefixes.insert(parts[..i].join("/"));
            }
        }
        Self {
            remaining,
            dir_prefixes,
            result: HashMap::new(),
        }
    }

    fn into_map(self) -> HashMap<String, ObjectId> {
        self.result
    }
}

impl TreeVisitor for RemainingFilteredCollector<'_> {
    fn is_include_dir(&mut self, prefix: &str, name: &str) -> ControlFlow<(), bool> {
        let full = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        ControlFlow::Continue(self.dir_prefixes.contains(&full))
    }

    fn visit_blob(&mut self, prefix: &str, name: &str, object_id: ObjectId) -> ControlFlow<()> {
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        if self.remaining.contains(&path) {
            self.result.insert(path, object_id);
        }
        ControlFlow::Continue(())
    }
}

/// Walk a git tree recursively, calling `visitor` for each entry.
pub(crate) fn walk_tree_blobs(
    repo: &gix::Repository,
    tree_id: ObjectId,
    visitor: &mut impl TreeVisitor,
) -> eyre::Result<()> {
    let mut stack = vec![(String::new(), tree_id)];
    while let Some((prefix, tid)) = stack.pop() {
        inc_find_object();
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
                .wrap_err("non-utf8 filename")?;

            match entry.mode().kind() {
                gix::object::tree::EntryKind::Tree => {
                    let ControlFlow::Continue(include) =
                        visitor.is_include_dir(&prefix, name)
                    else {
                        return Ok(());
                    };
                    if include {
                        let child_prefix = if prefix.is_empty() {
                            name.to_string()
                        } else {
                            format!("{prefix}/{name}")
                        };
                        stack.push((child_prefix, entry.object_id()));
                    }
                }
                gix::object::tree::EntryKind::Blob
                | gix::object::tree::EntryKind::BlobExecutable => {
                    let ControlFlow::Continue(()) =
                        visitor.visit_blob(&prefix, name, entry.object_id())
                    else {
                        return Ok(());
                    };
                }
                _ => {}
            }
        }
    }
    Ok(())
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
