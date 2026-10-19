# ADR 0002: AI Director architecture, schemas, and provider contracts

- **Status:** accepted (2026-10-18, approved for AI Director milestone)
- **Issue:** AI Director (story-to-timeline generation, multimodal AI workflows)

## Context

FilmCraft editors and creators increasingly look to AI models to accelerate the pre-production and assembly process: generating story outlines, screenplays, shot lists, character dialogue, voice narrations, storyboards, and video clips. However, bringing generative AI into FilmCraft must strictly comply with the core engineering rules defined in [AGENTS.md](../../AGENTS.md):

1. **Never crash:** External LLM responses and generated media are completely untrusted and hostile inputs. Corrupt or unparseable JSON, extreme values, or malformed media must return clear `Result<T, E>` errors and never panic or abort.
2. **Pure Rust and zero GPL/LGPL:** Codecs, transports, and parsers must remain pure Rust.
3. **Layering & offline-first:** Networking must remain strictly optional and off by default. Everything in Layer 2 must compile for `wasm32-unknown-unknown` (`cargo xtask wasm`). No network calls are permitted in unit tests.
4. **Exact time (`filmcraft_time::Tick`):** Shot durations, transitions, dialogue cues, and music timestamps must be represented using exact `Tick` integers (254,016,000,000 ticks/sec), never floating-point seconds.
5. **Everything is a command:** AI operations cannot silently mutate the active `Project`. AI workflows generate structured plans or assets that are ingested into the timeline through standard `Session::execute` undoable commands.
6. **Privacy & API key security:** AI API keys are confidential credentials. They must never leak into crash logs, project files, export metadata, console traces, or repository commits.

## Decisions

### 1. Crate Boundaries: `filmcraft-story` vs `filmcraft-ai`

We decouple the screenwriting domain model from network and provider infrastructure by establishing two separate Layer 2 (L2) crates:

- **`filmcraft-story` (`crates/story`, Layer 2):**
  - **Purpose:** Pure domain model for screenwriting, scene breakdowns, shot planning, characters, dialogue, camera directions, transitions, and audio/music cues.
  - **Dependencies:** `filmcraft-time`, `serde`, `serde_json`, `thiserror`.
  - **Boundary rules:** Absolutely **no HTTP or network dependencies**. It does not know what Gemini, OpenAI, or ureq are. It compiles instantly to `wasm32-unknown-unknown` and handles validation, consistency checking, duration arithmetic, and JSON interchange.
- **`filmcraft-ai` (`crates/ai`, Layer 2):**
  - **Purpose:** Provider client abstractions, transport handling, model catalog definitions, token cost estimation, request/response serialization, and cancellation plumbing.
  - **Dependencies:** `filmcraft-time`, `serde`, `serde_json`, `thiserror`, `log`, with optional `net` feature (`ureq`, `rustls`, `rustls-rustcrypto`).
  - **Boundary rules:** Crucially, `filmcraft-ai` **does not depend on `filmcraft-project`**. It deals exclusively with text prompts, structured JSON payloads, media bytes, and provider responses.
- **Timeline Integration (Layer 4/5):**
  - The conversion of a `StoryPlan` into project tracks, clips, graphics, and transitions is performed at Layer 4 (`filmcraft-engine`) via explicit, undoable `Command` implementations in `Session::execute`.
  - Neither `crates/story` nor `crates/ai` modifies `Project` or `Session` state directly.

### 2. Provider Abstractions: `AiProvider` and `AiTransport`

To ensure deterministic testing, zero real-network calls during CI, and swappable backends, all network communication routes through a two-tiered abstraction:

