// classpath and jvm patches for legacy forge on java 9+.
// lwjgl3ify ships a patched launchwrapper and retrofuturabootstrap that
// replace the old URLClassLoader-based launcher with one that works on
// modern java. this module handles extracting those patches, building
// the right --add-opens flags, and swapping out the broken log4j jars.

use std::path::{Path, PathBuf};

const LOG4J_FIXED_BASE: &str = "https://files.prismlauncher.org/maven/org/apache/logging/log4j";

pub struct LwjglifyPatches {
    pub jvm_args: Vec<String>,
    pub main_class: String,
    // extra args inserted before game args (used by the shim to pass
    // the real main class name)
    pub extra_args: Vec<String>,
}

// checks if lwjgl3ify is present in the mods folder, and if so:
// 1. extracts forge patches from the lwjgl3ify jar
// 2. prepends them to the classpath so they shadow vanilla launchwrapper
// 3. parses --add-opens from the patches manifest
// 4. replaces log4j 2.0-beta9 with prism's patched builds
// 5. returns the jvm args and overridden main class
pub async fn apply(
    minecraft_dir: &Path,
    lib_dir: &Path,
    classpath: &mut Vec<PathBuf>,
) -> Option<LwjglifyPatches> {
    let mods_dir = minecraft_dir.join("mods");
    let lwjgl3ify_jar = find_lwjgl3ify_jar(&mods_dir)?;

    // rfb only scans jars for plugin metadata. keeping the extracted
    // forgePatches payload as a zip makes it skip rfb-asm-safety and
    // rfb-modern-java, which GTNH needs on modern Java.
    let patches_dest = minecraft_dir.join(".forge-patches.jar");

    if let Err(e) = extract_forge_patches(&lwjgl3ify_jar, &patches_dest) {
        tracing::warn!("Failed to extract lwjgl3ify forge patches: {e}");
        return None;
    }

    // prepend so patched classes shadow vanilla launchwrapper
    classpath.insert(0, patches_dest.clone());

    let mut jvm_args = parse_add_opens(&patches_dest).unwrap_or_default();

    // RFB requires its own classloader to be the system classloader, and
    // its Main class handles bootstrapping into launchwrapper.
    // the other flags match lwjgl3ify's java9args.txt.
    jvm_args.extend([
        "-Djava.system.class.loader=com.gtnewhorizons.retrofuturabootstrap.RfbSystemClassLoader"
            .to_string(),
        "-Dfile.encoding=UTF-8".to_string(),
    ]);

    // forge patches replace launchwrapper, asm, and old lwjgl2.
    // lwjgl3ify redirects lwjgl2 calls to lwjgl3 at runtime, so lwjgl3
    // must be on the classpath (matching what prism does).
    strip_replaced_libs(classpath);
    add_lwjgl3(lib_dir, classpath);

    replace_log4j_fixed(lib_dir, classpath).await;

    // on java 24+, SecurityManager.getClassContext() was reimplemented to use
    // StackWalker. log4j 2.0-beta9's ThrowableProxy calls getClassContext() to
    // resolve stack frames, but StackWalker triggers class loading through
    // LaunchClassLoader, which debug-logs failures, which creates another
    // ThrowableProxy... infinite recursion. we break the loop by providing a
    // log4j config that sets the root level to INFO, so the debug() call in
    // LaunchClassLoader is a no-op and never creates a ThrowableProxy.
    write_log4j_config(minecraft_dir, &mut jvm_args);

    // RfbSystemClassLoader discovers plugins differently depending on whether
    // the main class is loaded by the JVM directly or through the system
    // classloader's loadClass(). direct invocation misses the rfb-asm-safety
    // and rfb-modern-java plugins from forgePatches, causing
    // ClassCircularityErrors. we use a tiny shim jar that loads the real main
    // class through ClassLoader.getSystemClassLoader().loadClass(), matching
    // how prism's EntryPoint does it.
    let shim_path = deploy_shim(minecraft_dir);
    classpath.insert(0, shim_path);

    Some(LwjglifyPatches {
        jvm_args,
        main_class: "MclShim".to_string(),
        extra_args: vec!["com.gtnewhorizons.retrofuturabootstrap.Main".to_string()],
    })
}

