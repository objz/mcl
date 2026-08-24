// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn test_game_version_to_neoforge_prefix() {
    assert_eq!(
        game_version_to_neoforge_prefix("1.21"),
        Some("21.0.".to_string())
    );
    assert_eq!(
        game_version_to_neoforge_prefix("1.20.4"),
        Some("20.4.".to_string())
    );
    assert_eq!(
        game_version_to_neoforge_prefix("1.21.1"),
        Some("21.1.".to_string())
    );
    assert_eq!(
        game_version_to_neoforge_prefix("26.1.2"),
        Some("26.1.2.".to_string())
    );
    assert_eq!(game_version_to_neoforge_prefix("invalid"), None);
}
