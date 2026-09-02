// Emit the linker script only for the MCU target, resolved from the crate
// directory so the command works from any working directory.
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if target == "riscv32imac-unknown-none-elf" {
        let dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
        println!("cargo:rustc-link-arg=-T{dir}/link.x");
    }
}
