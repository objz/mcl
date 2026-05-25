// renders a parsed launch profile into final argv-style lists for the JVM
// and the game. resolves conditional argument shapes (`{rules, value}`),
// filters them through the rule evaluator, and substitutes mojang template
// variables. legacy `minecraftArguments` strings are tokenised on whitespace
// and treated as a list of game args. pure function; no I/O.

use super::model::{Argument, ArgumentValue, LaunchProfile};
use super::rules::{RuleContext, evaluate};
use super::templates::{TemplateContext, substitute};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedArgs {
    pub jvm: Vec<String>,
    pub main_class: String,
    pub game: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("launch profile is missing a main class")]
    MissingMainClass,
}

pub fn render_args(
    profile: &LaunchProfile,
    rule_ctx: &RuleContext,
    template_ctx: &TemplateContext,
) -> Result<RenderedArgs, RenderError> {
    let main_class = profile
        .main_class
        .clone()
        .ok_or(RenderError::MissingMainClass)?;

    let mut jvm = Vec::new();
    let mut game = Vec::new();

    if let Some(args) = &profile.arguments {
        for arg in &args.jvm {
            push_argument(arg, rule_ctx, template_ctx, &mut jvm);
        }
        for arg in &args.game {
            push_argument(arg, rule_ctx, template_ctx, &mut game);
        }
    } else if let Some(legacy) = &profile.minecraft_arguments {
        for token in legacy.split_whitespace() {
            game.push(substitute(token, template_ctx));
        }
    }

    Ok(RenderedArgs {
        jvm,
        main_class,
        game,
    })
}

