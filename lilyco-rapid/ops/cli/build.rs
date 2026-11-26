fn main() {
    for path in [
        "../../core",
        "../../server",
        "../../web/src",
        "../../../lilyco-no_std/src",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
}
