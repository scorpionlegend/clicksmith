fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let icon = "packaging/windows/clicksmith.ico";
    println!("cargo:rerun-if-changed={icon}");

    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(icon)
        .set("FileDescription", "Clicksmith")
        .set("InternalName", "clicksmith")
        .set("OriginalFilename", "clicksmith.exe")
        .set("ProductName", "Clicksmith")
        .set("CompanyName", "Scorpion Legend");
    resource
        .compile()
        .expect("failed to embed Windows application resources");
}
