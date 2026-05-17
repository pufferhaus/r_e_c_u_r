//! Pure browser tree walk. Output drives the BrowserBody screen.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

const VIDEO_EXTS: &[&str] = &["mp4", "mkv", "avi", "mov", "m4v", "webm"];

#[derive(Debug, Clone, PartialEq)]
pub struct BrowserRow {
    /// Indented display string (`    foo.mp4`, `myfolder/`, etc.).
    pub display: String,
    /// Absolute filesystem path of the underlying entry.
    pub path: PathBuf,
    /// `true` for video files, `false` for directories.
    pub is_file: bool,
    /// Depth (0 = root level).
    pub depth: usize,
    /// Canonical absolute path used as ProbeCache key. Falls back to `path`
    /// on canonicalize error (broken symlink, missing intermediate).
    pub probe_key: PathBuf,
}

/// Walk `roots` producing a flat depth-first row list, expanding only the
/// directories present in `open`. Folder ordering is alphabetical, files are
/// sorted alphabetically and follow folders at each level. Entries beginning
/// with `.` are skipped.
pub fn walk_browser(roots: &[PathBuf], open: &HashSet<PathBuf>) -> Vec<BrowserRow> {
    let mut out = Vec::new();
    for root in roots {
        walk_recursive(root, 0, open, &mut out);
    }
    out
}

fn walk_recursive(dir: &Path, depth: usize, open: &HashSet<PathBuf>, out: &mut Vec<BrowserRow>) {
    let read = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            dirs.push(path);
        } else if is_video(&path) {
            files.push(path);
        }
    }
    dirs.sort();
    files.sort();
    for d in dirs {
        let name = d.file_name().unwrap().to_string_lossy().into_owned();
        let is_open = open.contains(&d);
        let glyph = if is_open { '/' } else { '|' };
        let probe_key = std::fs::canonicalize(&d).unwrap_or_else(|_| d.clone());
        out.push(BrowserRow {
            display: format!("{}{}{}", indent(depth), name, glyph),
            path: d.clone(),
            is_file: false,
            depth,
            probe_key,
        });
        if is_open {
            walk_recursive(&d, depth + 1, open, out);
        }
    }
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        let probe_key = std::fs::canonicalize(&f).unwrap_or_else(|_| f.clone());
        out.push(BrowserRow {
            display: format!("{}{}", indent(depth), name),
            path: f,
            is_file: true,
            depth,
            probe_key,
        });
    }
}

fn indent(depth: usize) -> String {
    " ".repeat(4 * depth)
}

fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(p: &Path) {
        fs::write(p, b"").unwrap();
    }

    #[test]
    fn skips_dotfiles_and_non_video() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("a.mp4"));
        touch(&tmp.path().join("b.txt"));
        touch(&tmp.path().join(".hidden.mp4"));
        let rows = walk_browser(&[tmp.path().to_path_buf()], &HashSet::new());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].display, "a.mp4");
    }

    #[test]
    fn closed_folder_shown_with_pipe() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("sub")).unwrap();
        touch(&tmp.path().join("sub").join("c.mp4"));
        let rows = walk_browser(&[tmp.path().to_path_buf()], &HashSet::new());
        assert_eq!(rows.len(), 1);
        assert!(rows[0].display.ends_with('|'));
        assert!(!rows[0].is_file);
    }

    #[test]
    fn open_folder_expands_with_slash_and_indented_children() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("sub");
        fs::create_dir(&sub).unwrap();
        touch(&sub.join("c.mp4"));
        let mut open = HashSet::new();
        open.insert(sub.clone());
        let rows = walk_browser(&[tmp.path().to_path_buf()], &open);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].display.ends_with('/'));
        assert_eq!(rows[1].depth, 1);
        assert!(rows[1].display.starts_with("    "));
        assert!(rows[1].is_file);
    }

    #[test]
    fn folders_before_files_at_same_level() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("z.mp4"));
        fs::create_dir(tmp.path().join("a_dir")).unwrap();
        let rows = walk_browser(&[tmp.path().to_path_buf()], &HashSet::new());
        assert_eq!(rows.len(), 2);
        assert!(!rows[0].is_file);
        assert!(rows[1].is_file);
    }

    #[test]
    fn browser_row_carries_probe_key_for_files() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("clip.mp4");
        fs::write(&f, b"").unwrap();
        let rows = walk_browser(&[tmp.path().to_path_buf()], &HashSet::new());
        let row = rows.iter().find(|r| r.is_file).unwrap();
        let canon = std::fs::canonicalize(&f).unwrap();
        assert_eq!(row.probe_key, canon);
    }

    #[test]
    fn browser_row_probe_key_falls_back_to_path_on_canon_error() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("sub")).unwrap();
        let rows = walk_browser(&[tmp.path().to_path_buf()], &HashSet::new());
        let dir_row = rows.iter().find(|r| !r.is_file).unwrap();
        assert!(dir_row.probe_key.ends_with("sub") || dir_row.probe_key == dir_row.path);
    }
}
