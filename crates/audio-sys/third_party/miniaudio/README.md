# Vendored miniaudio

- **Upstream:** https://github.com/mackron/miniaudio
- **Version:** 0.11.25 (released 2026-03-03), file `miniaudio.h`
- **SHA-256:** `ac7af4de748b7e26b777f37e01cee313a308a7296a3eb080e2906b320cc55c89`
  (re-check with `sha256sum miniaudio.h`)
- **License:** public domain (Unlicense) or MIT-0, at the user's choice; the
  full license text is at the end of `miniaudio.h`.

miniaudio is the C device backend for `crates/audio-sys`: it opens the
default output device on every supported platform (WASAPI/DirectSound/WinMM,
Core Audio, ALSA/PulseAudio/JACK, and so on), runs the real-time callback
thread, and converts the f32 mix Mikan produces into the hardware format.
Mikan does not use miniaudio's decoders (WAV decoding happens in
`mikan-assets`), its resource manager, its node graph, or its generators;
`src/mikan_miniaudio_config.h` compiles those parts out, and also compiles the
null backend out (`MA_NO_NULL`), so a machine with no audio hardware fails the
open and reports "no device" instead of silently mixing into a discarding
device.

The file is compiled by `build.rs` into `miniaudio_impl.c`'s static library
with warnings disabled; everything around it follows the warnings-as-errors
rule of `docs/ffi.md`.
