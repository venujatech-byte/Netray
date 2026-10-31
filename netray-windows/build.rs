fn main() {
    if std::env::var_os("CARGO_CFG_TARGET_OS").is_some_and(|os| os == "windows") {
        println!("cargo:rerun-if-changed=resources/app.manifest");
    }
}
