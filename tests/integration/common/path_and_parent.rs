/// Handle on an input path split into `"{parent_path}/{name}"`
/// or if `parent_path` is empty, just `name`
#[derive(Clone, Copy, Debug)]
pub struct PathAndParent<'a> {
    path: &'a str,
    parent_path: &'a str,
    name: &'a str,
}
impl<'a> PathAndParent<'a> {
    /// Parses the input path into `"{parent_path}/{name}"` or returns `None` if `name` is empty
    pub fn new(path: &'a str) -> Option<Self> {
        let (parent_path, name) = path.rsplit_once('/').unwrap_or(("", path));
        if name.is_empty() {
            return None;
        }
        Some(Self {
            path,
            parent_path,
            name,
        })
    }
    /// Returns the original input path
    pub fn get_path(self) -> &'a str {
        self.path
    }
    /// Returns the parent path (may be empty)
    pub fn get_parent_path(self) -> &'a str {
        self.parent_path
    }
    /// Returns the name (final element of the path) which is nonempty by construction
    pub fn get_name(self) -> &'a str {
        self.name
    }
}
