//! Netray Windows Build Script
//! Configures Windows application metadata and manifests.

fn main() {
    // Only link windows subsystem / manifests when targeting windows
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=resources/app.manifest");
    }
}
