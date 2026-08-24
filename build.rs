// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=java/RmclShim.java");

    let out_dir = env::var("OUT_DIR").unwrap();
    let out = Path::new(&out_dir);

    // compile RmclShim.java into a jar that gets embedded via include_bytes!
    let status = Command::new("javac")
        .arg("-source")
        .arg("8")
        .arg("-target")
        .arg("8")
        .arg("-d")
        .arg(out.to_str().unwrap())
        .arg("java/RmclShim.java")
        .status()
        .expect("Failed to run javac - is a JDK installed?");

    assert!(status.success(), "javac failed to compile RmclShim.java");

    // package into a jar
    let jar_path = out.join("rmcl-shim.jar");
    let status = Command::new("jar")
        .arg("cfe")
        .arg(jar_path.to_str().unwrap())
        .arg("RmclShim")
        .arg("-C")
        .arg(out.to_str().unwrap())
        .arg("RmclShim.class")
        .status()
        .expect("Failed to run jar - is a JDK installed?");

    assert!(status.success(), "jar failed to create rmcl-shim.jar");
}
