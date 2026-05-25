//! verifies the lib/main split worked: integration tests can see crate items
//! through the public API.

#[test]
fn lib_target_is_importable() {
    // touch one pure function from each major module so the linker fails if
    // any module went private by mistake during the split.
    assert!(rmcl::net::maven_coord_to_path("a:b:1.0").is_some());
    let _ = rmcl::config::SETTINGS.paths.effective_java_path();
}
