// mod loader installation. each loader (fabric, forge, neoforge, quilt, vanilla)
// implements the same trait so the UI can treat them uniformly: pick game version,
// pick loader version, install. the actual installation strategies differ wildly
// though (fabric/quilt just download jars, forge/neoforge run a whole java installer).

mod fabric;
mod forge;
mod neoforge;
mod quilt;
mod vanilla;

use std::path::Path;

use async_trait::async_trait;

use crate::instance::models::ModLoader;
use crate::net::{HttpClient, NetError};

pub use vanilla::VanillaInstaller;

#[derive(Debug, Clone)]
pub struct GameVersion {
    pub id: String,
    pub stable: bool,
}

#[async_trait]
pub trait ModLoaderInstaller: Send + Sync {
    fn loader_type(&self) -> ModLoader;

    async fn get_game_versions(&self, client: &HttpClient) -> Result<Vec<GameVersion>, NetError>;

    async fn get_versions(
        &self,
        client: &HttpClient,
        game_version: &str,
    ) -> Result<Vec<String>, NetError>;

    async fn install(
        &self,
        client: &HttpClient,
        game_version: &str,
        loader_version: &str,
        instance_dir: &Path,
        meta_dir: &Path,
    ) -> Result<(), NetError>;
}

// writes raw profile JSON bytes to meta_dir/loader-profiles/<filename>.
// callers that already have the upstream bytes (fabric/quilt http fetch,
// legacy forge versionInfo extract) use this directly to keep the on-disk
// file byte-for-byte identical to the source.
pub(crate) fn save_profile_bytes(
    meta_dir: &Path,
    filename: &str,
    bytes: &[u8],
) -> std::io::Result<()> {
    let profiles_dir = meta_dir.join("loader-profiles");
    std::fs::create_dir_all(&profiles_dir)?;
    std::fs::write(profiles_dir.join(filename), bytes)
}

// used by forge/neoforge. their java installer drops a version json into
// .minecraft/versions/. we copy that file byte-for-byte to our loader
// profile cache so launch-time code sees the full upstream JSON -
// inheritsFrom, arguments.jvm, library rules, all of it - instead of a
// stripped-down version that would silently drop modern features (e.g.
// the --add-opens flags forge 1.17+ ships for java 17+ support).
pub(crate) fn save_installer_profile(
    instance_dir: &Path,
    meta_dir: &Path,
    version_dir_name: &str,
    profile_filename: &str,
) -> Result<(), NetError> {
    let ver_json_path = instance_dir
        .join(".minecraft")
        .join("versions")
        .join(version_dir_name)
        .join(format!("{version_dir_name}.json"));

    if !ver_json_path.exists() {
        return Err(NetError::Parse(format!(
            "Version JSON not found at {}",
            ver_json_path.display()
        )));
    }

    let raw = std::fs::read(&ver_json_path)?;

    let profiles_dir = meta_dir.join("loader-profiles");
    std::fs::create_dir_all(&profiles_dir)?;
    let profile_path = profiles_dir.join(profile_filename);
    std::fs::write(&profile_path, &raw)?;
    Ok(())
}

