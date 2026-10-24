//! Build script: when building for Windows with the MinGW toolchain, compile
//! the program's icon and version information into the executable (so
//! Explorer, the taskbar and file properties show them), using MinGW's
//! `windres`. Other targets need nothing here.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-env-changed=WINDRES");
    let target = env::var("TARGET").unwrap_or_default();
    if !target.contains("windows-gnu") {
        return;
    }
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("set by cargo"));
    let icon = manifest_dir.join("assets").join("icon.ico");
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.1.0".to_owned());
    let numeric: Vec<u32> = version.split(['.', '-']).filter_map(|p| p.parse().ok()).take(3).collect();
    let (major, minor, patch) = (
        numeric.first().copied().unwrap_or(0),
        numeric.get(1).copied().unwrap_or(0),
        numeric.get(2).copied().unwrap_or(0),
    );
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "AnovaCraft"
      VALUE "ProductName", "AnovaCraft"
      VALUE "FileVersion", "{version}"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "AnovaCraft.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "/"),
    );
    let rc_path = out_dir.join("anovacraft.rc");
    std::fs::write(&rc_path, rc).expect("write the resource script");
    let object = out_dir.join("anovacraft-resources.o");
    let arch = if target.starts_with("aarch64") { "aarch64" } else { "x86_64" };
    let windres = env::var("WINDRES").unwrap_or_else(|_| format!("{arch}-w64-mingw32-windres"));
    let status = Command::new(&windres).arg(&rc_path).arg("-O").arg("coff").arg("-o").arg(&object).status();
    match status {
        Ok(s) if s.success() => println!("cargo:rustc-link-arg-bins={}", object.display()),
        _ => println!("cargo:warning=could not run {windres}; the executable will have no icon or version information"),
    }
}
