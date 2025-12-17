use anyhow::Context;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// In-memory file system that can be committed to disk
#[derive(Debug, Default)]
pub struct MemFs {
    /// Base path for all files
    base_path: PathBuf,
    /// Files to write (path relative to base_path -> content)
    files: BTreeMap<PathBuf, FileEntry>,
    /// Directories to create (path relative to base_path)
    dirs_to_clear: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
enum FileEntry {
    /// File with content to write
    Content(String),
    /// File to delete (if it exists)
    Delete,
}

impl MemFs {
    /// Create a new MemFs with a base path
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
            files: BTreeMap::new(),
            dirs_to_clear: Vec::new(),
        }
    }

    /// Write a file to the in-memory file system
    pub fn write(&mut self, path: impl Into<PathBuf>, content: impl Into<String>) {
        let path = path.into();
        self.files.insert(path, FileEntry::Content(content.into()));
    }

    /// Mark a file for deletion
    pub fn delete(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        self.files.insert(path, FileEntry::Delete);
    }

    /// Remove a file from MemFs (undo a write or delete)
    pub fn remove(&mut self, path: impl AsRef<Path>) {
        self.files.remove(path.as_ref());
    }

    /// Check if a file exists in MemFs
    pub fn contains(&self, path: impl AsRef<Path>) -> bool {
        self.files.contains_key(path.as_ref())
    }

    /// Get a file's content from MemFs (if it's a write, not a delete)
    pub fn get(&self, path: impl AsRef<Path>) -> Option<&str> {
        match self.files.get(path.as_ref()) {
            Some(FileEntry::Content(content)) => Some(content),
            _ => None,
        }
    }

    /// Mark a directory to be cleared before writing
    pub fn clear_dir(&mut self, path: impl Into<PathBuf>) {
        self.dirs_to_clear.push(path.into());
    }

    /// Get list of all files (for display purposes)
    pub fn list_files(&self) -> Vec<(&Path, bool)> {
        self.files
            .iter()
            .map(|(path, entry)| {
                let is_write = matches!(entry, FileEntry::Content(_));
                (path.as_path(), is_write)
            })
            .collect()
    }

    /// Get count of files to write
    pub fn file_count(&self) -> usize {
        self.files
            .values()
            .filter(|e| matches!(e, FileEntry::Content(_)))
            .count()
    }

    /// Commit all files to disk
    pub fn commit(&self) -> anyhow::Result<CommitStats> {
        let mut stats = CommitStats::default();

        // Clear directories first
        for dir in &self.dirs_to_clear {
            let full_path = self.base_path.join(dir);
            if full_path.exists() {
                fs::remove_dir_all(&full_path)
                    .with_context(|| format!("Failed to clear directory: {}", full_path.display()))?;
                stats.dirs_cleared += 1;
            }
        }

        // Process all files
        for (path, entry) in &self.files {
            let full_path = self.base_path.join(path);

            match entry {
                FileEntry::Content(content) => {
                    // Create parent directories if needed
                    if let Some(parent) = full_path.parent() {
                        if !parent.exists() {
                            fs::create_dir_all(parent)
                                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
                            stats.dirs_created += 1;
                        }
                    }

                    // Write the file
                    fs::write(&full_path, content)
                        .with_context(|| format!("Failed to write file: {}", full_path.display()))?;
                    stats.files_written += 1;
                }
                FileEntry::Delete => {
                    if full_path.exists() {
                        fs::remove_file(&full_path)
                            .with_context(|| format!("Failed to delete file: {}", full_path.display()))?;
                        stats.files_deleted += 1;
                    }
                }
            }
        }

        Ok(stats)
    }

    /// Clear all files from MemFs (reset)
    pub fn clear(&mut self) {
        self.files.clear();
        self.dirs_to_clear.clear();
    }
}

/// Statistics from a commit operation
#[derive(Debug, Default)]
pub struct CommitStats {
    pub files_written: usize,
    pub files_deleted: usize,
    pub dirs_created: usize,
    pub dirs_cleared: usize,
}

impl std::fmt::Display for CommitStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} files written, {} deleted, {} dirs created, {} cleared",
            self.files_written, self.files_deleted, self.dirs_created, self.dirs_cleared
        )
    }
}
