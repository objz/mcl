// per-launch log files stored under .minecraft/logs/launches/
// each launch gets its own timestamped file so you can go back and see what
// crashed last tuesday at 3am

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LogFileEntry {
    pub name: String,
    pub path: PathBuf,
}

pub fn log_dir(instances_dir: &Path, instance_name: &str) -> PathBuf {
    instances_dir
        .join(instance_name)
        .join(".minecraft")
        .join("logs")
        .join("launches")
}

pub fn create_log_file(instances_dir: &Path, instance_name: &str) -> Option<PathBuf> {
    let dir = log_dir(instances_dir, instance_name);
    std::fs::create_dir_all(&dir).ok()?;
    let now = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    Some(dir.join(format!("{now}.log")))
}

pub fn scan_log_files(instances_dir: &Path, instance_name: &str) -> Vec<LogFileEntry> {
    let dir = log_dir(instances_dir, instance_name);

    let read_dir = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut entries: Vec<LogFileEntry> = read_dir
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?.to_string();
            if name.ends_with(".log") {
                Some(LogFileEntry { name, path })
            } else {
                None
            }
        })
        .collect();

    entries.sort_by(|a, b| b.name.cmp(&a.name));
    entries
}

pub fn read_log_file(path: &Path) -> Vec<String> {
    match std::fs::read_to_string(path) {
        Ok(content) => content.lines().map(|l| l.to_string()).collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_log_dir(tmp: &Path, instance: &str) -> PathBuf {
        let dir = log_dir(tmp, instance);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn log_dir_builds_correct_path() {
        let p = log_dir(Path::new("/instances"), "my-world");
        assert_eq!(
            p,
            PathBuf::from("/instances/my-world/.minecraft/logs/launches")
        );
    }

    #[test]
    fn scan_log_files_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        setup_log_dir(tmp.path(), "inst");
        let entries = scan_log_files(tmp.path(), "inst");
        assert!(entries.is_empty());
    }

    #[test]
    fn scan_log_files_finds_logs() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_log_dir(tmp.path(), "inst");
        std::fs::write(dir.join("2024-01-01_12-00-00.log"), "line1\nline2").unwrap();
        std::fs::write(dir.join("2024-01-02_12-00-00.log"), "line3").unwrap();
        let entries = scan_log_files(tmp.path(), "inst");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "2024-01-02_12-00-00.log");
        assert_eq!(entries[1].name, "2024-01-01_12-00-00.log");
    }

    #[test]
    fn scan_log_files_ignores_non_log() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_log_dir(tmp.path(), "inst");
        std::fs::write(dir.join("notes.txt"), "not a log").unwrap();
        std::fs::write(dir.join("real.log"), "log line").unwrap();
        let entries = scan_log_files(tmp.path(), "inst");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "real.log");
    }

    #[test]
    fn scan_log_files_missing_dir_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let entries = scan_log_files(tmp.path(), "ghost");
        assert!(entries.is_empty());
    }

    #[test]
    fn read_log_file_returns_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.log");
        std::fs::write(&path, "alpha\nbeta\ngamma").unwrap();
        let lines = read_log_file(&path);
        assert_eq!(lines, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn read_log_file_missing_returns_empty() {
        let lines = read_log_file(Path::new("/nonexistent/test.log"));
        assert!(lines.is_empty());
    }

    #[test]
    fn create_log_file_creates_path() {
        let tmp = tempfile::tempdir().unwrap();
        let path = create_log_file(tmp.path(), "inst");
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.to_string_lossy().ends_with(".log"));
        assert!(path.parent().unwrap().exists());
    }
}
