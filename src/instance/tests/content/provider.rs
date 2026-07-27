use super::*;

#[test]
fn registry_falls_back_to_first_capable_provider() {
    let registry = ProviderRegistry::modrinth(crate::net::HttpClient::new());
    assert_eq!(registry.preferred("unknown").unwrap().id(), "modrinth");
}
