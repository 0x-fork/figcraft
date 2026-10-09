//! Windows: embed the icon and version info in figcraft.exe (no-op elsewhere).
fn main() {
    println!("cargo::rerun-if-changed=../../assets/app-icon/figcraft.ico");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        let mut res = winresource::WindowsResource::new();
        let ico = "../../assets/app-icon/figcraft.ico";
        if std::path::Path::new(ico).exists() {
            res.set_icon(ico);
        }
        res.set("ProductName", "FigCraft");
        res.set("FileDescription", "FigCraft");
        res.set("CompanyName", "ArtCraft");
        if let Err(e) = res.compile() {
            println!("cargo::warning=winresource: {e}");
        }
    }
    let sha = std::env::var("FIGCRAFT_BUILD_SHA").unwrap_or_default();
    println!("cargo::rustc-env=FIGCRAFT_BUILD_SHA={sha}");
    println!("cargo::rerun-if-env-changed=FIGCRAFT_BUILD_SHA");
}
