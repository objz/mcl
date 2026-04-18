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

trait AccountStoreLike {
    fn accounts(&self) -> &[Account];
    fn add_account(&mut self, account: Account);
}

impl AccountStoreLike for AccountStore {
    fn accounts(&self) -> &[Account] {
        &self.accounts
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

async fn add_microsoft_account() -> CliResult {
    if let Ok(mut slot) = crate::auth::DEVICE_CODE_DISPLAY.lock() {
        *slot = None;
    }

    let result_arc = crate::auth::start_microsoft_auth();

    loop {
        if let Ok(slot) = crate::auth::DEVICE_CODE_DISPLAY.lock()
            && let Some(info) = slot.as_ref() {
                println!("Open: {}", info.verification_uri);
                println!("Code: {}", info.user_code);
                break;
            }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    loop {
        if let Ok(slot) = result_arc.lock()
            && let Some(result) = slot.as_ref() {
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

    store.add_account(crate::auth::create_offline_account(username));
    Ok(())
}

fn delete_account(matches: &ArgMatches) -> CliResult {
    let username = required_arg(matches, "username")?;
    let mut store = AccountStore::load();
    let index = find_account_index(store.accounts(), username)
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
    let index = find_account_index(store.accounts(), username)
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
    use super::{add_offline_account, AccountStoreLike};
    use crate::auth::{Account, AccountType};

    #[derive(Default)]
    struct MockStore {
        accounts: Vec<Account>,
    }

    impl AccountStoreLike for MockStore {
        fn accounts(&self) -> &[Account] {
            &self.accounts
        }

        fn add_account(&mut self, account: Account) {
            self.accounts.push(account);
        }
    }

    #[test]
    fn creates_offline_account_through_store() {
        let mut store = MockStore::default();
        add_offline_account(&mut store, "Steve").expect("offline account should be added");

        assert_eq!(store.accounts.len(), 1);
        assert_eq!(store.accounts[0].username, "Steve");
        assert_eq!(store.accounts[0].account_type, AccountType::Offline);
    }

    #[test]
    fn rejects_empty_offline_username() {
        let mut store = MockStore::default();
        assert!(add_offline_account(&mut store, "   ").is_err());
    }
}
