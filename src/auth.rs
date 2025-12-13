use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use minecraft_msa_auth::MinecraftAuthorizationFlow;
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, ClientId, DeviceAuthorizationUrl, RefreshToken, Scope,
    StandardDeviceAuthorizationResponse, TokenResponse, TokenUrl,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

const CLIENT_ID: &str = "708e91b5-99f8-4a1d-80ec-e746cbb24771";
const DEVICE_CODE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const MSA_AUTHORIZE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
const MSA_TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const KEYRING_SERVICE: &str = "mcl-launcher";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub uuid: String,
    pub username: String,
    pub account_type: AccountType,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccountType {
    Microsoft,
    Offline,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceCodeInfo {
    pub user_code: String,
    pub verification_uri: String,
}

#[derive(Debug)]
pub enum AuthResult {
    Success(Account),
    Error(String),
}

#[derive(Deserialize)]
struct McProfile {
    id: String,
    name: String,
}

// ── Account Store ──────────────────────────────────────────────

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
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.accounts) {
            let _ = std::fs::write(&self.path, json);
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

    pub fn add(&mut self, account: Account) {
        let is_first = self.accounts.is_empty();
        let uuid = account.uuid.clone();
        self.accounts.retain(|a| a.uuid != uuid);
        let mut account = account;
        if is_first {
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
        if account.account_type == AccountType::Microsoft {
            let _ = delete_refresh_token(&account.uuid);
        }
        if account.active && !self.accounts.is_empty() {
            self.accounts[0].active = true;
        }
        self.save();
    }
}

fn account_store_path() -> PathBuf {
    dirs_next::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mcl")
        .join("accounts.json")
}

// ── Keyring ────────────────────────────────────────────────────

fn store_refresh_token(uuid: &str, token: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &format!("refresh:{uuid}"))
        .map_err(|e| e.to_string())?;
    entry.set_password(token).map_err(|e| e.to_string())
}

fn get_refresh_token(uuid: &str) -> Result<String, String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &format!("refresh:{uuid}"))
        .map_err(|e| e.to_string())?;
    entry.get_password().map_err(|e| e.to_string())
}

fn delete_refresh_token(uuid: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &format!("refresh:{uuid}"))
        .map_err(|e| e.to_string())?;
    entry.delete_credential().map_err(|e| e.to_string())
}

// ── Offline ────────────────────────────────────────────────────

pub fn offline_uuid(username: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    format!("OfflinePlayer:{username}").hash(&mut hasher);
    let h = hasher.finish();
    format!(
        "{:08x}-{:04x}-3{:03x}-{:04x}-{:012x}",
        (h >> 32) as u32,
        (h >> 16) as u16 & 0xFFFF,
        (h >> 4) as u16 & 0x0FFF,
        (h as u16 & 0x3FFF) | 0x8000,
        h & 0xFFFFFFFFFFFF,
    )
}

pub fn create_offline_account(username: &str) -> Account {
    Account {
        uuid: offline_uuid(username),
        username: username.to_string(),
        account_type: AccountType::Offline,
        active: false,
    }
}

// ── Microsoft Auth (oauth2 + minecraft-msa-auth) ───────────────

pub static DEVICE_CODE_DISPLAY: Lazy<Arc<Mutex<Option<DeviceCodeInfo>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));

async fn run_full_oauth_flow() -> Result<(String, Option<String>), String> {
    let oauth_client = BasicClient::new(ClientId::new(CLIENT_ID.to_string()))
        .set_auth_uri(AuthUrl::new(MSA_AUTHORIZE_URL.to_string()).map_err(|e| e.to_string())?)
        .set_token_uri(TokenUrl::new(MSA_TOKEN_URL.to_string()).map_err(|e| e.to_string())?)
        .set_device_authorization_url(
            DeviceAuthorizationUrl::new(DEVICE_CODE_URL.to_string()).map_err(|e| e.to_string())?,
        );

    let http_client = reqwest::Client::new();

    let details: StandardDeviceAuthorizationResponse = oauth_client
        .exchange_device_code()
        .add_scope(Scope::new("XboxLive.signin".to_string()))
        .add_scope(Scope::new("offline_access".to_string()))
        .request_async(&http_client)
        .await
        .map_err(|e| format!("Device code request failed: {e}"))?;

    if let Ok(mut slot) = DEVICE_CODE_DISPLAY.lock() {
        *slot = Some(DeviceCodeInfo {
            user_code: details.user_code().secret().to_string(),
            verification_uri: details.verification_uri().to_string(),
        });
    }

    let token = oauth_client
        .exchange_device_access_token(&details)
        .request_async(&http_client, tokio::time::sleep, None)
        .await
        .map_err(|e| format!("Authentication failed: {e}"))?;

    let ms_access_token = token.access_token().secret().to_string();
    let ms_refresh_token = token.refresh_token().map(|r| r.secret().to_string());

    Ok((ms_access_token, ms_refresh_token))
}

