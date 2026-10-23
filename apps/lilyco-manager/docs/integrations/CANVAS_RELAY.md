# Lilyco Canvas + API Relay integration — 2026-10-23

## Validated components
- Upstream [basketikun/infinite-canvas](https://github.com/basketikun/infinite-canvas), MIT. Canvas is a frontend authoring client, **not** an API reseller backend. Its browser stores the configured API key and requests OpenAI-compatible endpoints directly. Canvas/asset state is browser-local unless user configures synchronization. It includes Canvas Agent MCP and optional canvas-proxy.
- [lilyco-42/uncensored-api-relay](https://github.com/lilyco-42/uncensored-api-relay): Axum/Tokio/Reqwest OpenAI Chat Completions, Anthropic Messages, `/v1/models`, SSE streaming; documented `/health` endpoint. Not proof of image/video generation support.
- [lilyco-42/studio-billing](https://github.com/lilyco-42/studio-billing): FastAPI product/account/order/entitlement service. Do not share its admin tokens with browsers.

## Recommended service boundaries
```
Browser: Infinite Canvas (licensed fork, isolated origin)
    -> Public authenticated OpenAI-compatible API Gateway
        -> server-side model router + per-user key lookup + usage accounting
            -> contract-compliant upstream models (text/image/video separately)
Lilyco Manager -> read-only service health + adapters (no admin token in UI)
Studio Billing -> entitlement verification and payments (server-to-server only)
```

## MVP contract and gates
1. Run Canvas upstream unchanged in an isolated test environment; verify its required image/video endpoints separately. Do not assume text-only relay implements image/video.
2. Add model catalog: capabilities text/image/video, provider, pricing units, availability, rate limits. Disable UI actions without an available matching capability.
3. Keep permanent provider secrets on a server. Issue scoped, expiring end-user keys for Canvas. Canvas stores its own end-user key in browser storage by design, so warn users and disallow admin keys.
4. API gateway enforces authenticated users, quotas, per-key budgets, concurrent-request limits, idempotency for job submissions, and content size limits. Credentials are never logged.
5. Save per-request audit without private prompts: request_id, user_id, model_id, tokens or generation units, provider cost, billed amount, status and latency.
6. Billing uses authoritative confirmed usage, not estimated client-side token counting. Payment/entitlement operations require server-side signatures and replay protection.
7. Test single-session token spend, 10-concurrent P95 latency, retry amplification, and canvas-state synchronization before opening registration.
8. Deploy behind authenticated HTTPS reverse proxy; health endpoints must not leak infrastructure details. No provider-account farming or bypass of upstream quotas/terms.

## Infrastructure choices
- Reuse uncensored-api-relay for its proven text streaming routes **only after integration testing**. Extend a separately deployed gateway for supported image/video async jobs rather than falsely advertising capability.
- Lilyco Manager stays local-only on 127.0.0.1. Management API must not be accessible from a public Canvas origin.
- SeaORM/SQLite are fine for the first local management state; for multi-instance public billing use a transactional database and durable queue with reconciliation.

## API acceptance tests
- GET /v1/models returns only capability-matched authorized models
- POST /v1/chat/completions streams SSE and reports verified usage
- Image/video requests fail explicitly if no compatible provider is connected
- Unauthorized key -> 401; depleted quota -> 429; disabled plan -> 403
- Duplicate billing webhook or job ID does not issue duplicate charges
- No cross-user canvas history leakage; no secrets in logs or browser builds

## Delivery order
M0: API compatibility matrix + deployment inventory, no live changes.
M1: localhost/preview Canvas -> private test key -> existing relay text route.
M2: billing/usage metering and concurrency guardrails.
M3: image/video adapters with callbacks, storage, repeat-request prevention.
M4: production domain and paid plans only after security/latency acceptance.
