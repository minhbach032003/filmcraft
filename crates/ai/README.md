# filmcraft-ai

Layer 2 crate providing AI provider client abstractions, HTTP transport, pinned model catalog,
cost estimation, and background job coordination for FilmCraft.

## Features

- **[`AiTransport`]:** low-level HTTP transport trait; fully mockable via `FakeTransport` in unit
  tests. Real network calls are made only when the `net` feature is enabled.
- **[`AiProvider`]:** high-level abstraction for story generation, screenplay planning, and
  multimodal media creation.
- **[`ApiKey`]:** secure secret wrapper that guarantees redaction in `Debug`, `Display`, logs, and
  serialization.
- **[`CancellationToken`]:** cooperative cancellation token polled during background jobs.
- **Model Catalog:** statically verified metadata and pricing for Gemini 2.5 Flash, Gemini 2.5 Pro,
  Imagen 3, Veo 2, Gemini Flash TTS, and Kokoro 82M.

## Layering & WASM Rules

- Layer 2: does not depend on L3/L4/L5 crates (`render`, `engine`, `ui-egui`).
- Crucially: does **not** depend on `filmcraft-project`. Project mutation is handled via commands
  at L4 (`engine`).
- `net` is off by default: compiles cleanly to `wasm32-unknown-unknown` (`cargo xtask wasm`).
- All tests run offline with `FakeTransport`.
