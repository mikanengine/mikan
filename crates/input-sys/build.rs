//! Compiles the input module's native sources with warnings-as-errors.
//!
//! `docs/ffi.md` requires the module's own sources to build with
//! warnings-as-errors and to stay exception- and RTTI-free for C++. The C and
//! C++ sources are compiled separately — the backend stays C (compiled by the
//! C driver) and the runtime stays C++ (compiled by the C++ driver, which is
//! also what links the platform's C++ runtime) — for the same reason the scene
//! module separates them: passing C++-only flags such as `-fno-exceptions` to
//! a C file, or compiling a `.c` file in C++ mode, is a deprecation warning on
//! Apple clang that `-Werror` would reject.

fn main() {
    for file in ["src/mikan_input.h", "src/input_backend.c"] {
        println!("cargo:rerun-if-changed={file}");
    }
    for file in ["src/mikan_input.h", "src/input_runtime.cpp"] {
        println!("cargo:rerun-if-changed={file}");
    }

    // The C99 device backend: normalization, device state, the event buffer.
    cc::Build::new()
        .file("src/input_backend.c")
        .include("src")
        .warnings(true)
        .flag_if_supported("-Werror")
        .flag_if_supported("/WX")
        .flag_if_supported("-std=c99")
        .flag_if_supported("-pedantic")
        .compile("mikan_input_backend");

    // The C++ action runtime: bindings, contexts, action state machines.
    cc::Build::new()
        .cpp(true)
        .file("src/input_runtime.cpp")
        .include("src")
        .warnings(true)
        .flag_if_supported("-Werror")
        .flag_if_supported("/WX")
        .flag_if_supported("-pedantic")
        .flag_if_supported("-fno-exceptions")
        .flag_if_supported("-fno-rtti")
        .flag_if_supported("/EHs-c-")
        .flag_if_supported("/GR-")
        .compile("mikan_input_runtime");
}
