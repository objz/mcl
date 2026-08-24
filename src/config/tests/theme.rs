// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

// cover every BorderStyle variant. a mutation that swaps two arms of the
// match (e.g. Rounded -> Plain) would slip past testing just one variant.
#[rstest::rstest]
#[case::plain(BorderStyle::Plain, BorderType::Plain)]
#[case::rounded(BorderStyle::Rounded, BorderType::Rounded)]
#[case::double(BorderStyle::Double, BorderType::Double)]
#[case::thick(BorderStyle::Thick, BorderType::Thick)]
fn border_style_roundtrip(#[case] style: BorderStyle, #[case] expected: BorderType) {
    assert_eq!(style.to_border_type(), expected);
}

#[test]
fn theme_config_deserialize_builtin() {
    let toml_str = r#"
theme = "dracula"
border_style = "plain"
"#;
    let config: ThemeConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.theme, "dracula");
    assert_eq!(config.border_style, BorderStyle::Plain);
    assert!(config.custom.is_none());
}

#[test]
fn theme_config_with_partial_overrides() {
    let toml_str = r#"
theme = "dracula"

[custom]
accent = "Red"
"#;
    let config: ThemeConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.theme, "dracula");
    let overrides = config.custom.unwrap();
    assert_eq!(overrides.accent, Some(Color::Red));
    assert!(overrides.text.is_none());
}

#[test]
fn resolve_with_overrides_keeps_base() {
    let config = ThemeConfig {
        theme: "dracula".to_owned(),
        custom: Some(ThemeOverrides {
            accent: Some(Color::Red),
            ..ThemeOverrides::default()
        }),
        ..ThemeConfig::default()
    };
    let theme = resolve_app_theme(&config);
    assert_eq!(theme.accent(), Color::Red);
    let base = resolve_theme("dracula");
    assert_eq!(theme.text(), base.text());
    assert_eq!(theme.error(), base.error());
}

#[test]
fn resolve_builtin_theme() {
    let theme = resolve_theme("dracula");
    let expected = if std::env::var_os("NO_COLOR").is_some() {
        "no-color"
    } else {
        "dracula"
    };
    assert_eq!(theme.id(), expected);
}

// a theme file in config/theme/ is a flat CustomTheme: name/id/accent are
// required, everything else falls back to themekit defaults.
#[test]
fn custom_theme_file_parses() {
    let full = toml::from_str::<CustomTheme>(include_str!("../../../assets/example-theme.toml"))
        .expect("bundled example theme must parse");
    assert_eq!(full.id, "my-theme");
    assert_eq!(full.accent, Color::Rgb(249, 115, 22));
    assert_eq!(full.text, Color::Rgb(205, 214, 244));

    let minimal =
        toml::from_str::<CustomTheme>("name = \"X\"\nid = \"x\"\naccent = \"#f97316\"\n").unwrap();
    assert_eq!(minimal.accent, Color::Rgb(249, 115, 22));

    // the two failure modes users hit: nesting under [theme], or omitting name/id
    assert!(
        toml::from_str::<CustomTheme>("[theme]\nname = \"X\"\nid = \"x\"\naccent = \"Red\"\n")
            .is_err()
    );
    assert!(toml::from_str::<CustomTheme>("accent = \"Red\"\n").is_err());
}

#[test]
fn theme_config_empty_toml_uses_defaults() {
    let config: ThemeConfig = toml::from_str("").unwrap();
    assert_eq!(config.theme, "catppuccin");
    assert_eq!(config.border_style, BorderStyle::Rounded);
}
