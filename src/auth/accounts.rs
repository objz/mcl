// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_mc_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_mc_token_expires_at: Option<i64>,
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
            Ok(content) => match serde_json::from_str(&content) {
                Ok(accounts) => accounts,
                Err(e) => {
                    tracing::warn!("Failed to parse accounts file {}: {}", path.display(), e);
                    Vec::new()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::debug!("No accounts file at {}", path.display());
                Vec::new()
            }
            Err(e) => {
                tracing::warn!("Failed to read accounts file {}: {}", path.display(), e);
                Vec::new()
            }
        };
        tracing::debug!(
            "Loaded {} account(s) from {}",
            accounts.len(),
            path.display()
        );
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
                // this file holds microsoft refresh tokens: a torn write on
                // crash would permanently lock the user out, so write atomically.
                if let Err(e) = crate::storage::write_atomic(&self.path, json.as_bytes()) {
                    tracing::error!(
                        "Failed to write accounts file {}: {}",
                        self.path.display(),
                        e
                    );
                } else {
                    tracing::debug!(
                        "Saved {} account(s) to {}",
                        self.accounts.len(),
                        self.path.display()
                    );
                }
            }
            Err(e) => tracing::error!("Failed to serialize accounts: {}", e),
        }
    }

    pub fn active_account(&self) -> Option<&Account> {
        self.accounts.iter().find(|a| a.active)
    }

    pub fn has_microsoft_account(&self) -> bool {
        self.accounts
            .iter()
            .any(|account| account.account_type == AccountType::Microsoft)
    }

    pub fn set_active(&mut self, index: usize) {
        let Some(account) = self.accounts.get(index) else {
            // out-of-range: leave the current selection untouched. marking
            // every account inactive here would break the single-active
            // invariant and orphan the store with no usable account.
            tracing::warn!("Tried to select missing account index {}", index);
            return;
        };
        let username = account.username.clone();
        for (i, acc) in self.accounts.iter_mut().enumerate() {
            acc.active = i == index;
        }
        tracing::info!("Selected account '{}'", username);
        self.save();
    }

    // if an account with the same uuid already exists, replace it.
    // first account added auto-becomes active so there's always a selection.
    pub fn add(&mut self, account: Account) {
        let uuid = &account.uuid;
        let replaced = self.accounts.iter().any(|a| a.uuid == *uuid);
        // re-adding the currently active account must not drop the selection:
        // the replacement takes over the old account's active flag.
        let replaced_active = self
            .accounts
            .iter()
            .any(|a| a.uuid == *uuid && a.active);
        let account_type = account.account_type.clone();
        let username = account.username.clone();
        self.accounts.retain(|a| a.uuid != *uuid);
        let mut account = account;
        if self.accounts.is_empty() || replaced_active {
            account.active = true;
        }
        self.accounts.push(account);
        tracing::info!(
            "{} {:?} account '{}'",
            if replaced { "Updated" } else { "Added" },
            account_type,
            username
        );
        self.save();
    }

    pub fn remove(&mut self, index: usize) {
        if index >= self.accounts.len() {
            tracing::warn!("Tried to remove missing account index {}", index);
            return;
        }
        let account = self.accounts.remove(index);
        tracing::info!(
            "Removed {:?} account '{}'",
            account.account_type,
            account.username
        );
        if account.active && !self.accounts.is_empty() {
            self.accounts[0].active = true;
            tracing::debug!("Activated fallback account '{}'", self.accounts[0].username);
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
        cached_mc_token: None,
        cached_mc_token_expires_at: None,
    }
}

#[cfg(test)]
#[path = "tests/accounts.rs"]
mod tests;
