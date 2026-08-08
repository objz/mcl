use super::*;

impl AccountStore {
    pub(crate) fn empty_for_test(path: PathBuf) -> Self {
        Self {
            accounts: Vec::new(),
            path,
        }
    }
}

#[test]
fn offline_uuid_has_valid_v3_shape() {
    let uuid = offline_uuid("Steve");
    let parts: Vec<&str> = uuid.split('-').collect();
    assert_eq!(parts.len(), 5, "UUID must have 5 dash-separated parts");
    assert_eq!(parts[0].len(), 8);
    assert_eq!(parts[1].len(), 4);
    assert_eq!(parts[2].len(), 4);
    assert_eq!(parts[3].len(), 4);
    assert_eq!(parts[4].len(), 12);
    assert!(parts[2].starts_with('3'));
    let first_nibble = u8::from_str_radix(&parts[3][..1], 16).unwrap();
    assert!((0x8..=0xb).contains(&first_nibble));
}

#[test]
fn offline_uuid_is_pinned_for_known_names() {
    assert_eq!(
        offline_uuid("Steve"),
        "7e0a3689-ed7c-347c-87cc-3689ed7c47cc"
    );
    assert_eq!(offline_uuid("Alex"), "a13d42a3-454f-3e39-a392-42a3454fe392");
}

#[test]
fn offline_uuid_different_for_different_names() {
    assert_ne!(offline_uuid("Steve"), offline_uuid("Alex"));
}

#[test]
fn create_offline_account_fields() {
    let acc = create_offline_account("TestPlayer");
    assert_eq!(acc.username, "TestPlayer");
    assert_eq!(acc.account_type, AccountType::Offline);
    assert!(!acc.active);
    assert!(acc.refresh_token.is_none());
    // pin the uuid to the deterministic offline_uuid output so a regression
    // in the uuid derivation (e.g. salt change) would fail this test, not
    // just a non-empty-string check that any garbage would pass.
    assert_eq!(acc.uuid, offline_uuid("TestPlayer"));
}

fn make_store(dir: &std::path::Path) -> AccountStore {
    AccountStore {
        accounts: Vec::new(),
        path: dir.join("accounts.json"),
    }
}

fn microsoft_account(name: &str) -> Account {
    Account {
        uuid: format!("00000000-0000-0000-0000-{:012}", name.len()),
        username: name.to_owned(),
        account_type: AccountType::Microsoft,
        active: false,
        refresh_token: Some("refresh".to_owned()),
        cached_mc_token: None,
        cached_mc_token_expires_at: None,
    }
}

#[test]
fn store_add_first_becomes_active() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = make_store(tmp.path());
    store.add(create_offline_account("Alice"));
    assert_eq!(store.accounts.len(), 1);
    assert!(store.accounts[0].active);
}

#[test]
fn store_add_second_stays_inactive() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = make_store(tmp.path());
    store.add(create_offline_account("Alice"));
    store.add(create_offline_account("Bob"));
    assert_eq!(store.accounts.len(), 2);
    assert!(store.accounts[0].active);
    assert!(!store.accounts[1].active);
}

#[test]
fn store_add_duplicate_uuid_replaces() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = make_store(tmp.path());
    store.add(create_offline_account("Alice"));
    let mut dup = create_offline_account("Alice");
    dup.username = "AliceRenamed".to_owned();
    dup.uuid = store.accounts[0].uuid.clone();
    store.add(dup);
    assert_eq!(store.accounts.len(), 1);
    assert_eq!(store.accounts[0].username, "AliceRenamed");
}

#[test]
fn store_active_account_none_when_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let store = make_store(tmp.path());
    assert!(store.active_account().is_none());
}

#[test]
fn store_has_microsoft_account_when_one_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = make_store(tmp.path());
    store.add(create_offline_account("Offline"));
    assert!(!store.has_microsoft_account());

    store.add(microsoft_account("Owner"));
    assert!(store.has_microsoft_account());
}

#[test]
fn store_active_account_returns_active() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = make_store(tmp.path());
    store.add(create_offline_account("Alice"));
    store.add(create_offline_account("Bob"));
    let active = store.active_account().unwrap();
    assert_eq!(active.username, "Alice");
}

#[test]
fn store_set_active_changes_active() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = make_store(tmp.path());
    store.add(create_offline_account("Alice"));
    store.add(create_offline_account("Bob"));
    store.set_active(1);
    assert!(!store.accounts[0].active);
    assert!(store.accounts[1].active);
}

#[test]
fn store_remove_activates_first_remaining() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = make_store(tmp.path());
    store.add(create_offline_account("Alice"));
    store.add(create_offline_account("Bob"));
    store.remove(0);
    assert_eq!(store.accounts.len(), 1);
    assert_eq!(store.accounts[0].username, "Bob");
    assert!(store.accounts[0].active);
}

#[test]
fn store_remove_out_of_bounds_noop() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = make_store(tmp.path());
    store.add(create_offline_account("Alice"));
    store.remove(5);
    assert_eq!(store.accounts.len(), 1);
}

#[test]
fn store_save_and_reload() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = make_store(tmp.path());
    store.add(create_offline_account("Alice"));
    store.add(create_offline_account("Bob"));
    store.save();

    let reloaded = AccountStore {
        accounts: serde_json::from_str(
            &std::fs::read_to_string(tmp.path().join("accounts.json")).unwrap(),
        )
        .unwrap(),
        path: tmp.path().join("accounts.json"),
    };
    assert_eq!(reloaded.accounts.len(), 2);
    assert_eq!(reloaded.accounts[0].username, "Alice");
    assert!(reloaded.accounts[0].active);
}
