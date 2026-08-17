use std::{env, fs, path::PathBuf};

const PACKAGE_NAME: &str = "dev.nenw.keeless.passkey";
const APPLICATION_ID: &str = "PasskeyProvider";
const DEVELOPMENT_PUBLISHER: &str = "CN=Keeless Development";

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }

    println!("cargo:rerun-if-env-changed=MSIX_PUBLISHER");

    let publisher = match env::var("PROFILE").as_deref() {
        Ok("debug") => DEVELOPMENT_PUBLISHER.to_owned(),
        Ok(_) => env::var("MSIX_PUBLISHER")
            .expect("MSIX_PUBLISHER must be set for release Windows builds"),
        Err(_) => panic!("Cargo did not provide PROFILE"),
    };
    let manifest_path =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo did not provide OUT_DIR"))
            .join("keeless-passkey-windows.manifest");

    // Direct launches need this embedded identity to join the external-location package.
    let manifest = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<assembly manifestVersion="1.0" xmlns="urn:schemas-microsoft-com:asm.v1">
  <assemblyIdentity
    type="win32"
    name="{PACKAGE_NAME}"
    version="1.0.0.0"
    processorArchitecture="*" />
  <msix
    xmlns="urn:schemas-microsoft-com:msix.v1"
    publisher="{}"
    packageName="{PACKAGE_NAME}"
    applicationId="{APPLICATION_ID}" />
</assembly>
"#,
        escape_xml_attribute(&publisher),
    );

    fs::write(&manifest_path, manifest).expect("failed to create MSIX application manifest");

    println!("cargo:rustc-link-arg-bin=keeless-passkey-windows=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bin=keeless-passkey-windows=/MANIFESTINPUT:{}",
        manifest_path.display()
    );
}

fn escape_xml_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
