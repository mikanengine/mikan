/*
 * The vendored miniaudio build configuration, shared by the two
 * translation units that include `miniaudio.h`: `audio_backend.c` (which
 * declares the device API) and `miniaudio_impl.c` (which defines it).
 * Keeping the define set in one file means a declaration and its
 * definition can never disagree about which parts of the library exist.
 *
 * Decoding, encoding, the resource manager, the node graph, and the
 * waveform generators are compiled out: Mikan decodes WAV in Rust and
 * mixes in its own C++ runtime, so it needs only the device layer. The
 * null backend is compiled out too: a machine with no audio hardware
 * must fail the open and report "no device" so the caller can choose the
 * offline backend explicitly, never silently mix into a discarding
 * device. (The backend additionally rejects a null backend at runtime,
 * in case a future miniaudio version ever selects one anyway.)
 */

#ifndef MIKAN_MINIAUDIO_CONFIG_H
#define MIKAN_MINIAUDIO_CONFIG_H

#define MA_NO_DECODING
#define MA_NO_ENCODING
#define MA_NO_RESOURCE_MANAGER
#define MA_NO_NODE_GRAPH
#define MA_NO_GENERATION
#define MA_NO_NULL

#endif /* MIKAN_MINIAUDIO_CONFIG_H */
