# Salience / C→WASM adapter protocol for Lilyco Commerce Studio

> **Project discovery:** The user identified a project named Salience that converts or ports C source into WebAssembly. Its source repository and ABI were not located in lilyco-42, so do not claim it is integrated. This document defines the portable ABI it should target.

## Runtime selection
1. Use browser WebGPU when available and the operation is implemented/tested as a GPU shader.
2. Use a **verified, locally embedded WebAssembly module** (WebAssembly CPU execution).
3. Otherwise use a browser-native Canvas2D / JavaScript CPU fallback.
4. Never silently send images, tensors, source files, or prompts to a Lilyco server.

Single-threaded WASM works without special cross-origin isolation. WASM pthreads and SharedArrayBuffer require the correct COOP / COEP configuration and must ship a single-thread fallback. WebGPU is independent of multithreaded WASM support.

## Stable ABI proposal (wasm32)
Export functions matching the proven lilyco-42/wasm-pixel-kernels contract:

```c
#include <stdint.h>
uint32_t alloc(uint32_t nbytes);
void dealloc(uint32_t ptr, uint32_t nbytes);
int32_t kernel_count(void);
uint32_t kernel_names(uint32_t ptr, uint32_t capacity); // NUL-separated names
int32_t process(int32_t id, uint32_t rgba_ptr, uint32_t rgba_len,
                uint32_t width, uint32_t height,
                uint32_t params_f32_ptr, uint32_t params_count);
```

In C compiled for wasm32, exported pointers in the ABI are 32-bit offsets into the module's linear memory. The runtime must expose `memory`. The caller allocates input and f32 parameter buffers in the module, checks bounds and return code, reads the result, and frees both buffers even when processing fails.

For Salience-produced modules: require correct exports and a reviewed, stable ABI version. No automatic network fetch from unknown third parties or `eval()` of manifests. User-supplied C source is never compiled on a Lilyco production server.

## Verified first adapter
- Module origin: https://github.com/lilyco-42/wasm-pixel-kernels
- Pinned source commit: `94285d78cec5edb0e2f671b0bfe5632a4d05df10`
- Existing source CI confirms 65 numeric-reference-verified kernels at this commit (not all 485 catalogued).
- Lilyco CI separately builds this source to `wasm32-unknown-unknown`, embeds bytes into its single-file frontend at build time, and browser-tests `exposure`.
- This module is implemented in Rust, not C. It demonstrates the ABI that Salience's C code can target later.

## Resource / trust controls
- Input: PNG/JPEG/WebP <= 25MiB; <= 50 images/batch; reject oversized decoded images. Keep pixels / allocations bounded.
- WASM runtime: isolated from filesystem/network by default (no WASI host imports). The JS host must not provide IO imports.
- A malicious/infinite-loop kernel can still freeze the main thread. Next milestone is a Web Worker with watchdog termination, per-job memory and time budgets; do not auto-load unreviewed WASM.
- Runtime benchmark should measure 10/50-image batch latency, memory, download size, WASM warmup and mobile fallback against Canvas2D before choosing default accelerators.
- Do not suggest every C/C++ library can be mechanically translated: OS APIs, dynamic libraries, threads, GPU drivers and proprietary dependencies require explicit porting/licensing.

## Release constraints
All compilation/test is on GitHub Actions. No build on the user's local PC or Lilyco production servers. Keep independent source pin and update it only in a reviewed PR with numeric regression tests and source-license verification.