const SHIM_JAR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mcl-shim.jar"));

fn deploy_shim(minecraft_dir: &Path) -> PathBuf {
    let dest = minecraft_dir.join(".mcl-shim.jar");
    if let Err(e) = std::fs::write(&dest, SHIM_JAR) {
        tracing::warn!("Failed to write mcl-shim.jar: {e}");
    }
    dest
}

fn find_lwjgl3ify_jar(mods_dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(mods_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("lwjgl3ify") && name.ends_with(".jar") {
            return Some(entry.path());
        }
    }
    None
}

fn extract_forge_patches(lwjgl3ify_jar: &Path, dest: &Path) -> Result<(), std::io::Error> {
    use std::io::Read;

    let file = std::fs::File::open(lwjgl3ify_jar)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut entry = archive
        .by_name("me/eigenraven/lwjgl3ify/relauncher/forgePatches.zip")
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, e))?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf)?;
    std::fs::write(dest, &buf)
}

fn parse_add_opens(patches_archive: &Path) -> Option<Vec<String>> {
    use std::io::Read;

    let file = std::fs::File::open(patches_archive).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut entry = archive.by_name("META-INF/MANIFEST.MF").ok()?;
    let mut manifest = String::new();
    entry.read_to_string(&mut manifest).ok()?;

    // manifest continuation lines start with a single space
    let manifest = manifest.replace("\r\n ", "").replace("\n ", "");

    let mut args = Vec::new();
    for line in manifest.lines() {
        if let Some(value) = line.strip_prefix("Add-Opens: ") {
            for module_package in value.split_whitespace() {
                args.push("--add-opens".to_string());
                args.push(format!("{module_package}=ALL-UNNAMED"));
            }
        }
    }

    Some(args)
}

// strips libraries from the classpath that are replaced by forge patches
// or not needed with lwjgl3ify. matches what prism's component system does:
// no old lwjgl2, no vanilla launchwrapper/asm, no extra vanilla-only libs.
fn strip_replaced_libs(classpath: &mut Vec<PathBuf>) {
    let replaced = [
        "launchwrapper-",
        "asm-all-",
        "lwjgl-2.",
        "lwjgl_util-",
        "commons-compress-",
        "commons-io-",
        "guava-15.",
    ];

    classpath.retain(|entry| {
        let name = entry
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        let dominated = replaced.iter().any(|prefix| name.starts_with(prefix));
        if dominated {
            tracing::info!("Stripping {name} from classpath (replaced by forge-patches)");
        }
        !dominated
    });
}

// adds lwjgl 3.3.3 to the classpath. lwjgl3ify redirects old lwjgl2 calls
// to lwjgl3 at runtime, but the lwjgl3 jars need to be on the system
// classpath for this to work. prism includes these via its org.lwjgl3
// component; we add them from the meta library cache.
fn add_lwjgl3(lib_dir: &Path, classpath: &mut Vec<PathBuf>) {
    let lwjgl3_modules = [
        "lwjgl",
        "lwjgl-freetype",
        "lwjgl-glfw",
        "lwjgl-jemalloc",
        "lwjgl-openal",
        "lwjgl-opengl",
        "lwjgl-stb",
        "lwjgl-tinyfd",
    ];

    let os_classifier = match std::env::consts::OS {
        "macos" => "natives-macos",
        "windows" => "natives-windows",
        _ => "natives-linux",
    };

    // insert lwjgl3 jars right after forge patches (position 1+)
    let mut insert_pos = 1.min(classpath.len());
    for module in &lwjgl3_modules {
        let base = if *module == "lwjgl" {
            lib_dir.join("org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3.jar")
        } else {
            lib_dir.join(format!("org/lwjgl/{0}/3.3.3/{0}-3.3.3.jar", module))
        };
        if base.exists() {
            classpath.insert(insert_pos, base);
            insert_pos += 1;
        }

        // natives
        let natives = if *module == "lwjgl" {
            lib_dir.join(format!(
                "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-{os_classifier}.jar"
            ))
        } else {
            lib_dir.join(format!(
                "org/lwjgl/{0}/3.3.3/{0}-3.3.3-{1}.jar",
                module, os_classifier
            ))
        };
        if natives.exists() {
            classpath.insert(insert_pos, natives);
            insert_pos += 1;
        }
    }

    tracing::info!("Added {} LWJGL 3.3.3 jars to classpath", insert_pos - 1);
}

