// template substitution for mojang-style launch arguments. profiles use
// `${variable_name}` placeholders that the launcher fills in at launch
// time from the active session: paths, the user's account info, the
// classpath, the resolved natives directory, and so on.
//
// the full set of variables is documented in `TemplateContext`. unknown
// placeholders are left as-is and logged at `warn` level - that way if
// mojang adds a new variable in the future, the launcher fails open
// rather than silently swallowing it.

use std::path::Path;

pub struct TemplateContext<'a> {
    pub library_directory: &'a Path,
    pub classpath_separator: &'a str,
    pub version_name: &'a str,
    pub version_type: &'a str,
    pub natives_directory: &'a Path,
    pub classpath: &'a str,
    pub game_directory: &'a Path,
    pub assets_root: &'a Path,
    pub assets_index_name: &'a str,
    pub auth_player_name: &'a str,
    pub auth_uuid: &'a str,
    pub auth_access_token: &'a str,
    pub auth_xuid: &'a str,
    pub user_type: &'a str,
    pub user_properties: &'a str,
    pub launcher_name: &'a str,
    pub launcher_version: &'a str,
    pub clientid: &'a str,
}

pub fn substitute(input: &str, ctx: &TemplateContext) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(open) = rest.find("${") {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 2..];
        match after_open.find('}') {
            Some(close_rel) => {
                let name = &after_open[..close_rel];
                match lookup(name, ctx) {
                    Some(value) => out.push_str(&value),
                    None => {
                        tracing::warn!("unknown launch template variable: ${{{}}}", name);
                        out.push_str("${");
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after_open[close_rel + 1..];
            }
            None => {
                // unclosed `${...` - emit the rest literally and stop.
                out.push_str("${");
                out.push_str(after_open);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

// quick-play templates (`${quickPlayPath}`, `${quickPlaySingleplayer}`,
// `${quickPlayMultiplayer}`, `${quickPlayRealms}`) are intentionally not
// listed here. they only appear in `arguments.game` entries gated on
// `is_quick_play_*` feature flags; since rmcl never sets those flags
// (FeatureSet defaults to None across the board), the surrounding
// conditional argument is filtered out by the rule evaluator before
// template substitution even runs. if we ever expose quick-play to users,
// add the variables here AND set the corresponding feature flags in
// RuleContext at launch.
fn lookup(name: &str, ctx: &TemplateContext) -> Option<String> {
    Some(match name {
        "library_directory" => ctx.library_directory.display().to_string(),
        "classpath_separator" => ctx.classpath_separator.to_string(),
        "version_name" => ctx.version_name.to_string(),
        "version_type" => ctx.version_type.to_string(),
        "natives_directory" => ctx.natives_directory.display().to_string(),
        "classpath" => ctx.classpath.to_string(),
        "game_directory" => ctx.game_directory.display().to_string(),
        "assets_root" => ctx.assets_root.display().to_string(),
        "assets_index_name" => ctx.assets_index_name.to_string(),
        "auth_player_name" => ctx.auth_player_name.to_string(),
        "auth_uuid" => ctx.auth_uuid.to_string(),
        "auth_access_token" => ctx.auth_access_token.to_string(),
        "auth_xuid" => ctx.auth_xuid.to_string(),
        "user_type" => ctx.user_type.to_string(),
        "user_properties" => ctx.user_properties.to_string(),
        "launcher_name" => ctx.launcher_name.to_string(),
        "launcher_version" => ctx.launcher_version.to_string(),
        "clientid" => ctx.clientid.to_string(),
        _ => return None,
    })
}

#[cfg(test)]
#[path = "tests/templates.rs"]
mod tests;
