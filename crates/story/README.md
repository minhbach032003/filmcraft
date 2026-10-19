# filmcraft-story

Layer 2 crate containing the domain model for screenwriting, scene breakdowns, shot lists,
character definitions, and structural screenplay validation in FilmCraft.

## Features

- **[`StoryPlan`]:** full screenplay and sequence breakdown into scenes, shots, dialogues, and characters.
- **[`Character`]:** character library definition including visual descriptions, voice traits, style hints, and provenance.
- **[`Scene`] and [`Shot`]:** scene breakdown with shot framings (`ShotType`), camera movements (`CameraMovement`), dialogues, transitions, and audio hints.
- **Exact [`Tick`] Time:** all shot durations, dialogue timings, and transitions are integer ticks (`filmcraft_time::Tick`), ensuring exact frame synchronization across broadcast frame rates.
- **Validation:** structural integrity checks verifying non-negative durations, sequential scene/shot numbering, character reference resolution, and length bounds.

## Layering & Network Rules

- Layer 2: does not depend on L3/L4/L5 crates (`render`, `engine`, `ui-egui`).
- **No network dependencies:** completely pure domain data model.
- Compiles cleanly to `wasm32-unknown-unknown` (`cargo xtask wasm`).
