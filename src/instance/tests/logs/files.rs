use super::*;

#[test]
fn log_dir_builds_correct_path() {
    let p = log_dir(Path::new("/instances"), "my-world");
    assert_eq!(
        p,
        PathBuf::from("/instances/my-world/minecraft/logs/launches")
    );
}
