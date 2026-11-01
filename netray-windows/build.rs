fn main() {
    if std::env::var_os("CARGO_CFG_TARGET_OS").is_some_and(|os| os == "windows") {
        println!("cargo:rerun-if-changed=resources/app.manifest");
        println!("cargo:rerun-if-changed=resources/netray.ico");
        println!("cargo:rerun-if-changed=resources/resource.rc");

        let rc_file = std::path::Path::new("resources/resource.rc");
        if rc_file.exists() {
            let out_dir = std::env::var("OUT_DIR").unwrap_or_default();
            let res_file = format!("{}/resource.res", out_dir);
            let rc_status = std::process::Command::new("rc.exe")
                .args(["/fo", &res_file, "resources/resource.rc"])
                .status()
                .or_else(|_| {
                    std::process::Command::new("windres")
                        .args(["-i", "resources/resource.rc", "-O", "coff", "-o", &res_file])
                        .status()
                });

            if let Ok(status) = rc_status {
                if status.success() {
                    println!("cargo:rustc-link-arg={res_file}");
                }
            }
        }
    }
}