fn push_argument(
    arg: &Argument,
    rule_ctx: &RuleContext,
    template_ctx: &TemplateContext,
    out: &mut Vec<String>,
) {
    match arg {
        Argument::Literal(s) => out.push(substitute(s, template_ctx)),
        Argument::Conditional { rules, value } => {
            if !evaluate(rules, rule_ctx) {
                return;
            }
            match value {
                ArgumentValue::Single(s) => out.push(substitute(s, template_ctx)),
                ArgumentValue::Multiple(items) => {
                    for s in items {
                        out.push(substitute(s, template_ctx));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch_profile::model::Arguments;
    use crate::launch_profile::rules::{FeatureSet, OsCondition, Rule, RuleAction};
    use std::path::PathBuf;

    fn template_fixture<'a>(
        library_directory: &'a std::path::Path,
        natives_directory: &'a std::path::Path,
        game_directory: &'a std::path::Path,
        assets_root: &'a std::path::Path,
    ) -> TemplateContext<'a> {
        TemplateContext {
            library_directory,
            classpath_separator: ":",
            version_name: "1.20.1",
            natives_directory,
            classpath: "a.jar:b.jar",
            game_directory,
            assets_root,
            assets_index_name: "5",
            auth_player_name: "Player",
            auth_uuid: "00000000-0000-0000-0000-000000000000",
            auth_access_token: "token",
            auth_xuid: "0",
            user_type: "msa",
            user_properties: "{}",
            launcher_name: "rmcl",
            launcher_version: "0.3.0",
            clientid: "0",
        }
    }

    fn minimal_profile() -> LaunchProfile {
        LaunchProfile {
            id: "test".into(),
            inherits_from: None,
            main_class: Some("net.test.Main".into()),
            libraries: Vec::new(),
            arguments: None,
            minecraft_arguments: None,
            asset_index: None,
            assets: None,
            java_version: None,
            downloads: None,
            release_time: None,
            time: None,
            type_: None,
        }
    }

    #[test]
    fn legacy_minecraft_arguments_render_into_game() {
        let lib = PathBuf::from("/m/libraries");
        let nat = PathBuf::from("/m/natives");
        let game = PathBuf::from("/i/.minecraft");
        let assets = PathBuf::from("/m/assets");
        let template_ctx = template_fixture(&lib, &nat, &game, &assets);
        let features = FeatureSet::default();
        let rule_ctx = RuleContext {
            os_name: "linux",
            arch: "x86_64",
            features: &features,
        };

        let mut profile = minimal_profile();
        profile.minecraft_arguments =
            Some("--username ${auth_player_name} --version ${version_name}".into());

        let rendered = render_args(&profile, &rule_ctx, &template_ctx).unwrap();
        assert_eq!(rendered.main_class, "net.test.Main");
        assert!(rendered.jvm.is_empty());
        assert_eq!(
            rendered.game,
            vec!["--username", "Player", "--version", "1.20.1"]
        );
    }

    #[test]
    fn modern_arguments_render_with_literals_and_substitutions() {
        let lib = PathBuf::from("/m/libraries");
        let nat = PathBuf::from("/m/natives");
        let game = PathBuf::from("/i/.minecraft");
        let assets = PathBuf::from("/m/assets");
        let template_ctx = template_fixture(&lib, &nat, &game, &assets);
        let features = FeatureSet::default();
        let rule_ctx = RuleContext {
            os_name: "linux",
            arch: "x86_64",
            features: &features,
        };

        let mut profile = minimal_profile();
        profile.arguments = Some(Arguments {
            game: vec![
                Argument::Literal("--username".into()),
                Argument::Literal("${auth_player_name}".into()),
            ],
            jvm: vec![Argument::Literal(
                "-Djava.library.path=${natives_directory}".into(),
            )],
        });

        let rendered = render_args(&profile, &rule_ctx, &template_ctx).unwrap();
        assert_eq!(rendered.game, vec!["--username", "Player"]);
        assert_eq!(rendered.jvm, vec!["-Djava.library.path=/m/natives"]);
    }

    #[test]
    fn conditional_argument_with_single_value_is_filtered_by_os_rule() {
        let lib = PathBuf::from("/m/libraries");
        let nat = PathBuf::from("/m/natives");
        let game = PathBuf::from("/i/.minecraft");
        let assets = PathBuf::from("/m/assets");
        let template_ctx = template_fixture(&lib, &nat, &game, &assets);
        let features = FeatureSet::default();
        let rule_ctx = RuleContext {
            os_name: "linux",
            arch: "x86_64",
            features: &features,
        };

        let osx_only = Argument::Conditional {
            rules: vec![Rule {
                action: RuleAction::Allow,
                os: Some(OsCondition {
                    name: Some("osx".into()),
                    arch: None,
                }),
                features: None,
            }],
            value: ArgumentValue::Single("-XstartOnFirstThread".into()),
        };

        let mut profile = minimal_profile();
        profile.arguments = Some(Arguments {
            game: Vec::new(),
            jvm: vec![osx_only],
        });

        let rendered = render_args(&profile, &rule_ctx, &template_ctx).unwrap();
        assert!(
            rendered.jvm.is_empty(),
            "osx-only arg should be skipped on linux"
        );
    }

    #[test]
    fn conditional_argument_with_multiple_value_pushes_all() {
        let lib = PathBuf::from("/m/libraries");
        let nat = PathBuf::from("/m/natives");
        let game = PathBuf::from("/i/.minecraft");
        let assets = PathBuf::from("/m/assets");
        let template_ctx = template_fixture(&lib, &nat, &game, &assets);
        let features = FeatureSet::default();
        let rule_ctx = RuleContext {
            os_name: "linux",
            arch: "x86_64",
            features: &features,
        };

        let linux_arg = Argument::Conditional {
            rules: vec![Rule {
                action: RuleAction::Allow,
                os: Some(OsCondition {
                    name: Some("linux".into()),
                    arch: None,
                }),
                features: None,
            }],
            value: ArgumentValue::Multiple(vec![
                "--add-opens".into(),
                "java.base/sun.security.util=ALL-UNNAMED".into(),
            ]),
        };

        let mut profile = minimal_profile();
        profile.arguments = Some(Arguments {
            game: Vec::new(),
            jvm: vec![linux_arg],
        });

        let rendered = render_args(&profile, &rule_ctx, &template_ctx).unwrap();
        assert_eq!(
            rendered.jvm,
            vec!["--add-opens", "java.base/sun.security.util=ALL-UNNAMED"]
        );
    }

    #[test]
    fn missing_main_class_returns_error() {
        let lib = PathBuf::from("/m/libraries");
        let nat = PathBuf::from("/m/natives");
        let game = PathBuf::from("/i/.minecraft");
        let assets = PathBuf::from("/m/assets");
        let template_ctx = template_fixture(&lib, &nat, &game, &assets);
        let features = FeatureSet::default();
        let rule_ctx = RuleContext {
            os_name: "linux",
            arch: "x86_64",
            features: &features,
        };

        let mut profile = minimal_profile();
        profile.main_class = None;

        let result = render_args(&profile, &rule_ctx, &template_ctx);
        assert!(matches!(result, Err(RenderError::MissingMainClass)));
    }

    #[test]
    fn modern_arguments_takes_precedence_over_legacy_field() {
        // a profile that somehow has both arguments and minecraft_arguments
        // should use arguments only (legacy is fallback).
        let lib = PathBuf::from("/m/libraries");
        let nat = PathBuf::from("/m/natives");
        let game = PathBuf::from("/i/.minecraft");
        let assets = PathBuf::from("/m/assets");
        let template_ctx = template_fixture(&lib, &nat, &game, &assets);
        let features = FeatureSet::default();
        let rule_ctx = RuleContext {
            os_name: "linux",
            arch: "x86_64",
            features: &features,
        };

        let mut profile = minimal_profile();
        profile.arguments = Some(Arguments {
            game: vec![Argument::Literal("--from-arguments".into())],
            jvm: Vec::new(),
        });
        profile.minecraft_arguments = Some("--from-legacy".into());

        let rendered = render_args(&profile, &rule_ctx, &template_ctx).unwrap();
        assert_eq!(rendered.game, vec!["--from-arguments"]);
    }
}