// writes a log4j2.xml config that sets the root logger to INFO. this
// prevents LaunchClassLoader's debug() call from ever firing, which
// avoids the ThrowableProxy -> SecurityManager -> StackWalker infinite
// recursion on java 24+. the config is written to .minecraft/ and
// pointed to via -Dlog4j.configurationFile.
fn write_log4j_config(minecraft_dir: &Path, jvm_args: &mut Vec<String>) {
    let config_path = minecraft_dir.join(".mcl-log4j2.xml");
    let config = r#"<?xml version="1.0" encoding="UTF-8"?>
<Configuration status="WARN">
    <Appenders>
        <Console name="SysOut" target="SYSTEM_OUT">
            <PatternLayout pattern="[%d{HH:mm:ss}] [%t/%level] [%logger]: %msg%n"/>
        </Console>
        <Queue name="ServerGuiConsole">
            <PatternLayout pattern="[%d{HH:mm:ss} %level]: %msg%n"/>
        </Queue>
        <RollingRandomAccessFile name="File" fileName="logs/latest.log"
                filePattern="logs/%d{yyyy-MM-dd}-%i.log.gz">
            <PatternLayout pattern="[%d{HH:mm:ss}] [%t/%level]: %msg%n"/>
            <Policies>
                <TimeBasedTriggeringPolicy/>
                <OnStartupTriggeringPolicy/>
            </Policies>
        </RollingRandomAccessFile>
    </Appenders>
    <Loggers>
        <Root level="info">
            <AppenderRef ref="SysOut"/>
            <AppenderRef ref="File"/>
        </Root>
    </Loggers>
</Configuration>
"#;

    if let Err(e) = std::fs::write(&config_path, config) {
        tracing::warn!("Failed to write log4j2 config: {e}");
        return;
    }

    jvm_args.push(format!(
        "-Dlog4j.configurationFile={}",
        config_path.display()
    ));
}

// replaces log4j-api and log4j-core 2.0-beta9 in the classpath with
// prism's patched "-fixed" builds. the vanilla 2.0-beta9 has a bug
// where ThrowableProxy calls SecurityManager.getClassContext() which
// on java 24+ uses StackWalker internally, triggering class loading
// through LaunchClassLoader, which triggers more logging, infinite
// recursion, stack overflow. the fixed builds patch this out.
async fn replace_log4j_fixed(lib_dir: &Path, classpath: &mut [PathBuf]) {
    let replacements = [
        (
            "log4j-api-2.0-beta9.jar",
            "org/apache/logging/log4j/log4j-api/2.0-beta9-fixed/log4j-api-2.0-beta9-fixed.jar",
            format!("{LOG4J_FIXED_BASE}/log4j-api/2.0-beta9-fixed/log4j-api-2.0-beta9-fixed.jar"),
        ),
        (
            "log4j-core-2.0-beta9.jar",
            "org/apache/logging/log4j/log4j-core/2.0-beta9-fixed/log4j-core-2.0-beta9-fixed.jar",
            format!("{LOG4J_FIXED_BASE}/log4j-core/2.0-beta9-fixed/log4j-core-2.0-beta9-fixed.jar"),
        ),
    ];

    for (old_name, fixed_rel, url) in &replacements {
        let fixed_path = lib_dir.join(fixed_rel);

        if !fixed_path.exists() {
            tracing::info!("Downloading patched {old_name}...");
            if let Some(parent) = fixed_path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            let client = crate::net::HttpClient::new();
            if let Err(e) = crate::net::download_file(&client, url, &fixed_path, |_, _| {}).await {
                tracing::error!(
                    "Failed to download patched {old_name}: {e}, continuing with unpatched version"
                );
                continue;
            }
        }

        for entry in classpath.iter_mut() {
            if entry
                .file_name()
                .is_some_and(|n| n.to_string_lossy() == *old_name)
            {
                tracing::info!("Replacing {old_name} with patched version");
                *entry = fixed_path.clone();
            }
        }
    }
}
