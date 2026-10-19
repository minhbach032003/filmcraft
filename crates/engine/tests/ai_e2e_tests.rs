//! End-to-end integration test for FilmCraft AI Director.
//!
//! Validates the entire pipeline:
//! 1. Character creation in project.
//! 2. Screenplay ingestion via offline Fountain parser -> StoryPlan.
//! 3. StoryPlan validation.
//! 4. Sequence synthesis via `ai.buildSequence` with video clips, dialogue captions, and scene markers.
//! 5. Verify undo/redo atomicity: an entire sequence build can be undone in a single step.

use filmcraft_engine::Session;
use filmcraft_project::ItemId;
use serde_json::json;

#[test]
fn test_ai_director_e2e_full_workflow() {
    let mut session = Session::default();

    // Step 1: Add characters
    session
        .execute(
            "ai.characterAdd",
            json!({
                "id": "c_maya",
                "name": "Maya Vance",
                "description": "Astronaut pilot in orange flight suit"
            }),
        )
        .expect("characterAdd Maya");

    session
        .execute(
            "ai.characterAdd",
            json!({
                "id": "c_tars",
                "name": "TARS AI",
                "description": "Tactical navigation robot"
            }),
        )
        .expect("characterAdd TARS");

    assert_eq!(session.project.characters.len(), 2);

    // Step 2: Ingest screenplay text
    let script = r#"
INT. COCKPIT - NIGHT

A siren wails softly against red emergency lighting.

MAYA
(tense)
TARS, give me atmospheric entry coordinates.

TARS
Calculating trajectory. Hold your trajectory steady.

EXT. UPPER STRATOSPHERE - CONTINUOUS

The ship enters the burning atmosphere with orange plasma tails.
"#;

    let plan_val = session
        .execute(
            "ai.planScript",
            json!({
                "script": script,
                "title": "Orbital Re-entry",
                "mode": "fountain"
            }),
        )
        .expect("planScript");

    assert_eq!(plan_val["title"], json!("Orbital Re-entry"));
    let scenes = plan_val["scenes"].as_array().expect("scenes array");
    assert_eq!(scenes.len(), 2);

    // Step 3: Build Sequence from Plan
    let before_rev = session.revision;
    let build_res = session
        .execute(
            "ai.buildSequence",
            json!({
                "plan": plan_val,
                "name": "Orbital Re-entry Assembly"
            }),
        )
        .expect("buildSequence");

    assert_eq!(build_res["status"], json!("ok"));
    let seq_id = ItemId(build_res["sequence_id"].as_u64().expect("seq id"));
    let seq = session.project.sequence(seq_id).expect("sequence exists");

    // Assert sequence structure
    assert_eq!(seq.video_tracks.len(), 2);
    assert!(seq.video_tracks[0].items.len() >= 3);
    assert_eq!(seq.markers.len(), 2, "Marker for each scene");
    assert_eq!(seq.caption_tracks.len(), 1, "AI dialogue subtitle track");
    assert_eq!(seq.caption_tracks[0].captions.len(), 2, "Two spoken dialogue lines");
    assert_eq!(seq.caption_tracks[0].captions[0].text, "TARS, give me atmospheric entry coordinates.");
    assert_eq!(seq.caption_tracks[0].captions[1].text, "Calculating trajectory. Hold your trajectory steady.");

    // Step 4: Verify single-step undo
    assert!(session.revision > before_rev);
    let undo_res = session.execute("edit.undo", json!({}));
    assert!(undo_res.is_ok(), "Undo succeeds");
    // After undo, the sequence item no longer exists in project
    assert!(session.project.sequence(seq_id).is_none());

    // Step 5: Verify redo restores it
    let redo_res = session.execute("edit.redo", json!({}));
    assert!(redo_res.is_ok(), "Redo succeeds");
    assert!(session.project.sequence(seq_id).is_some());
}
