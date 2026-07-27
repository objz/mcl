use super::*;

fn microsoft_account(cached_mc_token_expires_at: Option<i64>) -> Account {
    Account {
        uuid: "00000000-0000-0000-0000-000000000000".to_owned(),
        username: "TestPlayer".to_owned(),
        account_type: AccountType::Microsoft,
        active: true,
        refresh_token: Some("refresh".to_owned()),
        cached_mc_token: Some("cached".to_owned()),
        cached_mc_token_expires_at,
    }
}

#[test]
fn cached_mc_token_is_valid_before_refresh_margin() {
    let now = 1_000;
    let account = microsoft_account(Some(now + MC_TOKEN_CACHE_REFRESH_MARGIN_SECS + 1));

    assert_eq!(valid_cached_mc_token(&account, now), Some("cached"));
}

#[test]
fn cached_mc_token_expires_inside_refresh_margin() {
    let now = 1_000;
    let account = microsoft_account(Some(now + MC_TOKEN_CACHE_REFRESH_MARGIN_SECS));

    assert!(valid_cached_mc_token(&account, now).is_none());
}

#[test]
fn cached_mc_token_requires_expiry() {
    let account = microsoft_account(None);

    assert!(valid_cached_mc_token(&account, 1_000).is_none());
}

#[test]
fn profile_uuid_is_normalized_without_slicing_unicode() {
    assert_eq!(
        normalize_profile_uuid("0123456789abcdef0123456789abcdef"),
        Some("01234567-89ab-cdef-0123-456789abcdef".to_owned())
    );

    let unicode_id = format!("{}é{}", "a".repeat(7), "b".repeat(23));
    assert_eq!(unicode_id.len(), 32);
    assert_eq!(normalize_profile_uuid(&unicode_id), None);
    assert_eq!(
        normalize_profile_uuid("not-a-valid-minecraft-profile-id"),
        None
    );
}
