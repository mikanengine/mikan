//! Compiles the audio module's native sources.
//!
//! `docs/ffi.md` requires the module's own sources to build with
//! warnings-as-errors and to stay exception- and RTTI-free for C++. The
//! rules bind Mikan's code, not the vendored miniaudio implementation: it
//! is compiled by its own translation unit with warnings disabled and no
//! warnings-as-errors, exactly like a vendored library's own build system.
//!
//! The C and C++ sources are compiled separately — the backend stays C
//! (compiled by the C driver) and the runtime stays C++ (compiled by the
//! C++ driver, which is also what links the platform's C++ runtime) — for
//! the same reason the scene and input modules separate them: passing
//! C++-only flags such as `-fno-exceptions` to a C file is a deprecation
//! warning on Apple clang that `-Werror` would reject.

fn main() {
    for file in [
        "src/mikan_audio.h",
        "src/mikan_miniaudio_config.h",
        "src/audio_backend.c",
        "src/miniaudio_impl.c",
        "third_party/miniaudio/miniaudio.h",
    ] {
        println!("cargo:rerun-if-changed={file}");
    }
    for file in ["src/mikan_audio.h", "src/audio_runtime.cpp"] {
        println!("cargo:rerun-if-changed={file}");
    }

    // The vendored miniaudio implementation: upstream code, upstream
    // flags. It compiles the device layer only; see
    // src/mikan_miniaudio_config.h for the parts that are switched off.
    cc::Build::new()
        .file("src/miniaudio_impl.c")
        .include("src")
        .include("third_party/miniaudio")
        .warnings(false)
        .compile("mikan_miniaudio");

    // The C99 device backend: the miniaudio device wiring, the offline
    // device, the sample-format conversion, and the status flags.
    cc::Build::new()
        .file("src/audio_backend.c")
        .include("src")
        .include("third_party/miniaudio")
        .warnings(true)
        .flag_if_supported("-Werror")
        .flag_if_supported("/WX")
        .flag_if_supported("-std=c99")
        .flag_if_supported("-pedantic")
        .compile("mikan_audio_backend");

    // The C++ runtime: the sound registry, the voice pool, the command
    // queue, and the mixer.
    cc::Build::new()
        .cpp(true)
        .file("src/audio_runtime.cpp")
        .include("src")
        .warnings(true)
        .flag_if_supported("-Werror")
        .flag_if_supported("/WX")
        .flag_if_supported("-pedantic")
        .flag_if_supported("-fno-exceptions")
        .flag_if_supported("-fno-rtti")
        .flag_if_supported("/EHs-c-")
        .flag_if_supported("/GR-")
        .compile("mikan_audio_runtime");

    // miniaudio runtime-links the platform audio APIs (dlopen/LoadLibrary),
    // so on glibc the only extra symbol it needs is the dynamic loader's,
    // which older glibc keeps in libdl. Adding it is a no-op on glibc
    // 2.34+ and skipped everywhere else.
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target == "linux" && target_env == "gnu" {
        println!("cargo:rustc-link-lib=dylib=dl");
    }
}