```rust
/// Low-level HTTP transport abstraction.
pub trait AiTransport: Send + Sync {
    /// Send an HTTP request cooperatively checking `cancel`.
    fn send(&self, req: &HttpRequest, cancel: &CancellationToken) -> Result<HttpResponse, AiError>;
}

/// High-level AI provider abstraction (Gemini, Mock, etc.).
pub trait AiProvider: Send + Sync {
    /// Provider identifier (e.g. "gemini", "mock").
    fn id(&self) -> &'static str;

    /// Generate structured output or completion.
    fn generate(
        &self,
        req: &GenerateRequest,
        cancel: &CancellationToken,
    ) -> Result<GenerateResponse, AiError>;

    /// Estimate token count and USD cost before invoking the provider.
    fn estimate_cost(&self, req: &GenerateRequest) -> Result<CostEstimate, AiError>;
}
```

- **`FakeTransport` in tests:** Tests inject a `FakeTransport` pre-loaded with canned JSON responses or error scenarios (HTTP 429, malformed bodies, network disconnects). Unit tests never touch the public internet.
- **`HttpTransport` in production:** When the `net` feature is enabled, `HttpTransport` uses `ureq` with pure-Rust TLS (`rustls` + `rustls-rustcrypto`) and platform root certificate verification, identical to the pattern in `filmcraft-speech`.

### 3. Feature Flags & WASM Compatibility

- In `crates/ai/Cargo.toml`:
  ```toml
  [features]
  default = []
  net = ["dep:ureq", "dep:rustls", "dep:rustls-rustcrypto"]
  ```
- Because `net` is disabled by default, `cargo check --target wasm32-unknown-unknown` (`cargo xtask wasm`) succeeds out of the box.
- For Web / WebAssembly targets, the browser runner (`apps/filmcraft-web`) can provide an `AiTransport` implementation backed by JavaScript `fetch()` or message passing, without pulling in non-wasm POSIX sockets.

### 4. API Key Storage, Flow, and Redaction Guarantee

API keys (such as `FILMCRAFT_GEMINI_API_KEY`) grant billable access to external cloud providers and must be protected at all times.

#### Storage and Resolution Precedence
When an AI call is initiated, the key is resolved in this order:
1. **Environment Variable:** `FILMCRAFT_GEMINI_API_KEY` (highest precedence, ideal for headless CLI and automated scripts).
2. **User Configuration File:** `$CONFIG_DIR/filmcraft/ai_keys.json` (where `$CONFIG_DIR` is `~/.config` on Linux, `~/Library/Application Support` on macOS, `%APPDATA%` on Windows). On Unix platforms, this file is created and checked with strict POSIX permissions `0600` (read/write only by owner).
3. **Session Memory:** Stored in application settings via UI dialog for the current running session.

#### Redaction Guarantees
- The key is wrapped in a dedicated `ApiKey` type.
- `ApiKey` implements `Debug` and `Display` by emitting `"[REDACTED]"`.
- `ApiKey` implements `Serialize` by rejecting or redacting serialization unless an explicit secure export method is called.
- Error messages returned from `AiTransport` or `AiProvider` strip query strings (which often hold `?key=...`) and authorization headers from URIs before constructing `AiError`.
- API keys are **never** stored inside `.fcproj` project files, interchange exports (EDL, XML, OTIO), or crash dumps.

### 5. Character Library in the Project Document

Visual and narrative consistency requires recurring characters to maintain consistent descriptions, voices, and visual references across multiple shots and scenes.

The Character model defines:
- `id`: Stable alphanumeric identifier (e.g. `char_elena_01`).
- `name`: Human-readable display name (e.g. "Elena Rostova").
- `aliases`: List of alternative names or aliases (e.g. `["Elena", "Commander"]`).
- `description`: Detailed physical appearance, age, clothing, personality, and distinctive visual traits.
- `reference_assets`: List of project media asset IDs or file paths providing visual reference images for image/video diffusion models.
- `voice`: Speech configuration containing `voice_id` (e.g. Kokoro voice name or Gemini TTS voice identifier), `pitch`, `rate`, and delivery tone.
- `style_notes`: Guidance on visual style, artist influences, positive prompts, and negative prompts.
- `provenance`: Origin metadata (`UserCreated`, `AiGenerated`, seed, timestamp, generator model).

### 6. `StoryPlan` Schema Specification & JSON Contract

