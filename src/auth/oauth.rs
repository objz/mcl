// microsoft oauth device code flow for minecraft authentication.
// the flow goes: MSA device code -> MSA token -> xbox/mc token exchange -> mc profile fetch.
// device code auth is used because it works without a redirect URI, which is nice for a TUI.

use std::sync::{Arc, LazyLock, Mutex};

use minecraft_msa_auth::MinecraftAuthorizationFlow;
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, ClientId, DeviceAuthorizationUrl, RefreshToken, Scope,
    StandardDeviceAuthorizationResponse, TokenResponse, TokenUrl,
};
use serde::Deserialize;

use super::accounts::{Account, AccountType, AuthResult};

const CLIENT_ID: &str = "708e91b5-99f8-4a1d-80ec-e746cbb24771";
const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const MSA_AUTHORIZE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
const MSA_TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceCodeInfo {
    pub user_code: String,
    pub verification_uri: String,
}

#[derive(Deserialize)]
struct McProfile {
    id: String,
    name: String,
}

// shared slot so the TUI can poll for the device code to show the user
pub static DEVICE_CODE_DISPLAY: LazyLock<Arc<Mutex<Option<DeviceCodeInfo>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(None)));

async fn run_full_oauth_flow() -> Result<(String, Option<String>), String> {
    let oauth_client = BasicClient::new(ClientId::new(CLIENT_ID.to_owned()))
        .set_auth_uri(AuthUrl::new(MSA_AUTHORIZE_URL.to_owned()).map_err(|e| e.to_string())?)
        .set_token_uri(TokenUrl::new(MSA_TOKEN_URL.to_owned()).map_err(|e| e.to_string())?)
        .set_device_authorization_url(
            DeviceAuthorizationUrl::new(DEVICE_CODE_URL.to_owned()).map_err(|e| e.to_string())?,
        );

    let http_client = reqwest::Client::new();

    let details: StandardDeviceAuthorizationResponse = oauth_client
        .exchange_device_code()
        .add_scope(Scope::new("XboxLive.signin".to_owned()))
        .add_scope(Scope::new("offline_access".to_owned()))
        .request_async(&http_client)
        .await
        .map_err(|e| format!("Device code request failed: {e}"))?;

    if let Ok(mut slot) = DEVICE_CODE_DISPLAY.lock() {
        *slot = Some(DeviceCodeInfo {
            user_code: details.user_code().secret().to_owned(),
            verification_uri: details.verification_uri().to_string(),
        });
    }

    let token = oauth_client
        .exchange_device_access_token(&details)
        .request_async(&http_client, tokio::time::sleep, None)
        .await
        .map_err(|e| format!("Authentication failed: {e}"))?;

    let ms_access_token = token.access_token().secret().to_owned();
    let ms_refresh_token = token.refresh_token().map(|r| r.secret().to_owned());

    Ok((ms_access_token, ms_refresh_token))
}

// kicks off auth on a background task, returns a mutex the caller can poll for the result.
// the TUI checks DEVICE_CODE_DISPLAY for the code to show, and this mutex for completion.
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
        return AuthResult::Error("Account does not own Minecraft".to_owned());
    }

    let profile: McProfile = match profile_resp.json().await {
        Ok(p) => p,
        Err(e) => return AuthResult::Error(format!("Profile parse failed: {e}")),
    };

    // mojang returns uuids without dashes because of course they do
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

    AuthResult::Success(Account {
        uuid,
        username: profile.name,
        account_type: AccountType::Microsoft,
        active: false,
        refresh_token: ms_refresh_token.map(|s| s.to_owned()),
    })
}

// returns (mc_access_token, new_refresh_token). for offline accounts it just
// hands back a dummy token since the server doesn't care.
pub async fn refresh_and_get_token(account: &Account) -> Result<(String, Option<String>), String> {
    match account.account_type {
        AccountType::Offline => Ok(("0".to_owned(), None)),
        AccountType::Microsoft => {
            let refresh = account.refresh_token.as_deref().ok_or_else(|| {
                format!(
                    "No saved credentials for '{}'. Please remove and re-add the account.",
                    account.username
                )
            })?;

            let oauth_client = BasicClient::new(ClientId::new(CLIENT_ID.to_owned()))
                .set_auth_uri(
                    AuthUrl::new(MSA_AUTHORIZE_URL.to_owned()).map_err(|e| e.to_string())?,
                )
                .set_token_uri(TokenUrl::new(MSA_TOKEN_URL.to_owned()).map_err(|e| e.to_string())?);

            let http_client = reqwest::Client::new();

            let token = oauth_client
                .exchange_refresh_token(&RefreshToken::new(refresh.to_owned()))
                .add_scope(Scope::new("XboxLive.signin".to_owned()))
                .add_scope(Scope::new("offline_access".to_owned()))
                .request_async(&http_client)
                .await
                .map_err(|e| format!("Token refresh failed: {e}"))?;

            let ms_access_token = token.access_token().secret().to_owned();
            let new_refresh = token.refresh_token().map(|r| r.secret().to_owned());

            let mc_flow = MinecraftAuthorizationFlow::new(reqwest::Client::new());
            let mc_token = mc_flow
                .exchange_microsoft_token(&ms_access_token)
                .await
                .map_err(|e| format!("Minecraft auth failed: {e}"))?;

            Ok((mc_token.access_token().as_ref().to_owned(), new_refresh))
        }
    }
}
