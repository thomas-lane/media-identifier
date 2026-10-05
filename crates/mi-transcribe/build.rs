//! Links clang's runtime library on macOS.
//!
//! whisper.cpp's Metal code checks the macOS version at run time (`@available`). When the code is
//! built for an older macOS than the SDK (release builds target macOS 11, see
//! `bundle.macOS.minimumSystemVersion`), clang turns each check into a call to
//! `__isPlatformVersionAtLeast`, which lives in `libclang_rt.osx.a`. rustc links with
//! `-nodefaultlibs`, so that library is not linked unless this script asks for it; without it
//! the release app fails to link.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let output = Command::new("xcrun")
        .args(["clang", "--print-runtime-dir"])
        .output()
        .or_else(|_| Command::new("clang").arg("--print-runtime-dir").output());
    match output {
        Ok(out) if out.status.success() => {
            let dir = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            if std::path::Path::new(&dir)
                .join("libclang_rt.osx.a")
                .is_file()
            {
                println!("cargo:rustc-link-search=native={dir}");
                println!("cargo:rustc-link-lib=static=clang_rt.osx");
            } else {
                println!(
                    "cargo:warning=libclang_rt.osx.a not found in {dir}; release builds may not link"
                );
            }
        }
        _ => println!(
            "cargo:warning=could not ask clang for its runtime folder; release builds may not link"
        ),
    }
}