The `StoryPlan` schema represents the complete breakdown of a story into scenes, shots, dialogues, and camera directives. All durations use exact FilmCraft `Tick` values (`Tick(i64)`).

#### Schema Fields
- `title`: String. Title of the screenplay or sequence.
- `logline`: Optional String. High-level synopsis.
- `genre`: Optional String. Story genre.
- `target_duration`: Optional `Tick`. Target sequence duration.
- `characters`: Array of `Character` specifications.
- `scenes`: Array of `Scene` items:
  - `id`: Stable scene ID.
  - `scene_number`: 1-based index.
  - `heading`: Scene heading (e.g. "EXT. SPACE STATION AIRLOCK - NIGHT").
  - `summary`: Scene action summary.
  - `mood`: Emotional atmosphere / lighting mood.
  - `shots`: Array of `Shot` items:
    - `id`: Stable shot ID.
    - `shot_number`: 1-based index within scene.
    - `shot_type`: Wide, Medium, CloseUp, ExtremeCloseUp, OverTheShoulder, Establishing, Insert, etc.
    - `camera_movement`: Static, PanLeft, PanRight, TiltUp, TiltDown, Tracking, Crane, Handheld, ZoomIn, ZoomOut.
    - `description`: Narrative description of what occurs in the shot.
    - `visual_prompt`: Optimized prompt for image/video generation models.
    - `characters_present`: Array of character IDs present in the shot.
    - `dialogues`: Array of dialogue lines:
      - `character_id`: Character ID speaking.
      - `text`: Spoken text.
      - `emotion`: Emotion or delivery cue (e.g. "whispering, urgent").
      - `estimated_duration`: Optional `Tick`.
    - `duration`: `Tick`. Exact planned duration of the shot.
    - `transition_out`: Transition to next shot (Cut, Dissolve, Wipe, FadeToBlack) with duration `Tick`.
    - `music_hint`: Background score suggestion.
    - `sfx_hints`: List of specific sound effects with relative timing.

#### Realistic JSON Example

```json
{
  "title": "Orbital Descent",
  "logline": "A stranded pilot attempts a perilous atmospheric entry over a ruined Earth.",
  "genre": "Sci-Fi Thriller",
  "target_duration": 1524096000000,
  "characters": [
    {
      "id": "char_elena",
      "name": "Elena Rostova",
      "aliases": ["Elena", "Vanguard Lead"],
      "description": "Mid-30s astronaut, intense hazel eyes, weathered flight suit with thermal patches.",
      "reference_assets": ["assets/chars/elena_ref_01.png"],
      "voice": {
        "voice_id": "gemini_tts_en_female_01",
        "pitch": 0.0,
        "pace": 1.05,
        "style": "calm under pressure"
      },
      "style_notes": "Cinematic 35mm film grain, anamorphic lens flare, photorealistic",
      "provenance": {
        "creator": "ai_director",
        "model": "gemini-2.5-pro",
        "seed": 421098
      }
    }
  ],
  "scenes": [
    {
      "id": "scene_01",
      "scene_number": 1,
      "heading": "INT. RECON COCKPIT - CONTINUOUS",
      "summary": "Warning lights flicker as re-entry heat shields begin to glow.",
      "mood": "Claustrophobic, high tension, amber warning strobes",
      "shots": [
        {
          "id": "shot_01_01",
          "shot_number": 1,
          "shot_type": "CloseUp",
          "camera_movement": "Handheld",
          "description": "Tight on Elena's visor reflecting orange plasma flames as the cockpit violently vibrates.",
          "visual_prompt": "Close-up of female astronaut visor reflecting fiery re-entry plasma, cinematic lighting, photorealistic, 8k, bokeh",
          "characters_present": ["char_elena"],
          "dialogues": [
            {
              "character_id": "char_elena",
              "text": "Control, angle of attack is holding at twenty-eight degrees. Temperature is nominal.",
              "emotion": "tense, focused",
              "estimated_duration": 889056000000
            }
          ],
          "duration": 1016064000000,
          "transition_out": {
            "kind": "Cut",
            "duration": 0
          },
          "music_hint": "Low bass drone building with ticking percussion",
          "sfx_hints": ["hull_creaking.wav", "alarm_pulse_amber.wav"]
        },
        {
          "id": "shot_01_02",
          "shot_number": 2,
          "shot_type": "Wide",
          "camera_movement": "Tracking",
          "description": "Exterior of the shuttle streaking across the upper mesosphere, glowing plasma trail behind it.",
          "visual_prompt": "Wide shot of sleek black experimental spacecraft plunging into Earth's upper atmosphere, plasma trail, curvature of the planet below, photorealistic",
          "characters_present": [],
          "dialogues": [],
          "duration": 508032000000,
          "transition_out": {
            "kind": "Dissolve",
            "duration": 127008000000
          },
          "music_hint": "Full orchestral swell into crescendo",
          "sfx_hints": ["atmospheric_rumble_stereo.wav"]
        }
      ]
    }
  ]
}
```

