//! Git repository introspection.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static METRICS: Metrics = Metrics::new();

struct Metrics {
    find_object_calls: AtomicU64,
    visited_files: AtomicU64,
    visited_dirs: AtomicU64,
}
impl Metrics {
    const fn new() -> Self {
        Self {
            find_object_calls: AtomicU64::new(0),
            visited_files: AtomicU64::new(0),
            visited_dirs: AtomicU64::new(0),
        }
    }
    fn inc_find_object(&self) {
        self.find_object_calls.fetch_add(1, Ordering::Relaxed);
    }
    fn inc_visit_file(&self) {
        self.visited_files.fetch_add(1, Ordering::Relaxed);
    }
    fn inc_visit_dir(&self) {
        self.visited_dirs.fetch_add(1, Ordering::Relaxed);
    }
}
impl std::fmt::Display for Metrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            find_object_calls,
            visited_files,
            visited_dirs,
        } = self;
        let find_object_calls = find_object_calls.load(Ordering::Relaxed);
        let visited_dirs = visited_dirs.load(Ordering::Relaxed);
        let visited_files = visited_files.load(Ordering::Relaxed);
        write!(
            f,
            "metrics: find_object_calls={find_object_calls}, visited_dirs={visited_dirs}, visited_files={visited_files}"
        )
    }
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
pub fn list_files(
    directory: &std::path::Path,
    pathspecs: &[String],
    excludes: &[String],
) -> eyre::Result<Vec<FileEntry>> {
    let repo = gix::discover(directory).wrap_err("failed to discover git repository")?;

    let head_id = repo.head_id().wrap_err("failed to resolve HEAD")?;

    // Walk commits newest-first.
    let mut commits_iter = repo.rev_walk([head_id]).all().wrap_err("rev-walk failed")?;

    // Step 1: Process HEAD to build the candidate set.  We reuse the HEAD commit
    // lookup that the loop would do anyway, so no extra find_object calls.
    let Some(head_info) = commits_iter.next() else {
        return Ok(vec![]);
    };
    let head_info = head_info.wrap_err("rev-walk failed")?;

    METRICS.inc_find_object();
    let head_commit = repo
        .find_object(head_info.id)
        .wrap_err("find HEAD commit")?
        .try_into_commit()
        .wrap_err("HEAD is not a commit")?;

    let head_date = commit_date(head_commit.time().wrap_err("HEAD commit time")?.seconds)?;
    let head_tree_id = head_commit.tree().wrap_err("HEAD tree")?.id;

    // Apply pathspec and exclude filters immediately (Step 1).
    let head_filtered = {
        let mut v = IncludeExcludeCollector::new(pathspecs, excludes);
        walk_tree_blobs(&repo, head_tree_id, &mut v)?;
        v.into_map()
    };

    // Step 2: Track files that still need a date; exit early once all are dated.
    let mut remaining: HashSet<String> = head_filtered.keys().cloned().collect();
    // BTreeMap keeps entries sorted by path, so no explicit sort is needed at the end.
    let mut file_dates: BTreeMap<String, (Date, ObjectId)> = BTreeMap::new();

    // Date HEAD's changes against its parent.
    let head_parent_id = {
        let decoded = head_commit.decode().wrap_err("decode HEAD commit")?;
        decoded
            .parents
            .first()
            .map(|hex| gix::ObjectId::from_hex(hex))
            .transpose()
            .wrap_err("parse HEAD parent id")?
    };
    let (head_parent_tree_id, head_parent_subset) =
        walk_parent_blobs(&repo, head_parent_id, &remaining)?;
    apply_diff(
        &head_filtered,
        &head_parent_subset,
        head_date,
        &mut file_dates,
        &mut remaining,
    );

    // Cache the most recently walked parent tree so the next loop iteration can reuse it
    // as its current tree (linear-history optimisation: commit N-1's tree == commit N's
    // parent tree).  For merge commits the cache simply won't hit.
    //
    // Note: gix's built-in object cache would save disk I/O for repeated reads of the
    // same object, but walking the tree and building the HashMap would still be repeated;
    // the explicit cache here avoids that redundant work entirely.
    let mut cached_tree: Option<(ObjectId, HashMap<String, ObjectId>)> =
        head_parent_tree_id.map(|id| (id, head_parent_subset));

    // Walk older commits with the filtered visitor.
    for info in commits_iter {
        if remaining.is_empty() {
            break;
        }
        let info = info.wrap_err("rev-walk failed")?;

        METRICS.inc_find_object();
        let commit = repo
            .find_object(info.id)
            .wrap_err("find commit")?
            .try_into_commit()
            .wrap_err_with(|| format!("not a commit: {}", info.id))?;

        let date = commit_date(commit.time().wrap_err("commit time")?.seconds)?;
        let tree_id = commit.tree().wrap_err("commit tree")?.id;

        // Step 3: filtered visitor prunes subtrees with no remaining files.
        // Reuse the cached parent tree walk when the tree id matches (linear history).
        let current_subset = match cached_tree.take() {
            Some((cached_id, mut cached_map)) if cached_id == tree_id => {
                // The cached map may contain paths dated in the previous iteration;
                // filter it down to only the still-undated files before comparing.
                cached_map.retain(|path, _| remaining.contains(path.as_str()));
                cached_map
            }
            _ => {
                let mut v = RemainingFilteredCollector::new(&remaining);
                walk_tree_blobs(&repo, tree_id, &mut v)?;
                v.into_map()
            }
        };

        let parent_id = {
            let decoded = commit.decode().wrap_err("decode commit")?;
            decoded
                .parents
                .first()
                .map(|hex| gix::ObjectId::from_hex(hex))
                .transpose()
                .wrap_err("parse parent id")?
        };
        let (parent_tree_id, parent_subset) = walk_parent_blobs(&repo, parent_id, &remaining)?;
        apply_diff(
            &current_subset,
            &parent_subset,
            date,
            &mut file_dates,
            &mut remaining,
        );

        // Cache this parent for the next iteration.
        cached_tree = parent_tree_id.map(|id| (id, parent_subset));
    }

    let entries: Vec<FileEntry> = file_dates
        .into_iter()
        .map(|(path, (date, blob_hash))| FileEntry {
            date,
            blob_hash,
            path: PathBuf::from(path),
        })
        .collect();

    let is_log_metrics = std::env::var("CYCLEDIT_LOG_METRICS").as_deref() == Ok("1");
    if is_log_metrics {
        eprintln!("{METRICS}");
    }

    Ok(entries)
}

