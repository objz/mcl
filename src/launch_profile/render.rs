// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

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
#[path = "tests/render.rs"]
mod tests;
