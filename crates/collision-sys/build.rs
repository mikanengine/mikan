//! Compiles the C narrow-phase kernel with warnings-as-errors.
//!
//! `docs/ffi.md` requires the module's own sources to build with
//! warnings-as-errors and the build to be reproducible; the flags below are
//! the strictest set the three supported toolchains (cc, clang, cl.exe) accept.

fn main() {
    println!("cargo:rerun-if-changed=src/contacts.c");
    println!("cargo:rerun-if-changed=src/mikan_contacts.h");

    cc::Build::new()
        .file("src/contacts.c")
        .include("src")
        .warnings(true)
        .flag_if_supported("-Werror")
        .flag_if_supported("/WX")
        .flag_if_supported("-std=c99")
        .flag_if_supported("-pedantic")
        .compile("mikan_contacts");
}
