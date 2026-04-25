// auth module: account storage (offline + microsoft) and the oauth device code flow
mod accounts;
mod oauth;

pub use accounts::{
    Account, AccountStore, AccountType, AuthResult, account_store_path, create_offline_account,
    offline_uuid,
};
pub use oauth::{DEVICE_CODE_DISPLAY, DeviceCodeInfo, refresh_and_get_token, start_microsoft_auth};