pub fn get_installer(loader: ModLoader) -> Box<dyn ModLoaderInstaller + Send + Sync> {
    match loader {
        ModLoader::Vanilla => Box::new(vanilla::VanillaInstaller),
        ModLoader::Fabric => Box::new(fabric::FabricInstaller),
        ModLoader::Forge => Box::new(forge::ForgeInstaller),
        ModLoader::NeoForge => Box::new(neoforge::NeoForgeInstaller),
        ModLoader::Quilt => Box::new(quilt::QuiltInstaller),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vanilla_factory() {
        let installer = get_installer(ModLoader::Vanilla);
        assert_eq!(installer.loader_type(), ModLoader::Vanilla);
    }

    #[tokio::test]
    async fn test_vanilla_get_versions() {
        let client = HttpClient::new();
        let installer = VanillaInstaller;
        let versions = installer.get_versions(&client, "1.20.1").await.unwrap();
        assert!(!versions.is_empty());
        assert_eq!(versions[0], "vanilla");
    }

    #[tokio::test]
    async fn test_vanilla_install_noop() {
        let client = HttpClient::new();
        let installer = VanillaInstaller;
        let tmp = std::env::temp_dir().join("rmcl_test_vanilla_install");
        let meta = std::env::temp_dir().join("rmcl_test_meta");
        installer
            .install(&client, "1.20.1", "vanilla", &tmp, &meta)
            .await
            .unwrap();
    }

    #[tokio::test]
    #[ignore = "hits live Mojang API"]
    async fn test_vanilla_get_game_versions() {
        let client = HttpClient::new();
        let installer = VanillaInstaller;
        let versions = installer.get_game_versions(&client).await.unwrap();
        assert!(!versions.is_empty());
        assert!(versions.iter().any(|v| v.id == "1.20.1"));
    }

    #[test]
    fn save_installer_profile_copies_raw_bytes_verbatim() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let instance_dir = tmp.path().join("instance");
        let meta_dir = tmp.path().join("meta");

        // a synthetic installer version JSON with the modern arguments
        // object - exactly the shape we used to strip.
        let installer_json = br#"{
            "id": "1.20.1-forge-47.2.0",
            "inheritsFrom": "1.20.1",
            "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
            "libraries": [{ "name": "net.minecraftforge:forge:47.2.0" }],
            "arguments": {
                "game": ["--launchTarget", "forge_client"],
                "jvm": ["--add-opens", "java.base/sun.security.util=cpw.mods.securejarhandler"]
            }
        }"#;

        let ver_dir = instance_dir
            .join(".minecraft")
            .join("versions")
            .join("1.20.1-forge-47.2.0");
        std::fs::create_dir_all(&ver_dir).unwrap();
        let ver_json_path = ver_dir.join("1.20.1-forge-47.2.0.json");
        std::fs::write(&ver_json_path, installer_json).unwrap();

        save_installer_profile(
            &instance_dir,
            &meta_dir,
            "1.20.1-forge-47.2.0",
            "forge-1.20.1-47.2.0.json",
        )
        .unwrap();

        let saved = std::fs::read(
            meta_dir
                .join("loader-profiles")
                .join("forge-1.20.1-47.2.0.json"),
        )
        .unwrap();
        assert_eq!(
            saved,
            installer_json.to_vec(),
            "saved profile should be byte-for-byte identical to installer output"
        );
    }

    #[test]
    fn legacy_forge_version_info_round_trips_through_save_profile_json() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let meta_dir = tmp.path().to_path_buf();

        // synthetic versionInfo shape from a 1.7.10 forge install_profile.json.
        // legacy forge uses minecraftArguments (the flat string format) and
        // typically omits inheritsFrom (the launch flow's implicit fallback
        // handles that).
        let version_info = serde_json::json!({
            "id": "1.7.10-Forge10.13.4.1614-1.7.10",
            "mainClass": "net.minecraft.launchwrapper.Launch",
            "minecraftArguments": "--username ${auth_player_name} --tweakClass cpw.mods.fml.common.launcher.FMLTweaker",
            "libraries": [
                { "name": "net.minecraftforge:forge:10.13.4.1614", "url": "http://files.minecraftforge.net/maven/" },
                { "name": "net.minecraft:launchwrapper:1.9" }
            ]
        });

        // mirror the install path: write versionInfo as compact JSON.
        let profiles_dir = meta_dir.join("loader-profiles");
        std::fs::create_dir_all(&profiles_dir).unwrap();
        let serialized = serde_json::to_vec(&version_info).unwrap();
        std::fs::write(
            profiles_dir.join("forge-1.7.10-10.13.4.1614.json"),
            &serialized,
        )
        .unwrap();

        let saved_bytes = std::fs::read(
            meta_dir
                .join("loader-profiles")
                .join("forge-1.7.10-10.13.4.1614.json"),
        )
        .unwrap();

        // the cached profile must parse as a LaunchProfile so the launch
        // flow's render_args + resolve pipeline can consume it.
        let profile: crate::launch_profile::model::LaunchProfile =
            serde_json::from_slice(&saved_bytes).unwrap();
        assert_eq!(profile.id, "1.7.10-Forge10.13.4.1614-1.7.10");
        assert_eq!(
            profile.main_class.as_deref(),
            Some("net.minecraft.launchwrapper.Launch")
        );
        assert!(profile.inherits_from.is_none()); // launch flow's implicit fallback adds it
        assert!(
            profile
                .minecraft_arguments
                .as_deref()
                .unwrap()
                .contains("--tweakClass")
        );
        assert_eq!(profile.libraries.len(), 2);
        assert_eq!(
            profile.libraries[0].name,
            "net.minecraftforge:forge:10.13.4.1614"
        );
        assert_eq!(
            profile.libraries[0].url.as_deref(),
            Some("http://files.minecraftforge.net/maven/")
        );
        // legacy libs typically have no `downloads.artifact` - they're
        // resolved at launch time via maven_coord_to_path(name).
        assert!(profile.libraries[0].downloads.is_none());
    }

    #[test]
    fn raw_fabric_profile_bytes_parse_as_launch_profile() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let meta_dir = tmp.path();

        // a synthetic upstream fabric profile shape (id, mainClass,
        // libraries with name+url, no inheritsFrom, no arguments).
        // simulates what the install path will write verbatim.
        let upstream_bytes = br#"{
            "id": "fabric-loader-0.14.21-1.20.1",
            "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
            "libraries": [
                { "name": "net.fabricmc:fabric-loader:0.14.21", "url": "https://maven.fabricmc.net/" },
                { "name": "net.fabricmc:intermediary:1.20.1", "url": "https://maven.fabricmc.net/" }
            ]
        }"#;

        let profiles_dir = meta_dir.join("loader-profiles");
        std::fs::create_dir_all(&profiles_dir).unwrap();
        let profile_path = profiles_dir.join("fabric-1.20.1-0.14.21.json");
        std::fs::write(&profile_path, upstream_bytes).unwrap();

        let saved = std::fs::read(&profile_path).unwrap();
        assert_eq!(saved, upstream_bytes.to_vec());

        let parsed: crate::launch_profile::model::LaunchProfile =
            serde_json::from_slice(&saved).unwrap();
        assert_eq!(parsed.id, "fabric-loader-0.14.21-1.20.1");
        assert_eq!(
            parsed.main_class.as_deref(),
            Some("net.fabricmc.loader.impl.launch.knot.KnotClient")
        );
        assert!(parsed.inherits_from.is_none()); // launch flow's implicit fallback handles it
        assert!(parsed.arguments.is_none());
        assert_eq!(parsed.libraries.len(), 2);
        assert_eq!(
            parsed.libraries[0].url.as_deref(),
            Some("https://maven.fabricmc.net/")
        );
    }
}
