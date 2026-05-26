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
// rust's stdlib doesn't expose this, so we read it where it's cheap and
// reliable: linux via /proc/sys/kernel/osrelease, other platforms return
// empty. when the host string is empty, version-gated rules don't match
// (conservative default in the rule evaluator) - which is fine because
// real-world profiles using os.version are vanishingly rare.
pub fn mojang_os_version() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(s) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
            return s.trim().to_string();
        }
    }
    String::new()
}
