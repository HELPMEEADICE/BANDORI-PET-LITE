fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/bandori.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../assets/icons/bandori.ico")
            .set("FileDescription", "Bandori 桌宠 LITE")
            .set("ProductName", "BANDORI-PET-LITE")
            .compile()
            .expect("compile Windows application icon");
    }
}
