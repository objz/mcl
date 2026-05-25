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

fn lookup(name: &str, ctx: &TemplateContext) -> Option<String> {
    Some(match name {
        "library_directory" => ctx.library_directory.display().to_string(),
        "classpath_separator" => ctx.classpath_separator.to_string(),
        "version_name" => ctx.version_name.to_string(),
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
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ctx_fixture<'a>(
        library_directory: &'a Path,
        natives_directory: &'a Path,
        game_directory: &'a Path,
        assets_root: &'a Path,
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

    #[test]
    fn no_placeholders_unchanged() {
        let lib = PathBuf::from("/m/libraries");
        let nat = PathBuf::from("/m/natives");
        let game = PathBuf::from("/i/.minecraft");
        let assets = PathBuf::from("/m/assets");
        let ctx = ctx_fixture(&lib, &nat, &game, &assets);
        assert_eq!(
            substitute("--add-modules ALL-MODULE-PATH", &ctx),
            "--add-modules ALL-MODULE-PATH"
        );
    }

    #[test]
    fn single_known_substitution() {
        let lib = PathBuf::from("/m/libraries");
        let nat = PathBuf::from("/m/natives");
        let game = PathBuf::from("/i/.minecraft");
        let assets = PathBuf::from("/m/assets");
        let ctx = ctx_fixture(&lib, &nat, &game, &assets);
        assert_eq!(substitute("v=${version_name}", &ctx), "v=1.20.1");
    }

    #[test]
    fn unknown_placeholder_left_as_is() {
        let lib = PathBuf::from("/m/libraries");
        let nat = PathBuf::from("/m/natives");
        let game = PathBuf::from("/i/.minecraft");
        let assets = PathBuf::from("/m/assets");
        let ctx = ctx_fixture(&lib, &nat, &game, &assets);
        assert_eq!(
            substitute("x=${not_a_real_var}y", &ctx),
            "x=${not_a_real_var}y"
        );
    }

    #[test]
    fn unclosed_placeholder_left_as_is() {
        let lib = PathBuf::from("/m/libraries");
        let nat = PathBuf::from("/m/natives");
        let game = PathBuf::from("/i/.minecraft");
        let assets = PathBuf::from("/m/assets");
        let ctx = ctx_fixture(&lib, &nat, &game, &assets);
        assert_eq!(
            substitute("--prefix ${unclosed", &ctx),
            "--prefix ${unclosed"
        );
    }

    #[test]
    fn dollar_without_brace_is_literal() {
        let lib = PathBuf::from("/m/libraries");
        let nat = PathBuf::from("/m/natives");
        let game = PathBuf::from("/i/.minecraft");
        let assets = PathBuf::from("/m/assets");
        let ctx = ctx_fixture(&lib, &nat, &game, &assets);
        assert_eq!(substitute("$$ literal $5 $", &ctx), "$$ literal $5 $");
    }

    #[test]
    fn multiple_substitutions() {
        let lib = PathBuf::from("/m/libraries");
        let nat = PathBuf::from("/m/natives");
        let game = PathBuf::from("/i/.minecraft");
        let assets = PathBuf::from("/m/assets");
        let ctx = ctx_fixture(&lib, &nat, &game, &assets);
        assert_eq!(
            substitute("${version_name}-${auth_player_name}", &ctx),
            "1.20.1-Player"
        );
    }

    #[test]
    fn path_substitution() {
        let lib = PathBuf::from("/m/libraries");
        let nat = PathBuf::from("/m/natives");
        let game = PathBuf::from("/i/.minecraft");
        let assets = PathBuf::from("/m/assets");
        let ctx = ctx_fixture(&lib, &nat, &game, &assets);
        assert_eq!(
            substitute("-DlibraryDirectory=${library_directory}", &ctx),
            "-DlibraryDirectory=/m/libraries"
        );
    }

    #[test]
    fn substituted_value_is_not_recursively_substituted() {
        // simulate a `user_properties` value that happens to contain a `${...}`
        // pattern. it should NOT trigger another substitution pass.
        let lib = PathBuf::from("/m/libraries");
        let nat = PathBuf::from("/m/natives");
        let game = PathBuf::from("/i/.minecraft");
        let assets = PathBuf::from("/m/assets");
        let ctx = TemplateContext {
            library_directory: &lib,
            classpath_separator: ":",
            version_name: "1.20.1",
            natives_directory: &nat,
            classpath: "a.jar:b.jar",
            game_directory: &game,
            assets_root: &assets,
            assets_index_name: "5",
            auth_player_name: "Player",
            auth_uuid: "00000000-0000-0000-0000-000000000000",
            auth_access_token: "token",
            auth_xuid: "0",
            user_type: "msa",
            user_properties: "${version_name}",
            launcher_name: "rmcl",
            launcher_version: "0.3.0",
            clientid: "0",
        };
        assert_eq!(substitute("${user_properties}", &ctx), "${version_name}");
    }

    #[test]
    fn empty_input_is_empty() {
        let lib = PathBuf::from("/m/libraries");
        let nat = PathBuf::from("/m/natives");
        let game = PathBuf::from("/i/.minecraft");
        let assets = PathBuf::from("/m/assets");
        let ctx = ctx_fixture(&lib, &nat, &game, &assets);
        assert_eq!(substitute("", &ctx), "");
    }

    #[test]
    fn windows_style_backslashes_in_value_pass_through() {
        // simulate a Windows install where library_directory is a path with
        // backslashes. the substitution must not interpret backslashes as
        // escape sequences or do anything else clever - it just copies the
        // value into the output.
        let lib = PathBuf::from(r"C:\Users\test\.minecraft\libraries");
        let nat = PathBuf::from(r"C:\Users\test\.minecraft\natives");
        let game = PathBuf::from(r"C:\Users\test\.minecraft");
        let assets = PathBuf::from(r"C:\Users\test\.minecraft\assets");
        let ctx = ctx_fixture(&lib, &nat, &game, &assets);
        let result = substitute("-Dpath=${library_directory}", &ctx);
        assert!(
            result.contains(r"C:\Users\test\.minecraft\libraries"),
            "expected backslashes preserved, got: {result}"
        );
    }
}
