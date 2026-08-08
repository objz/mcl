use super::resolve_log_path;

#[test]
fn resolves_latest_log_when_no_file_is_given() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("demo/minecraft/logs/launches");
    std::fs::create_dir_all(&dir).expect("log directory should exist");
    std::fs::write(dir.join("2024-01-02_03-04-05.log"), "newer").expect("write newer log");
    std::fs::write(dir.join("2024-01-01_03-04-05.log"), "older").expect("write older log");

    let path = resolve_log_path(tmp.path(), "demo", None).expect("latest log should resolve");
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("2024-01-02_03-04-05.log")
    );
}

#[test]
fn resolves_named_log_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("demo/minecraft/logs/launches");
    std::fs::create_dir_all(&dir).expect("log directory should exist");
    std::fs::write(dir.join("latest.log"), "hello").expect("write named log");

    let path =
        resolve_log_path(tmp.path(), "demo", Some("latest.log")).expect("named log should resolve");
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("latest.log")
    );
}
