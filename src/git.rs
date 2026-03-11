use std::path::PathBuf;

pub struct FileEntry {
    pub date: jiff::civil::Date,
    pub blob_hash: gix::ObjectId,
    pub path: PathBuf,
}
