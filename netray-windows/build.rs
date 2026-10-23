fn main() {
    if std::env::var_os("CARGO_CFG_TARGET_OS").is_some_and(|os| os == "windows")
        && std::env::var_os("PROFILE").is_some_and(|profile| profile == "release")
    {
        println!("cargo:rustc-link-arg-bin=netray-windows=/MANIFESTUAC:level='requireAdministrator'");
    }
}