pub fn start_microsoft_auth() -> Arc<Mutex<Option<AuthResult>>> {
    let result: Arc<Mutex<Option<AuthResult>>> = Arc::new(Mutex::new(None));
    let result_clone = result.clone();

    tokio::spawn(async move {
        let outcome = run_full_auth_flow().await;
        if let Ok(mut slot) = result_clone.lock() {
            *slot = Some(outcome);
        }
    });

    result
}

async fn run_full_auth_flow() -> AuthResult {
    let (ms_access_token, ms_refresh_token) = match run_full_oauth_flow().await {
        Ok(pair) => pair,
        Err(e) => return AuthResult::Error(e),
    };

    exchange_and_build_account(&ms_access_token, ms_refresh_token.as_deref()).await
}

async fn exchange_and_build_account(
    ms_access_token: &str,
    ms_refresh_token: Option<&str>,
) -> AuthResult {
    let mc_flow = MinecraftAuthorizationFlow::new(reqwest::Client::new());
    let mc_token = match mc_flow.exchange_microsoft_token(ms_access_token).await {
        Ok(t) => t,
        Err(e) => return AuthResult::Error(format!("Minecraft auth failed: {e}")),
    };

    let client = reqwest::Client::new();
    let profile_resp = match client
        .get(MC_PROFILE_URL)
        .header(
            "Authorization",
            format!("Bearer {}", mc_token.access_token().as_ref()),
        )
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return AuthResult::Error(format!("Profile fetch failed: {e}")),
    };

    if !profile_resp.status().is_success() {
        return AuthResult::Error("Account does not own Minecraft".to_string());
    }

    let profile: McProfile = match profile_resp.json().await {
        Ok(p) => p,
        Err(e) => return AuthResult::Error(format!("Profile parse failed: {e}")),
    };

    let uuid = if profile.id.len() == 32 {
        format!(
            "{}-{}-{}-{}-{}",
            &profile.id[..8],
            &profile.id[8..12],
            &profile.id[12..16],
            &profile.id[16..20],
            &profile.id[20..32],
        )
    } else {
        profile.id.clone()
    };

    if let Some(refresh) = ms_refresh_token {
        let _ = store_refresh_token(&uuid, refresh);
    }

    AuthResult::Success(Account {
        uuid,
        username: profile.name,
        account_type: AccountType::Microsoft,
        active: false,
    })
}

// ── Token Refresh ──────────────────────────────────────────────

pub async fn refresh_and_get_token(account: &Account) -> Result<String, String> {
    match account.account_type {
        AccountType::Offline => Ok("0".to_string()),
        AccountType::Microsoft => {
            let refresh = get_refresh_token(&account.uuid)?;

            let oauth_client = BasicClient::new(ClientId::new(CLIENT_ID.to_string()))
                .set_auth_uri(
                    AuthUrl::new(MSA_AUTHORIZE_URL.to_string()).map_err(|e| e.to_string())?,
                )
                .set_token_uri(
                    TokenUrl::new(MSA_TOKEN_URL.to_string()).map_err(|e| e.to_string())?,
                );

            let http_client = reqwest::Client::new();

            let token = oauth_client
                .exchange_refresh_token(&RefreshToken::new(refresh))
                .add_scope(Scope::new("XboxLive.signin".to_string()))
                .add_scope(Scope::new("offline_access".to_string()))
                .request_async(&http_client)
                .await
                .map_err(|e| format!("Token refresh failed: {e}"))?;

            let ms_access_token = token.access_token().secret().to_string();

            if let Some(new_refresh) = token.refresh_token() {
                let _ = store_refresh_token(&account.uuid, new_refresh.secret());
            }

            let mc_flow = MinecraftAuthorizationFlow::new(reqwest::Client::new());
            let mc_token = mc_flow
                .exchange_microsoft_token(&ms_access_token)
                .await
                .map_err(|e| format!("Minecraft auth failed: {e}"))?;

            Ok(mc_token.access_token().as_ref().to_string())
        }
    }
}
