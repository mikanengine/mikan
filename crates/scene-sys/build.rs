//! Compiles the scene module's native sources with warnings-as-errors.
//!
//! `docs/ffi.md` requires the module's own sources to build with
//! warnings-as-errors and to stay exception- and RTTI-free for C++. The C and
//! C++ sources are compiled separately: the kernel stays C (compiled by the C
//! driver) and the graph stays C++ (compiled by the C++ driver, which is also
//! what links the platform's C++ runtime). Passing C++-only flags such as
//! `-fno-exceptions` to a C file, or compiling a `.c` file in C++ mode, is a
//! deprecation warning on Apple clang that `-Werror` would reject, so the two
//! files must not share one `cc::Build`.

fn main() {
    for file in [
        "src/mikan_transform.h",
        "src/transform.c",
        "src/mikan_scene_graph.h",
        "src/scene_graph.cpp",
    ] {
        println!("cargo:rerun-if-changed={file}");
    }

    // The C99 transform kernel. It never allocates and never touches the C++
    // standard library; keeping it on the C driver is what makes it C.
    cc::Build::new()
        .file("src/transform.c")
        .include("src")
        .warnings(true)
        .flag_if_supported("-Werror")
        .flag_if_supported("/WX")
        .flag_if_supported("-std=c99")
        .flag_if_supported("-pedantic")
        // The transform kernel's bit-exactness against the Rust reference
        // depends on multiply/add never being fused into an FMA; some
        // toolchains contract by default in non-ISO modes (for example GCC
        // on aarch64), so contraction is disabled explicitly.
        .flag_if_supported("-ffp-contract=off")
        .compile("mikan_scene_transform");

    // The C++ scene graph. `cpp(true)` selects the C++ driver, which also
    // emits the platform C++ runtime link directive for the module.
    cc::Build::new()
        .cpp(true)
        .file("src/scene_graph.cpp")
        .include("src")
        .warnings(true)
        .flag_if_supported("-Werror")
        .flag_if_supported("/WX")
        .flag_if_supported("-pedantic")
        .flag_if_supported("-fno-exceptions")
        .flag_if_supported("-fno-rtti")
        .flag_if_supported("/EHs-c-")
        .flag_if_supported("/GR-")
        .compile("mikan_scene_graph");
}
