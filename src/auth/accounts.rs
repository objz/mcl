// account management: persistence, switching active accounts, and offline uuid generation

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub uuid: String,
    pub username: String,
    pub account_type: AccountType,
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccountType {
    Microsoft,
    Offline,
}

#[derive(Debug)]
pub enum AuthResult {
    Success(Account),
    Error(String),
}

pub struct AccountStore {
    pub accounts: Vec<Account>,
    path: PathBuf,
}

impl AccountStore {
    pub fn load() -> Self {
        let path = account_store_path();
        let accounts = match std::fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        Self { accounts, path }
    }

    pub fn save(&self) {
        if let Some(parent) = self.path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::error!("Failed to create accounts directory: {}", e);
            return;
        }
        match serde_json::to_string_pretty(&self.accounts) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&self.path, json) {
                    tracing::error!("Failed to write accounts file: {}", e);
                }
            }
            Err(e) => tracing::error!("Failed to serialize accounts: {}", e),
        }
    }

    pub fn active_account(&self) -> Option<&Account> {
        self.accounts.iter().find(|a| a.active)
    }

    pub fn set_active(&mut self, index: usize) {
        for (i, acc) in self.accounts.iter_mut().enumerate() {
            acc.active = i == index;
        }
        self.save();
    }

    // if an account with the same uuid already exists, replace it.
    // first account added auto-becomes active so there's always a selection.
    pub fn add(&mut self, account: Account) {
        let uuid = &account.uuid;
        self.accounts.retain(|a| a.uuid != *uuid);
        let mut account = account;
        if self.accounts.is_empty() {
            account.active = true;
        }
        self.accounts.push(account);
        self.save();
    }

    pub fn remove(&mut self, index: usize) {
        if index >= self.accounts.len() {
            return;
        }
        let account = self.accounts.remove(index);
        if account.active && !self.accounts.is_empty() {
            self.accounts[0].active = true;
        }
        self.save();
    }
}

pub fn account_store_path() -> PathBuf {
    crate::config::get_config_path().join("accounts.json")
}

// deterministic fake uuid from a username, formatted as uuid v3 with the proper
// version and variant bits set. not cryptographically meaningful, just needs to
// be consistent so the same offline name always maps to the same uuid.
pub fn offline_uuid(username: &str) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    format!("OfflinePlayer:{username}").hash(&mut hasher);
    let h = hasher.finish();
    format!(
        "{:08x}-{:04x}-3{:03x}-{:04x}-{:012x}",
        (h >> 32) as u32,
        (h >> 16) as u16,
        (h >> 4) as u16 & 0x0FFF,
        (h as u16 & 0x3FFF) | 0x8000,
        h & 0xFFFFFFFFFFFF,
    )
}

pub fn create_offline_account(username: &str) -> Account {
    Account {
        uuid: offline_uuid(username),
        username: username.to_owned(),
        account_type: AccountType::Offline,
        active: false,
        refresh_token: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_is_valid_format() {
        let uuid = offline_uuid("Steve");
        let parts: Vec<&str> = uuid.split('-').collect();
        assert_eq!(parts.len(), 5, "UUID must have 5 dash-separated parts");
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[1].len(), 4);
        assert_eq!(parts[2].len(), 4);
        assert_eq!(parts[3].len(), 4);
        assert_eq!(parts[4].len(), 12);
    }

    #[test]
    fn offline_uuid_version_3_marker() {
        let uuid = offline_uuid("Steve");
        assert!(uuid.split('-').nth(2).unwrap().starts_with('3'));
    }

    #[test]
    fn offline_uuid_variant_bit_set() {
        let uuid = offline_uuid("Steve");
        let part3 = uuid.split('-').nth(3).unwrap();
        let first_nibble = u8::from_str_radix(&part3[..1], 16).unwrap();
        assert!((0x8..=0xb).contains(&first_nibble));
    }

    #[test]
    fn offline_uuid_deterministic() {
        assert_eq!(offline_uuid("Steve"), offline_uuid("Steve"));
        assert_eq!(offline_uuid("Alex"), offline_uuid("Alex"));
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
        assert!(!acc.uuid.is_empty());
    }

    fn make_store(dir: &std::path::Path) -> AccountStore {
        AccountStore {
            accounts: Vec::new(),
            path: dir.join("accounts.json"),
        }
    }

    fn dummy_account(name: &str) -> Account {
        Account {
            uuid: offline_uuid(name),
            username: name.to_owned(),
            account_type: AccountType::Offline,
            active: false,
            refresh_token: None,
        }
    }

    #[test]
    fn store_add_first_becomes_active() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = make_store(tmp.path());
        store.add(dummy_account("Alice"));
        assert_eq!(store.accounts.len(), 1);
        assert!(store.accounts[0].active);
    }

    #[test]
    fn store_add_second_stays_inactive() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = make_store(tmp.path());
        store.add(dummy_account("Alice"));
        store.add(dummy_account("Bob"));
        assert_eq!(store.accounts.len(), 2);
        assert!(store.accounts[0].active);
        assert!(!store.accounts[1].active);
    }

    #[test]
    fn store_add_duplicate_uuid_replaces() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = make_store(tmp.path());
        store.add(dummy_account("Alice"));
        let mut dup = dummy_account("Alice");
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
    fn store_active_account_returns_active() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = make_store(tmp.path());
        store.add(dummy_account("Alice"));
        store.add(dummy_account("Bob"));
        let active = store.active_account().unwrap();
        assert_eq!(active.username, "Alice");
    }

    #[test]
    fn store_set_active_changes_active() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = make_store(tmp.path());
        store.add(dummy_account("Alice"));
        store.add(dummy_account("Bob"));
        store.set_active(1);
        assert!(!store.accounts[0].active);
        assert!(store.accounts[1].active);
    }

    #[test]
    fn store_remove_activates_first_remaining() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = make_store(tmp.path());
        store.add(dummy_account("Alice"));
        store.add(dummy_account("Bob"));
        store.remove(0);
        assert_eq!(store.accounts.len(), 1);
        assert_eq!(store.accounts[0].username, "Bob");
        assert!(store.accounts[0].active);
    }

    #[test]
    fn store_remove_out_of_bounds_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = make_store(tmp.path());
        store.add(dummy_account("Alice"));
        store.remove(5);
        assert_eq!(store.accounts.len(), 1);
    }

    #[test]
    fn store_save_and_reload() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = make_store(tmp.path());
        store.add(dummy_account("Alice"));
        store.add(dummy_account("Bob"));
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
}
