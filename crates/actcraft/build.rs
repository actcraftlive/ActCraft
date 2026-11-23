// Puts the ActCraft icon into the Windows executable, so Explorer, shortcuts and a pinned taskbar
// button show it (the window icon is set at run time). When no resource compiler is available the
// program is built without it, with a warning.
fn main() {
    println!("cargo:rerun-if-changed=actcraft.rc");
    println!("cargo:rerun-if-changed=../../assets/logo/actcraft.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && let Err(e) = embed_resource::compile("actcraft.rc", embed_resource::NONE).manifest_optional()
    {
        println!("cargo:warning=program icon not embedded: {e}");
    }
}
