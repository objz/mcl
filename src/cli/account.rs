// cli handlers for account management (microsoft oauth + offline accounts)
use std::io;
use std::time::Duration;

use clap::ArgMatches;

use crate::auth::{Account, AccountStore, AuthResult};
use crate::cli::output::{active_marker, print_table};

type CliResult = Result<(), Box<dyn std::error::Error>>;

pub async fn handle_account(matches: &ArgMatches) -> CliResult {
    match matches.subcommand() {
        Some(("list", _)) => list_accounts(),
        Some(("add", sub_matches)) => add_account(sub_matches).await,
        Some(("delete", sub_matches)) => delete_account(sub_matches),
        Some(("use", sub_matches)) => use_account(sub_matches),
        _ => Ok(()),
    }
}

// trait indirection so a mock store can be swapped in for tests
trait AccountStoreLike {
    fn has_microsoft_account(&self) -> bool;
    fn add_account(&mut self, account: Account);
}

impl AccountStoreLike for AccountStore {
    fn has_microsoft_account(&self) -> bool {
        AccountStore::has_microsoft_account(self)
    }

    fn add_account(&mut self, account: Account) {
        self.add(account);
    }
}

fn list_accounts() -> CliResult {
    let store = AccountStore::load();
    let rows = store
        .accounts
        .iter()
        .map(|account| {
            vec![
                active_marker(account.active).to_string(),
                account.username.clone(),
                format!("{:?}", account.account_type),
            ]
        })
        .collect::<Vec<_>>();

    print_table(&[" ", "Username", "Type"], &rows);
    Ok(())
}

async fn add_account(matches: &ArgMatches) -> CliResult {
    if matches.get_flag("microsoft") {
        return add_microsoft_account().await;
    }

    if let Some(username) = matches.get_one::<String>("offline") {
        let mut store = AccountStore::load();
        add_offline_account(&mut store, username)?;
        println!("Added offline account '{}'.", username);
    }

    Ok(())
}

// microsoft auth runs on a background thread via device code flow.
// polls two shared slots: one for the device code to display,
// then one for the final auth result. not the prettiest pattern
// but it keeps the oauth complexity out of the CLI layer.
async fn add_microsoft_account() -> CliResult {
    if let Ok(mut slot) = crate::auth::DEVICE_CODE_DISPLAY.lock() {
        *slot = None;
    }

    let result_arc = crate::auth::start_microsoft_auth();

    // wait for the device code to become available before showing it
    loop {
        if let Ok(slot) = crate::auth::DEVICE_CODE_DISPLAY.lock()
            && let Some(info) = slot.as_ref()
        {
            println!("Open: {}", info.verification_uri);
            println!("Code: {}", info.user_code);
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // now wait for the user to complete auth in their browser
    loop {
        if let Ok(slot) = result_arc.lock()
            && let Some(result) = slot.as_ref()
        {
            return match result {
                AuthResult::Success(account) => {
                    let mut store = AccountStore::load();
                    store.add(account.clone());
                    println!("Added Microsoft account '{}'.", account.username);
                    Ok(())
                }
                AuthResult::Error(message) => Err(io::Error::other(message.clone()).into()),
            };
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn add_offline_account<T: AccountStoreLike>(store: &mut T, username: &str) -> CliResult {
    let username = username.trim();
    if username.is_empty() {
        return Err(io::Error::other("offline username cannot be empty").into());
    }
    if !store.has_microsoft_account() {
        return Err(io::Error::other(
            "add a Microsoft account that owns Minecraft before adding offline accounts",
        )
        .into());
    }

    store.add_account(crate::auth::create_offline_account(username));
    Ok(())
}

fn delete_account(matches: &ArgMatches) -> CliResult {
    let username = required_arg(matches, "username")?;
    let mut store = AccountStore::load();
    let index = find_account_index(&store.accounts, username)
        .ok_or_else(|| io::Error::other(format!("account '{}' not found", username)))?;

    if !matches.get_flag("yes") && !confirm(&format!("Delete '{}'", username))? {
        println!("Cancelled.");
        return Ok(());
    }

    store.remove(index);
    println!("Deleted '{}'.", username);
    Ok(())
}

fn use_account(matches: &ArgMatches) -> CliResult {
    let username = required_arg(matches, "username")?;
    let mut store = AccountStore::load();
    let index = find_account_index(&store.accounts, username)
        .ok_or_else(|| io::Error::other(format!("account '{}' not found", username)))?;
    store.set_active(index);
    println!("Active account set to '{}'.", username);
    Ok(())
}

fn find_account_index(accounts: &[Account], username: &str) -> Option<usize> {
    accounts
        .iter()
        .position(|account| account.username.eq_ignore_ascii_case(username))
}

use super::utils::{confirm, required_arg};

#[cfg(test)]
mod tests {
    use super::{AccountStoreLike, add_offline_account};
    use crate::auth::{Account, AccountType};

    #[derive(Default)]
    struct MockStore {
        accounts: Vec<Account>,
    }

    impl AccountStoreLike for MockStore {
        fn has_microsoft_account(&self) -> bool {
            self.accounts
                .iter()
                .any(|account| account.account_type == AccountType::Microsoft)
        }

        fn add_account(&mut self, account: Account) {
            self.accounts.push(account);
        }
    }

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
        let mut store = MockStore::default();
        store.add_account(microsoft_account());
        add_offline_account(&mut store, "Steve").expect("offline account should be added");

        assert_eq!(store.accounts.len(), 2);
        assert_eq!(store.accounts[1].username, "Steve");
        assert_eq!(store.accounts[1].account_type, AccountType::Offline);
    }

    #[test]
    fn rejects_offline_account_before_microsoft_account_exists() {
        let mut store = MockStore::default();
        let err = add_offline_account(&mut store, "Steve")
            .expect_err("offline account should require a microsoft account");

        assert!(err.to_string().contains("Microsoft account"));
        assert!(store.accounts.is_empty());
    }

    #[test]
    fn rejects_empty_offline_username() {
        let mut store = MockStore::default();
        assert!(add_offline_account(&mut store, "   ").is_err());
    }
}
