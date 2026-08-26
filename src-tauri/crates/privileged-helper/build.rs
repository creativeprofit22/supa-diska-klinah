fn main() {
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resources = winresource::WindowsResource::new();
        resources.set_manifest_file("windows-app-manifest.xml");
        resources
            .compile()
            .expect("failed to compile the privileged helper manifest");
    }
}
