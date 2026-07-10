{
  description = "rmcl: A fully featured Minecraft TUI launcher";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      packages.default = pkgs.rustPlatform.buildRustPackage {
        pname = "rmcl";
        version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
        src = pkgs.lib.cleanSource ./.;
        cargoLock.lockFile = ./Cargo.lock;
        nativeBuildInputs = [pkgs.jdk];

        doCheck = false;

        meta = with pkgs.lib; {
          description = "A fully featured Minecraft TUI launcher";
          homepage = "https://github.com/objz/rmcl";
          license = licenses.gpl3Only;
          mainProgram = "rmcl";
        };
      };
    })
    // {
      homeManagerModules.default = {
        config,
        lib,
        pkgs,
        ...
      }:
        with lib; let
          cfg = config.programs.rmcl;
          tomlFormat = pkgs.formats.toml {};
          filterNulls = filterAttrs (_: v: v != null);
          nullStr = types.nullOr types.str;
          nullUint = types.nullOr types.ints.unsigned;
          pathsCfg = {
            instances_dir = cfg.instancesDir;
            meta_dir = cfg.metaDir;
            java_path = cfg.javaPath;
          };
          defaultsCfg = {
            memory_min = cfg.memoryMin;
            memory_max = cfg.memoryMax;
          };
          uiCfg = {
            error_auto_dismiss_ms = cfg.errorAutoDismissMs;
            error_slide_start_ms = cfg.errorSlideStartMs;
            error_fly_out_ms = cfg.errorFlyOutMs;
            max_error_events = cfg.maxErrorEvents;
          };
        in {
          options.programs.rmcl = {
            enable = mkEnableOption "rmcl: A fully featured Minecraft TUI launcher";

            package = mkOption {
              type = types.package;
              default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
              defaultText = literalExpression "self.packages.\${pkgs.stdenv.hostPlatform.system}.default";
              description = "The rmcl package to use.";
            };

            # -- config.toml: paths --
            instancesDir = mkOption {
              type = nullStr;
              default = null;
              example = "~/games/minecraft/instances";
              description = ''
                Where Minecraft instances are stored.
                Default: ~/.local/share/rmcl/instances.
              '';
            };

            metaDir = mkOption {
              type = nullStr;
              default = null;
              example = "~/games/minecraft/meta";
              description = ''
                Metadata and cache directory.
                Default: ~/.local/share/rmcl/meta.
              '';
            };

            javaPath = mkOption {
              type = nullStr;
              default = null;
              example = "${pkgs.jdk}/bin/java";
              description = ''
                Path to Java executable for launching Minecraft.
                Default: falls back to JAVA_HOME.
              '';
            };

            # -- config.toml: defaults --
            memoryMin = mkOption {
              type = nullStr;
              default = null;
              example = "512M";
              description = ''
                Minimum Java heap size (Xms) for Minecraft instances.
                Default: "512M".
              '';
            };

            memoryMax = mkOption {
              type = nullStr;
              default = null;
              example = "4G";
              description = ''
                Maximum Java heap size (Xmx) for Minecraft instances.
                Default: "2G".
              '';
            };

            # -- config.toml: ui --
            errorAutoDismissMs = mkOption {
              type = nullUint;
              default = null;
              example = 5000;
              description = ''
                How long (ms) an error toast stays visible before auto-dismissing.
                Default: 5000.
              '';
            };

            errorSlideStartMs = mkOption {
              type = nullUint;
              default = null;
              example = 3500;
              description = ''
                When (ms) the error toast begins sliding off screen.
                Default: 3500.
              '';
            };

            errorFlyOutMs = mkOption {
              type = nullUint;
              default = null;
              example = 300;
              description = ''
                Duration (ms) of the error toast fly-out animation.
                Default: 300.
              '';
            };

            maxErrorEvents = mkOption {
              type = nullUint;
              default = null;
              example = 50;
              description = ''
                Maximum number of error events retained in the log buffer.
                Default: 50.
              '';
            };

            # -- theme.toml --
            theme = mkOption {
              type = types.str;
              default = "catppuccin";
              example = "dracula";
              description = ''
                Base theme name (one of: catppuccin, dracula, nord, gruvbox,
                one-dark, solarized, tailwind, tokyo-night, rose-pine, terminal)
                or an absolute path to a custom theme file.
              '';
            };

            borderStyle = mkOption {
              type = types.enum ["rounded" "plain" "double" "thick"];
              default = "rounded";
              description = "Widget border style.";
            };

            customTheme = mkOption {
              type = types.attrs;
              default = {};
              example = literalExpression ''
                {
                  accent = "Red";
                  text = "White";
                  surface = { Rgb = [36 36 36]; };
                  background = { Rgb = [30 30 30]; };
                }
              '';
              description = ''
                Color overrides applied on top of the base theme.
                Each key is a color slot (accent, accent_dim, text, text_dim,
                text_bright, success, error, warning, info, diff_added,
                diff_removed, diff_context, border, surface, background).
                Values are either a named color string ("Red", "White", etc.)
                or an RGB inline table ({ Rgb = [R G B]; }).
              '';
            };

            # -- extra config.toml fields --
            extraConfig = mkOption {
              type = tomlFormat.type;
              default = {};
              example = literalExpression ''
                {
                  general.debug = true;
                }
              '';
              description = ''
                Additional config.toml settings merged into the generated file.
              '';
            };
          };

          config = mkIf cfg.enable {
            home.packages = [cfg.package];

            xdg.configFile."rmcl/config.toml".source = tomlFormat.generate "rmcl-config.toml" (recursiveUpdate {
                paths = filterNulls pathsCfg;
                defaults = filterNulls defaultsCfg;
                ui = filterNulls uiCfg;
              } cfg.extraConfig);

            xdg.configFile."rmcl/theme.toml".source = tomlFormat.generate "rmcl-theme.toml" ({
                inherit (cfg) theme;
                border_style = cfg.borderStyle;
              }
              // optionalAttrs (cfg.customTheme != {}) {
                custom = cfg.customTheme;
              });
          };
        };
    };
}