fn commit_date(seconds: i64) -> eyre::Result<Date> {
    let ts = jiff::Timestamp::from_second(seconds).wrap_err("timestamp")?;
    Ok(ts.to_zoned(jiff::tz::TimeZone::UTC).date())
}

/// Walk `parent_id`'s tree and collect the blob map filtered to `remaining` paths.
/// Also returns the parent tree's `ObjectId` so the caller can cache the result.
/// Returns `(None, empty map)` when `parent_id` is `None` (root commit).
fn walk_parent_blobs(
    repo: &gix::Repository,
    parent_id: Option<ObjectId>,
    remaining: &HashSet<String>,
) -> eyre::Result<(Option<ObjectId>, HashMap<String, ObjectId>)> {
    let Some(pid) = parent_id else {
        return Ok((None, HashMap::new()));
    };
    METRICS.inc_find_object();
    let parent_commit = repo
        .find_object(pid)
        .wrap_err("find parent commit")?
        .try_into_commit()
        .wrap_err("parent not a commit")?;
    let parent_tree_id = parent_commit.tree().wrap_err("parent tree")?.id;
    let mut v = RemainingFilteredCollector::new(remaining);
    walk_tree_blobs(repo, parent_tree_id, &mut v)?;
    Ok((Some(parent_tree_id), v.into_map()))
}

