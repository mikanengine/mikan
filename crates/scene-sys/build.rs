//! Compiles the scene module's native sources with warnings-as-errors.
//!
//! `docs/ffi.md` requires the module's own sources to build with
//! warnings-as-errors, to stay exception- and RTTI-free for C++, and to be
//! reproducible. The flags below are the strictest set the three supported
//! toolchains (cc, clang, cl.exe) accept; the C and C++ files share one build
//! and therefore no `-std` flag is pinned — the toolchain defaults
//! (C11/C++14 or newer) are already at or above what the sources need.

fn main() {
    for file in [
        "src/mikan_transform.h",
        "src/transform.c",
        "src/mikan_scene_graph.h",
        "src/scene_graph.cpp",
    ] {
        println!("cargo:rerun-if-changed={file}");
    }

    cc::Build::new()
        // The module links as one library and the graph pulls in the C++
        // standard library (`std::vector`, `std::unordered_map`), so the
        // build goes through the C++ driver: `transform.c` remains C source
        // — it is C99-compatible C++ as well — and `cpp(true)` is what makes
        // cc link the platform's C++ runtime for the whole module.
        .cpp(true)
        .file("src/transform.c")
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
        // The transform kernel's bit-exactness against the Rust reference
        // depends on multiply/add never being fused into an FMA; some
        // toolchains contract by default in non-ISO modes (for example GCC
        // on aarch64), so contraction is disabled explicitly.
        .flag_if_supported("-ffp-contract=off")
        .compile("mikan_scene");
}
