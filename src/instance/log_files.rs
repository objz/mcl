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
