use super::add_offline_account;
use crate::auth::{Account, AccountStore, AccountType};

fn microsoft_account() -> Account {
    Account {
        uuid: "00000000-0000-0000-0000-000000000001".to_owned(),
        username: "Owner".to_owned(),
        account_type: AccountType::Microsoft,
        active: false,
        refresh_token: Some("refresh".to_owned()),
        cached_mc_token: None,
        cached_mc_token_expires_at: None,
    }
}

#[test]
fn creates_offline_account_after_microsoft_account_exists() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = AccountStore::empty_for_test(temp.path().join("accounts.json"));
    store.add(microsoft_account());
    add_offline_account(&mut store, "Steve").expect("offline account should be added");

    assert_eq!(store.accounts.len(), 2);
    assert_eq!(store.accounts[1].username, "Steve");
    assert_eq!(store.accounts[1].account_type, AccountType::Offline);
}

#[test]
fn rejects_offline_account_before_microsoft_account_exists() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = AccountStore::empty_for_test(temp.path().join("accounts.json"));
    let err = add_offline_account(&mut store, "Steve")
        .expect_err("offline account should require a microsoft account");

    assert!(err.to_string().contains("Microsoft account"));
    assert!(store.accounts.is_empty());
}

#[test]
fn rejects_empty_offline_username() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = AccountStore::empty_for_test(temp.path().join("accounts.json"));
    assert!(add_offline_account(&mut store, "   ").is_err());
}
