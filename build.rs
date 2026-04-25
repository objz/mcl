use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=java/MclShim.java");

    let out_dir = env::var("OUT_DIR").unwrap();
    let out = Path::new(&out_dir);

    // compile MclShim.java into a jar that gets embedded via include_bytes!
    let status = Command::new("javac")
        .arg("-source")
        .arg("8")
        .arg("-target")
        .arg("8")
        .arg("-d")
        .arg(out.to_str().unwrap())
        .arg("java/MclShim.java")
        .status()
        .expect("Failed to run javac - is a JDK installed?");

    assert!(status.success(), "javac failed to compile MclShim.java");

    // package into a jar
    let jar_path = out.join("mcl-shim.jar");
    let status = Command::new("jar")
        .arg("cfe")
        .arg(jar_path.to_str().unwrap())
        .arg("MclShim")
        .arg("-C")
        .arg(out.to_str().unwrap())
        .arg("MclShim.class")
        .status()
        .expect("Failed to run jar - is a JDK installed?");

    assert!(status.success(), "jar failed to create mcl-shim.jar");
}
