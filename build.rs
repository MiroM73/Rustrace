//! Windows: embeds the program icon (drawn by `src/icon.rs`) and the version information into
//! the executable, so Explorer, the taskbar and associated .log files show the RusTrace icon.

#[path = "src/icon.rs"]
#[allow(dead_code)]
mod icon;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/icon.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("rustrace.ico");
    std::fs::write(&out, icon::ico(&[16, 20, 24, 32, 40, 48, 64, 256])).unwrap();
    let mut res = winresource::WindowsResource::new();
    res.set_icon(out.to_str().unwrap());
    res.set("FileDescription", "RusTrace log viewer");
    res.set("ProductName", "RusTrace");
    res.compile().expect("compiling the Windows resources (rc.exe from the Windows SDK)");
}
