fn main() {
    for path in ["templates", "web/src", "ops"] {
        println!("cargo:rerun-if-changed={path}");
    }
}
