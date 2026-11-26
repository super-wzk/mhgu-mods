fn main() {
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("arm")
        || std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("switch")
    {
        return;
    }
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg=-T{dir}/link/switch32.ld");
    println!("cargo:rustc-link-arg=--build-id=sha1");
    println!("cargo:rerun-if-changed=link/switch32.ld");
}
