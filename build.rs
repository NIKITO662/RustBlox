fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "RustBlox");
        res.set("FileDescription", "RustBlox Launcher");
        res.set("CompanyName", "ItsNiki66");
        res.set("LegalCopyright", "Made by ItsNiki66. Not affiliated with Roblox Corporation.");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icon embedding skipped: {e}");
        }
    }
}
