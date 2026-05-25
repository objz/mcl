// system-detection helpers shared by launch and install paths. mojang
// names some things differently from rust's std::env::consts (e.g. macOS
// is "osx" in mojang profile rules), so this module is the single source
// of truth for translating.

pub fn mojang_os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "osx",
        other => other,
    }
}

pub fn mojang_arch_name() -> &'static str {
    match std::env::consts::ARCH {
        "x86" => "x86",
        "x86_64" => "x86_64",
        "aarch64" => "arm64",
        other => other,
    }
}

// the host OS version string. mojang rules occasionally constrain natives
// selection on os.version with a regex (e.g. macOS 10.x-only natives).
// rust's stdlib doesn't expose this directly, so we synthesise something
// useful per-platform. on linux we read the kernel version; on macOS the
// product version is more useful but reading it requires shelling out so
// we fall back to the kernel version. on windows we use a similar
// approach. real-world profiles using os.version are rare; if this helper
// returns an empty string the rule evaluator treats version constraints
// as non-matching (defensive default).
pub fn mojang_os_version() -> String {
    #[cfg(unix)]
    {
        use std::process::Command;
        if let Ok(out) = Command::new("uname").arg("-r").output()
            && out.status.success()
        {
            return String::from_utf8_lossy(&out.stdout).trim().to_string();
        }
    }
    #[cfg(windows)]
    {
        // crude - sufficient for the rare profile that needs windows version
        // gating. real launchers use GetVersionEx via winapi; we can add
        // that later if needed.
        return std::env::var("OS").unwrap_or_default();
    }
    String::new()
}
