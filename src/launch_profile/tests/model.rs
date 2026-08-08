use super::*;
use crate::launch_profile::rules::RuleAction;

const MODERN_FIXTURE: &str = r#"{
        "id": "1.20.1",
        "type": "release",
        "mainClass": "net.minecraft.client.main.Main",
        "assetIndex": {
            "id": "5",
            "url": "https://example.invalid/5.json",
            "sha1": "0000000000000000000000000000000000000000"
        },
        "javaVersion": {
            "component": "java-runtime-gamma",
            "majorVersion": 17
        },
        "libraries": [
            {
                "name": "org.lwjgl:lwjgl:3.3.1",
                "downloads": {
                    "artifact": {
                        "url": "https://example.invalid/lwjgl.jar",
                        "path": "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar",
                        "sha1": "1111111111111111111111111111111111111111",
                        "size": 100
                    }
                },
                "rules": [
                    { "action": "allow", "os": { "name": "linux" } }
                ]
            }
        ],
        "arguments": {
            "game": [
                "--username", "${auth_player_name}",
                {
                    "rules": [{ "action": "allow", "features": { "is_demo_user": true } }],
                    "value": "--demo"
                }
            ],
            "jvm": [
                "-Djava.library.path=${natives_directory}",
                {
                    "rules": [{ "action": "allow", "os": { "name": "osx" } }],
                    "value": ["-XstartOnFirstThread"]
                }
            ]
        }
    }"#;

const LEGACY_FIXTURE: &str = r#"{
        "id": "1.7.10",
        "type": "release",
        "mainClass": "net.minecraft.client.main.Main",
        "minecraftArguments": "--username ${auth_player_name} --version ${version_name} --gameDir ${game_directory}",
        "assetIndex": {
            "id": "1.7.10",
            "url": "https://example.invalid/1.7.10.json",
            "sha1": "0000000000000000000000000000000000000000"
        },
        "libraries": []
    }"#;

const LOADER_FIXTURE: &str = r#"{
        "id": "1.20.1-forge-47.2.0",
        "inheritsFrom": "1.20.1",
        "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
        "libraries": [
            { "name": "net.minecraftforge:forge:47.2.0" }
        ],
        "arguments": {
            "game": ["--launchTarget", "forge_client"],
            "jvm": [
                "--add-opens", "java.base/sun.security.util=cpw.mods.securejarhandler",
                "-DlibraryDirectory=${library_directory}"
            ]
        }
    }"#;

#[test]
fn parses_modern_arguments_object() {
    let profile: LaunchProfile = serde_json::from_str(MODERN_FIXTURE).unwrap();
    assert_eq!(profile.id, "1.20.1");
    assert_eq!(
        profile.main_class.as_deref(),
        Some("net.minecraft.client.main.Main")
    );
    assert!(profile.inherits_from.is_none());
    assert!(profile.minecraft_arguments.is_none());

    let args = profile.arguments.as_ref().expect("arguments present");
    assert_eq!(args.game.len(), 3);
    assert_eq!(args.jvm.len(), 2);

    // first game arg should be a literal "--username"
    match &args.game[0] {
        Argument::Literal(s) => assert_eq!(s, "--username"),
        _ => panic!("expected literal"),
    }
    // third game arg should be a conditional with a single-string value
    match &args.game[2] {
        Argument::Conditional { rules, value } => {
            assert_eq!(rules.len(), 1);
            assert_eq!(rules[0].action, RuleAction::Allow);
            assert!(matches!(value, ArgumentValue::Single(_)));
        }
        _ => panic!("expected conditional"),
    }
    // second jvm arg should be a conditional with a multi-string value
    match &args.jvm[1] {
        Argument::Conditional { value, .. } => {
            assert!(matches!(value, ArgumentValue::Multiple(_)));
        }
        _ => panic!("expected conditional"),
    }
}

#[test]
fn parses_legacy_minecraft_arguments_string() {
    let profile: LaunchProfile = serde_json::from_str(LEGACY_FIXTURE).unwrap();
    assert_eq!(profile.id, "1.7.10");
    assert!(profile.arguments.is_none());
    assert!(
        profile
            .minecraft_arguments
            .as_deref()
            .unwrap()
            .contains("${version_name}")
    );
    assert!(profile.libraries.is_empty());
}

#[test]
fn parses_loader_profile_with_inherits_from() {
    let profile: LaunchProfile = serde_json::from_str(LOADER_FIXTURE).unwrap();
    assert_eq!(profile.id, "1.20.1-forge-47.2.0");
    assert_eq!(profile.inherits_from.as_deref(), Some("1.20.1"));
    assert!(profile.asset_index.is_none()); // inherited from parent
    let args = profile.arguments.as_ref().unwrap();
    assert_eq!(args.game.len(), 2);
    assert_eq!(args.jvm.len(), 3);
}

#[test]
fn modern_profile_round_trips() {
    let original: LaunchProfile = serde_json::from_str(MODERN_FIXTURE).unwrap();
    let serialized = serde_json::to_string(&original).unwrap();
    let reparsed: LaunchProfile = serde_json::from_str(&serialized).unwrap();
    assert_eq!(original, reparsed);
}

#[test]
fn loader_profile_round_trips() {
    let original: LaunchProfile = serde_json::from_str(LOADER_FIXTURE).unwrap();
    let serialized = serde_json::to_string(&original).unwrap();
    let reparsed: LaunchProfile = serde_json::from_str(&serialized).unwrap();
    assert_eq!(original, reparsed);
}

#[test]
fn legacy_profile_round_trips() {
    let original: LaunchProfile = serde_json::from_str(LEGACY_FIXTURE).unwrap();
    let serialized = serde_json::to_string(&original).unwrap();
    let reparsed: LaunchProfile = serde_json::from_str(&serialized).unwrap();
    assert_eq!(original, reparsed);
}
