// java runtime discovery shared by launching, loader installation, and settings.

#[must_use]
pub fn detect_java_path() -> String {
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        let java_name = if cfg!(windows) { "java.exe" } else { "java" };
        let bin = std::path::Path::new(&java_home).join("bin").join(java_name);
        if bin.exists() {
            tracing::trace!("Detected Java from JAVA_HOME: {}", bin.display());
            return bin.to_string_lossy().to_string();
        }
        tracing::warn!(
            "JAVA_HOME is set to {}, but {} does not exist",
            java_home,
            bin.display()
        );
    }
    match which::which("java") {
        Ok(path) => {
            tracing::trace!("Detected Java from PATH: {}", path.display());
            path.to_string_lossy().to_string()
        }
        Err(e) => {
            tracing::warn!(
                "Could not find java on PATH, falling back to literal 'java': {}",
                e
            );
            "java".to_string()
        }
    }
}