*(Note: `1016064000000` ticks = 4.0 seconds; `508032000000` ticks = 2.0 seconds; `127008000000` ticks = 0.5 seconds.)*

### 7. Long Jobs, Progress, Cancellation, Cost Estimation, and Dry Run

1. **Background execution:** Generating story plans, image diffusion, and video generation are long jobs. They run asynchronously on worker threads or background tasks managed by `filmcraft-engine`'s job queue.
2. **Cooperative Cancellation:** Every operation receives an atomic `CancellationToken`. Sockets and poll loops check `cancel.is_cancelled()`. If set, the job immediately aborts with `Err(AiError::Cancelled)`.
3. **Progress Reporting:** Callbacks receive `(completed_units: u64, total_units: u64, stage_message: &str)`. Returning `false` from a progress callback cancels the operation.
4. **Cost Estimation:** Before executing any billable API call, callers invoke `AiProvider::estimate_cost(&req)`. The returned `CostEstimate` contains:
   - `estimated_input_tokens`: u32
   - `estimated_output_tokens`: u32
   - `estimated_images`: u32
   - `estimated_video_seconds`: f64
   - `estimated_cost_usd`: f64
   The UI or CLI can present this estimate to the user for explicit confirmation.
5. **Dry Run Mode:** All generation requests include a `dry_run: bool` flag. When `dry_run` is `true`:
   - Inputs and schemas are fully validated.
   - Cost estimates and token calculations are produced.
   - No external HTTP request is dispatched.
   - A mock or synthesized skeleton response is returned without billable expense.

### 8. Pinned Model Catalog

Model IDs are centralized in a configurable, statically verified catalog within `filmcraft-ai`:

| Model ID | Provider | Capability | Context / Specs | Pricing (per 1M / unit) |
|---|---|---|---|---|
| `gemini-2.5-flash` | Google | Text / Structured JSON | 1M tokens, rapid planning | $0.10 input / $0.40 output |
| `gemini-2.5-pro` | Google | Text / Complex Reasoning | 2M tokens, deep narrative analysis | $1.25 input / $5.00 output |
| `imagen-3.0-generate-002` | Google | Image Generation | 1:1, 16:9, 9:16 storyboards | $0.030 per image |
| `veo-2.0-generate-001` | Google | Video Generation | 720p/1080p video clips, 5–8 sec | $0.20 per second |
| `gemini-2.5-flash-tts` | Google | Cloud Multilingual TTS | High naturalness, multi-voice | $15.00 per 1M characters |
| `kokoro-82m` | Local (Candle) | Local Pure-Rust TTS | 24kHz natural speech, 0ms network | Free / Offline |

Default model IDs are pinned in code constants to avoid runtime drift, but the catalog is user-extensible via preferences.

## Consequences

- Clear separation of concerns: Screenwriting data structures in `filmcraft-story` remain completely pure, fast, and testable without HTTP or platform dependencies.
- Provider and network concerns in `filmcraft-ai` remain decoupled from project mutation logic.
- WebAssembly (`wasm32-unknown-unknown`) builds remain green by default.
- Zero network exposure during testing; complete offline predictability.
