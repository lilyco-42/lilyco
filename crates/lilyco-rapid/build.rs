fn main() {
    for path in [
        "../rapid-core",
        "../rapid-server",
        "web/src",
        "ops/ui",
        "../lilyco-no_std/src",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
}
