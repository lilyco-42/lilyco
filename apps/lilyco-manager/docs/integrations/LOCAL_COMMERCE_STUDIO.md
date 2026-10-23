# Local-first Lyco Commerce Studio

## Scope and status (2026-10-23)
The Lilyco Manager Vue UI includes a **working local browser image preparation pipeline**:
- Multiple image import (max 50, up to 25 MiB each), alpha-bound centering, white/gray/transparent 800–2000px SKU exports, PNG/JPEG/WebP and an uncompressed ZIP with a delivery manifest.
- On-device WebGPU compute shader performs exposure adjustment when supported. CPU image pixels fallback is available; WebGPU is not used for AI masking.
- No image uploads, cloud inference, or Lilyco server GPU are involved in this workflow.
- Project discovery catalog contains curated own and third-party GitHub URLs; contributors submit via GitHub Issue, reviewed before bundling. Discovery NEVER grants execution rights.

## Complementary tools
- rembg-ui / Rembg Studio: client-side / local Python ONNX model background removal, MobileSAM and local batch processor. Users first produce transparent PNG there and import it here. No request is sent to a remote service by the commerce image processing UI.
- basketikun/infinite-canvas: independent browser canvas and local Canvas Agent/MCP, can connect to user-supplied OpenAI-compatible service. Keys must never be embedded in public builds.
- Ark0N/Codeman: local AI coding agent console; potential fit only. 'Codema' spoken by the user may refer to a different product; do not assume an integration.
- lilyco CLI/MCP, lyco-router and lyco-ai-toolkit: project catalog entries. Later tool execution requires explicit, reviewed adapters with permission boundaries.

## Performance and client acceleration
- HTML Canvas2D and ImageBitmap perform core JPEG/PNG/WebP editing locally, including fallback.
- WebGPU compute runs real exposure color-channel processing in the user's browser; the UI detects GPU adapter availability.
- Real on-device background removal is planned via ONNX Runtime Web (WebGPU provider with WASM fallback) after shipping a versioned, commercially permitted model and verifying quality / download size / browser memory.
- The browser and GPU handle end-user workloads; server is NOT a fallback and has zero access to their images.
- Future service worker/PWA must use versioned model caches and explicit storage controls; distinguish static frontend CDN bandwidth from GPU compute.

## Project publication and updates
- Reviewed catalog is `apps/lilyco-manager/catalog.json`; `scripts/validate-catalog.mjs` checks schema, publisher, HTTPS github owner/repo and unique IDs. No arbitrary download or install endpoints are included.
- Third parties submit via `.github/ISSUE_TEMPLATE/eco-project.yml`, maintainers manually verify license, provenance, security and claims.
- Upstream release watcher creates a tracking issue when a new Infinite Canvas release appears. Never merge upstream without review, data migration plan and local Canvas Agent compatibility tests.
- Bundled client catalog is an immutable release snapshot; updating it requires CI and redistribution of the EXE. A future signed catalog feed can supply faster updates.

## Business model
1. Free local client image editor, BYOK and open source integration guides.
2. Paid SKU templates, batch workflows, professional delivery reports, managed update packs and commercial support.
3. Optional cloud sync/storage paid explicitly with user consent; users' images stay local by default.
4. Optional third-party commercial image/video model plans only with explicit authorization and a measured margin. Never rely on pooled educational quotas or undocumented resale permissions.

## Invariants
Never upload local images without consent. Do not claim browser WebGPU exposure == full AI inference. Never auto-run third-party repos. Do not use public Lilyco GPU servers for user jobs. Preserve attribution and upstream licenses. No plaintext provider admin tokens in the browser or catalog.

## Acceptance
- [x] Vue TypeScript typecheck and local Vite build
- [x] Catalog 7 entries validated
- [x] Browser loads local studio, sees WebGPU support and control widgets; no JS errors
- [x] Real Chrome export test: PNG input -> canvas rendering -> ZIP download -> image and delivery manifest assertion; verifies zero outbound upload requests
- [ ] Windows/Linux GitHub Actions and packaged EXE verified for this commit
- [ ] Native Rembg Studio connection requires a local reviewed adapter
- [ ] ONNX WebGPU/WASM image segmentation not yet implemented

