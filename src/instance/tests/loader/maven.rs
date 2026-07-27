use super::*;

#[test]
fn maven_3_part_coord() {
    assert_eq!(
        maven_coord_to_path("org.example:artifact:1.0"),
        Some("org/example/artifact/1.0/artifact-1.0.jar".to_string())
    );
}

#[test]
fn maven_4_part_coord_with_classifier() {
    assert_eq!(
        maven_coord_to_path("org.example:artifact:1.0:sources"),
        Some("org/example/artifact/1.0/artifact-1.0-sources.jar".to_string())
    );
}

#[test]
fn maven_nested_group() {
    assert_eq!(
        maven_coord_to_path("com.google.code.gson:gson:2.10"),
        Some("com/google/code/gson/gson/2.10/gson-2.10.jar".to_string())
    );
}

#[test]
fn maven_invalid_coordinates() {
    for coordinate in ["org.example:artifact", "a:b:c:d:e", "just-a-string", ""] {
        assert_eq!(maven_coord_to_path(coordinate), None);
    }
}
