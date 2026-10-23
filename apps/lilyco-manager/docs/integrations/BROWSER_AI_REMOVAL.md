# Client-side AI background removal (experimental)

Date: 2026-10-23

## User choice and running costs
The Commerce Studio explicitly asks users to click **AI background removal**; nothing downloads at page open. The selected PNG/JPEG/WebP is resized locally to a maximum of 1024px before being sent to a dedicated browser Web Worker as an in-memory ArrayBuffer. The Worker is a client-side thread, NOT a backend service.

The Worker lazily imports the pinned @huggingface/transformers **3.8.1** browser runtime from jsDelivr after opt-in. Runtime WASM assets and model files are fetched by the user browser directly from vendor/CDN origins, never from a Lilyco GPU server. The browser may reuse cached model weights. JS/CDN download latency and availability depend on the user's network; vendor endpoints see network metadata/IP, not the uploaded image pixel data. Model fetches must use GET, never prompt/image uploads.

**Recommended models:**
- People / clothing on models: `Xenova/modnet`, Apache-2.0. The quantized CPU model is approximately 6.6 MB, GPU model approximately 13 MB; not intended for product-only images.
- General SKU subjects: `onnx-community/ormbg-ONNX`, Apache-2.0. Quantized ONNX model approximately 44 MB; higher memory use. Stick to WASM CPU until MaxPool WebGPU interoperability issue is cleared.

Images and the output PNG remain on the customer's device. A client may cancel by terminating the Worker. Do not silently fallback to server inference. Models do not guarantee perfect outlines, physical accuracy or preservation of trademarks; preview and correction should be visible to the user.

## Architecture
- Default/offline: local Canvas2D, existing embedded wasm-pixel-kernels and optional WebGPU pixel operations.
- Optional AI: `background.worker.ts` compiled inline to a lightweight Worker, with **runtime fetched lazily**, client-cached ONNX and local inference.
- Test gates: frontend typecheck, production single-file bundle under 1 MB, Playwright worker mock E2E tests (no upload, no runtime download at app startup), real model inference optional/manual network test.
- Performance: one AI image per manual action; no automatic 50-image batch inference or background resource use. Main thread only resizes, previews and coordinates work. Worker performs model compute.

## Security/monetization
- Do not embed customer or provider API keys in worker, frontend build or GitHub repository.
- Browser uses pinned CDN package version, but pinning a CDN path is not content-integrity verification. Add reviewed SHA/SRI validation or controlled static asset signing before declaring the inference runtime production-hardened.
- Keep copyrights/license notices for models and runtime, and require review when bumping versions.
- No unlimited resale of vendor-hosted inference quotas. Business model focuses on paid local SKU workflow and support, not pooled student accounts.
- No calls to `lain42.top` for inference.

## Known limitations
- CDN model and runtime download needed on first use; browser storage can be cleared. Offline use of AI mode is conditional on browser cache, while basic image tools remain offline.
- WebGPU model load may fail and revert to WASM CPU. True WASM GPU compute does not exist: WebGPU uses GPU, WebAssembly uses CPU.
- This is an experimental feature until a live model run is independently verified, and output quality tested on product photos.