/// For each path in `current` whose blob differs from `parent`, record `date` in
/// `file_dates` and remove the path from `remaining`.
fn apply_diff(
    current: &HashMap<String, ObjectId>,
    parent: &HashMap<String, ObjectId>,
    date: Date,
    file_dates: &mut BTreeMap<String, (Date, ObjectId)>,
    remaining: &mut HashSet<String>,
) {
    for (path, blob_hash) in current {
        if parent.get(path) != Some(blob_hash) {
            file_dates.insert(path.clone(), (date, *blob_hash));
            remaining.remove(path);
        }
    }
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
        let path = format_prefix_and_name(prefix, name);
        self.0.insert(path, object_id);
        ControlFlow::Continue(())
    }
}

fn format_prefix_and_name(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}/{name}")
    }
}

struct IncludeExcludeCollector<'a> {
    pathspecs: &'a [String],
    excludes: &'a [String],
    result: HashMap<String, ObjectId>,
}
impl<'a> IncludeExcludeCollector<'a> {
    fn new(pathspecs: &'a [String], excludes: &'a [String]) -> Self {
        Self {
            pathspecs,
            excludes,
            result: HashMap::new(),
        }
    }
    fn into_map(self) -> HashMap<String, ObjectId> {
        let Self {
            pathspecs: _,
            excludes: _,
            result,
        } = self;
        result
    }
}
impl TreeVisitor for IncludeExcludeCollector<'_> {
    fn is_include_dir(&mut self, prefix: &str, name: &str) -> ControlFlow<(), bool> {
        // NOTE: `pathspecs` can match individual files (arbitrarily deep in the tree), so
        //       only consider "excludes" here
        let Self {
            pathspecs: _,
            excludes,
            result: _,
        } = self;

        if excludes.is_empty() {
            return ControlFlow::Continue(true);
        }

        let path = format_prefix_and_name(prefix, name);
        let matches_exclude = excludes
            .iter()
            .any(|spec| glob_match::glob_match(spec, &path));
        ControlFlow::Continue(!matches_exclude)
    }

    fn visit_blob(&mut self, prefix: &str, name: &str, object_id: ObjectId) -> ControlFlow<()> {
        let Self {
            pathspecs,
            excludes,
            result,
        } = self;

        let name_matches_exclude = excludes
            .iter()
            .any(|spec| glob_match::glob_match(spec, name));

        if !name_matches_exclude {
            let path = format_prefix_and_name(prefix, name);

            let matches_include = pathspecs.is_empty()
                || pathspecs.iter().any(|spec| {
                    let name_matches = glob_match::glob_match(spec, name);
                    let path_matches = glob_match::glob_match(spec, &path);
                    name_matches || path_matches
                });
            let matches_exclude = excludes
                .iter()
                .any(|spec| glob_match::glob_match(spec, &path));

            if matches_include && !matches_exclude {
                result.insert(path, object_id);
            }
        }

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
        let full = format_prefix_and_name(prefix, name);
        let is_include_dir = self.dir_prefixes.contains(&full);
        ControlFlow::Continue(is_include_dir)
    }

    fn visit_blob(&mut self, prefix: &str, name: &str, object_id: ObjectId) -> ControlFlow<()> {
        let path = format_prefix_and_name(prefix, name);
        // TODO: want to **remove** from `remaining`, so we can break if it's empty
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
        METRICS.inc_find_object();
        let tree = repo
            .find_object(tid)
            .wrap_err("find tree obj")?
            .try_into_tree()
            .wrap_err_with(|| format!("not a tree: {tid}"))?;

        for entry in tree.iter() {
            let entry = entry.wrap_err("tree entry")?;
            let name = entry.filename().to_str().wrap_err("non-utf8 filename")?;

            match entry.mode().kind() {
                gix::object::tree::EntryKind::Tree => {
                    let ControlFlow::Continue(include) = visitor.is_include_dir(&prefix, name)
                    else {
                        return Ok(());
                    };
                    if include {
                        METRICS.inc_visit_dir();

                        let child_prefix = format_prefix_and_name(&prefix, name);
                        stack.push((child_prefix, entry.object_id()));
                    }
                }
                gix::object::tree::EntryKind::Blob
                | gix::object::tree::EntryKind::BlobExecutable => {
                    METRICS.inc_visit_file();

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
