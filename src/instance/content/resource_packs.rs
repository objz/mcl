// resource pack scanner backed by the shared pack.mcmeta reader.

use std::path::Path;

use super::ContentEntry;

pub fn scan_one_resource_pack(path: &Path, file_stem: &str, enabled: bool) -> ContentEntry {
    super::packs::scan_one_pack(path, file_stem, enabled)
}

pub fn scan_resource_packs(instances_dir: &Path, instance_name: &str) -> Vec<ContentEntry> {
    super::packs::scan_packs(instances_dir, instance_name, "resourcepacks")
}

#[cfg(test)]
mod tests {
    use super::super::packs::extract_description;
    use serde_json::json;

    // every case exercises a distinct match arm in extract_description.
    #[rstest::rstest]
    #[case::string(json!("Simple pack"), "Simple pack")]
    #[case::object_with_text(json!({"text": "Hello world"}), "Hello world")]
    #[case::object_without_text(json!({"color": "red"}), "")]
    #[case::array_of_strings(json!(["Hello", " ", "world"]), "Hello world")]
    #[case::array_of_objects(json!([{"text": "A"}, {"text": "B"}]), "AB")]
    #[case::mixed_array(json!(["Prefix ", {"text": "suffix"}]), "Prefix suffix")]
    #[case::empty_array(json!([]), "")]
    #[case::null(serde_json::Value::Null, "")]
    #[case::number(json!(42), "")]
    #[case::bool(json!(true), "")]
    fn extract_description_handles(#[case] input: serde_json::Value, #[case] expected: &str) {
        assert_eq!(extract_description(&input), expected);
    }
}
