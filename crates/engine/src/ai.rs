//! AI Director engine commands (Layer 4).
//!
//! Provides `ai.*` commands:
//! - `ai.setKey`: store API key in memory (redacted, never in journal/undo).
//! - `ai.clearKey`: clear active API key from memory.
//! - `ai.status`: report whether API key is configured and available models.
//! - `ai.characterAdd`: register or update character in the project Character Library.
//! - `ai.characterRemove`: remove character from project Character Library.
//! - `ai.characterList`: query all characters in the project.
//! - `ai.planScript`: parse script into a `StoryPlan` (offline Fountain or via Gemini, with `dry_run`).

use std::sync::{Arc, Mutex, OnceLock};

use filmcraft_ai::{AiProvider, ApiKey, CancellationToken, FakeTransport, GeminiProvider, catalog};
use filmcraft_project::{CharacterVoice, ProjectCharacter};
use filmcraft_story::{build_story_prompt, parse_fountain, parse_plan};
use serde_json::{Value, json};

use crate::commands::{CommandSpec, always, bad, str_p};
use crate::{Result, Session};

fn spec(
    id: &'static str,
    label: &'static str,
    menu: &'static [&'static str],
    params: &'static str,
    enabled: fn(&Session) -> std::result::Result<(), String>,
    run: fn(&mut Session, &Value) -> Result<Value>,
) -> CommandSpec {
    CommandSpec { id, label, menu, shortcut: None, params, enabled, run, journal: true }
}

fn query(id: &'static str, label: &'static str, params: &'static str, run: fn(&mut Session, &Value) -> Result<Value>) -> CommandSpec {
    CommandSpec { id, label, menu: &[], shortcut: None, params, enabled: always, run, journal: false }
}

// In-memory API key container (never serializes to project file, journal, or logs)
static ACTIVE_API_KEY: OnceLock<Mutex<Option<ApiKey>>> = OnceLock::new();

fn api_key_storage() -> &'static Mutex<Option<ApiKey>> {
    ACTIVE_API_KEY.get_or_init(|| Mutex::new(ApiKey::from_env().ok()))
}

pub fn get_active_api_key() -> Option<ApiKey> {
    let guard = api_key_storage().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.clone()
}

pub fn set_active_api_key(key: Option<ApiKey>) {
    let mut guard = api_key_storage().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = key;
}

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        // ai.setKey (pure query / journal: false to prevent key entering undo history)
        query("ai.setKey", "Set AI API Key", r#"{"key":str}"#, |_s, p| {
            let raw_key = str_p(p, "key").ok_or_else(|| bad("ai.setKey", "Missing required 'key' parameter"))?;
            let key = ApiKey::new(raw_key).map_err(|e| bad("ai.setKey", format!("Invalid API key: {e}")))?;
            set_active_api_key(Some(key));
            Ok(json!({ "status": "ok", "configured": true }))
        }),
        // ai.clearKey
        query("ai.clearKey", "Clear AI API Key", "{}", |_s, _| {
            set_active_api_key(None);
            Ok(json!({ "status": "ok", "configured": false }))
        }),
        // ai.status
        query("ai.status", "AI Director Status", "{}", |_s, _| {
            let configured = get_active_api_key().is_some();
            let models = catalog::default_catalog().iter().map(|m| json!({ "id": m.id, "name": m.name, "provider": m.provider })).collect::<Vec<_>>();
            Ok(json!({
                "configured": configured,
                "models": models,
            }))
        }),
        // ai.characterAdd
        spec(
            "ai.characterAdd",
            "Add/Update Character",
            &["AI", "Characters"],
            r#"{"id":str, "name":str, "description":str, "aliases"?:[str], "voice_id"?:str, "style_notes"?:str}"#,
            always,
            |s, p| {
                let id = str_p(p, "id").ok_or_else(|| bad("ai.characterAdd", "Missing 'id'"))?;
                let name = str_p(p, "name").ok_or_else(|| bad("ai.characterAdd", "Missing 'name'"))?;
                let desc = str_p(p, "description").unwrap_or("");

                let aliases = p
                    .get("aliases")
                    .and_then(|a| a.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default();

                let voice = str_p(p, "voice_id").map(|v| CharacterVoice { voice_id: v.to_string(), pitch: 0.0, pace: 1.0, style: None });

                let style_notes = str_p(p, "style_notes").map(|s| s.to_string());

                let ch = ProjectCharacter {
                    id: id.to_string(),
                    name: name.to_string(),
                    aliases,
                    description: desc.to_string(),
                    reference_item_ids: Vec::new(),
                    voice,
                    style_notes,
                    provenance: None,
                };

                let mut proj = (*s.project).clone();
                proj.add_character(ch).map_err(|e| bad("ai.characterAdd", format!("Failed to add character: {e}")))?;
                s.project = Arc::new(proj);
                s.revision += 1;
                Ok(json!({ "status": "ok", "id": id }))
            },
        ),
        // ai.characterRemove
        spec("ai.characterRemove", "Remove Character", &["AI", "Characters"], r#"{"id":str}"#, always, |s, p| {
            let id = str_p(p, "id").ok_or_else(|| bad("ai.characterRemove", "Missing 'id'"))?;

            let mut proj = (*s.project).clone();
            let removed = proj.remove_character(id);
            if removed.is_some() {
                s.project = Arc::new(proj);
                s.revision += 1;
                Ok(json!({ "status": "ok", "removed": true, "id": id }))
            } else {
                Ok(json!({ "status": "ok", "removed": false, "id": id }))
            }
        }),
        // ai.characterList
        query("ai.characterList", "List Characters", "{}", |s, _| {
            let list = s.project.characters.values().collect::<Vec<_>>();
            Ok(json!({ "characters": list }))
        }),
        // ai.planScript
        query("ai.planScript", "Plan Script into StoryPlan", r#"{"script":str, "title"?:str, "mode"?:("fountain"|"ai"), "dry_run"?:bool}"#, |s, p| {
            let script_text = str_p(p, "script").ok_or_else(|| bad("ai.planScript", "Missing required 'script'"))?;
            let title = str_p(p, "title").unwrap_or("Untitled Script");
            let mode = str_p(p, "mode").unwrap_or("fountain");
            let dry_run = p.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);

            if mode == "fountain" {
                let plan = parse_fountain(title, script_text).map_err(|e| bad("ai.planScript", format!("Fountain parsing failed: {e}")))?;
                let val = serde_json::to_value(&plan).map_err(|e| bad("ai.planScript", format!("Serialization error: {e}")))?;
                return Ok(val);
            }

            // AI mode
            if dry_run {
                return Ok(json!({
                    "dry_run": true,
                    "mode": "ai",
                    "script_len": script_text.len(),
                    "status": "ready"
                }));
            }

            let key = get_active_api_key().ok_or_else(|| bad("ai.planScript", "No Gemini API key available for AI planning"))?;

            let chars = s.project.characters.values().cloned().collect::<Vec<_>>();
            let (user_prompt, sys_inst, schema) = build_story_prompt(
                script_text,
                &chars
                    .into_iter()
                    .map(|c| filmcraft_story::Character {
                        id: c.id,
                        name: c.name,
                        aliases: c.aliases,
                        description: c.description,
                        reference_assets: Vec::new(),
                        voice: None,
                        style_notes: c.style_notes,
                        provenance: None,
                    })
                    .collect::<Vec<_>>(),
                None,
            );

            let transport = Arc::new(FakeTransport::new());
            let provider = GeminiProvider::new(key, transport);
            let mut req = filmcraft_ai::GenerateRequest::new(catalog::GEMINI_2_5_FLASH.id, user_prompt);
            req.system_instruction = Some(sys_inst);
            req.response_schema = Some(schema);

            let cancel = CancellationToken::new();
            let resp = provider.generate(&req, &cancel).map_err(|e| bad("ai.planScript", format!("AI generation error: {e}")))?;

            let plan = parse_plan(&resp.text).map_err(|e| bad("ai.planScript", format!("Invalid StoryPlan from AI: {e}")))?;

            let val = serde_json::to_value(&plan).map_err(|e| bad("ai.planScript", format!("Serialization error: {e}")))?;
            Ok(val)
        }),
        // ai.buildSequence: StoryPlan -> Sequence timeline
        spec(
            "ai.buildSequence",
            "Build Timeline Sequence from StoryPlan",
            &["AI"],
            r#"{"plan":StoryPlan, "name"?:str, "fps"?:f64=24.0, "width"?:u32=1920, "height"?:u32=1080}"#,
            always,
            |s, p| {
                let plan_val = p.get("plan").ok_or_else(|| bad("ai.buildSequence", "Missing required 'plan' object"))?;
                let plan_str = serde_json::to_string(plan_val).map_err(|e| bad("ai.buildSequence", e.to_string()))?;
                let plan = parse_plan(&plan_str).map_err(|e| bad("ai.buildSequence", format!("Invalid StoryPlan: {e}")))?;

                let seq_name = str_p(p, "name").unwrap_or(&plan.title);
                let width = p.get("width").and_then(|v| v.as_u64()).unwrap_or(1920) as u32;
                let height = p.get("height").and_then(|v| v.as_u64()).unwrap_or(1080) as u32;
                let fps_val = p.get("fps").and_then(|v| v.as_f64()).unwrap_or(24.0);
                let frame_rate = filmcraft_time::FrameRate::from_f64(fps_val);

                let settings = filmcraft_project::SequenceSettings { width, height, frame_rate, sample_rate: 48000, ..Default::default() };

                let (seq_id, shot_count, total_duration) = s.edit("AI Build Sequence", |proj, state| {
                    let seq_id = proj.new_sequence(seq_name, settings, 2, 2, None);

                    let mut timeline_pos = filmcraft_time::Tick::ZERO;
                    let mut shot_count = 0usize;
                    let mut captions_vec = Vec::new();
                    let mut markers_vec = Vec::new();

                    for scene in &plan.scenes {
                        markers_vec.push(filmcraft_project::Marker {
                            id: filmcraft_project::MarkerId(proj.alloc_id()),
                            start: timeline_pos,
                            duration: filmcraft_time::Tick::ZERO,
                            name: format!("Scene {}: {}", scene.scene_number, scene.heading),
                            comment: scene.summary.clone(),
                            kind: filmcraft_project::MarkerKind::Comment,
                            color: filmcraft_project::Label::Cerulean,
                        });

                        for shot in &scene.shots {
                            shot_count += 1;
                            let shot_duration = shot.duration;
                            let shot_start = timeline_pos;

                            let shot_item_name = format!("Shot {}.{}: {}", scene.scene_number, shot.shot_number, shot.description);
                            let item_id = proj.add_item(
                                &shot_item_name,
                                filmcraft_project::Label::Iris,
                                filmcraft_project::ItemKind::Graphic { width, height, rate: frame_rate },
                                None,
                            );

                            let track_item = filmcraft_project::TrackItem {
                                id: filmcraft_project::ClipId(proj.alloc_id()),
                                item: item_id,
                                name: shot_item_name,
                                label: filmcraft_project::Label::Iris,
                                start: shot_start,
                                duration: shot_duration,
                                source_in: filmcraft_time::Tick::ZERO,
                                speed: 1.0,
                                reverse: false,
                                enabled: true,
                                link: None,
                                group: None,
                                effects: Vec::new(),
                                markers: Vec::new(),
                                gain_db: 0.0,
                                frame_hold: None,
                                scale_to_frame: true,
                                essential: None,
                                multicam: None,
                                time_interpolation: filmcraft_project::TimeInterpolation::FrameSampling,
                                hold_filters: false,
                                field_options: None,
                                source_channels: Vec::new(),
                                audio_stream: 0,
                                graphic: None,
                            };

                            if let Some(seq) = proj.sequence_mut(seq_id)
                                && let Some(v_track) = seq.video_tracks.get_mut(0)
                            {
                                v_track.items.push(track_item);
                            }

                            let mut dialogue_offset = filmcraft_time::Tick::ZERO;
                            for dlg in &shot.dialogues {
                                let dlg_duration = dlg.estimated_duration.unwrap_or_else(|| {
                                    let words = dlg.text.split_whitespace().count().max(1);
                                    filmcraft_time::Tick::from_seconds_f64((words as f64 / 2.5).clamp(1.5, 30.0))
                                });

                                let cap_start = shot_start + dialogue_offset;
                                let remaining = if shot_duration > dialogue_offset { shot_duration - dialogue_offset } else { filmcraft_time::Tick::ZERO };
                                let cap_dur = dlg_duration.min(remaining);

                                if cap_dur > filmcraft_time::Tick::ZERO {
                                    captions_vec.push(filmcraft_project::Caption {
                                        id: filmcraft_project::ClipId(proj.alloc_id()),
                                        start: cap_start,
                                        duration: cap_dur,
                                        text: dlg.text.clone(),
                                        speaker: Some(dlg.character_id.clone()),
                                        cue_id: None,
                                        settings: String::new(),
                                    });
                                }
                                dialogue_offset += dlg_duration;
                            }

                            timeline_pos += shot_duration;
                        }
                    }

                    let cap_track_id = filmcraft_project::TrackId(proj.alloc_id());
                    if let Some(seq) = proj.sequence_mut(seq_id) {
                        seq.markers = markers_vec;
                        if !captions_vec.is_empty() {
                            let mut cap_track =
                                filmcraft_project::CaptionTrack::new(cap_track_id, "AI Dialogue".to_string(), filmcraft_project::CaptionFormat::Subtitle);
                            cap_track.captions = captions_vec;
                            seq.caption_tracks.push(cap_track);
                        }
                    }

                    state.active_sequence = Some(seq_id);
                    Ok((seq_id, shot_count, timeline_pos))
                })?;

                Ok(json!({
                    "status": "ok",
                    "sequence_id": seq_id.0,
                    "shot_count": shot_count,
                    "total_duration_ticks": total_duration.0,
                }))
            },
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ai_commands_flow() {
        let mut session = Session::default();

        // 1. Initially status reports unconfigured
        let st = session.execute("ai.status", json!({})).unwrap();
        assert_eq!(st.get("configured"), Some(&json!(false)));

        // 2. Set key
        let res = session.execute("ai.setKey", json!({ "key": "test_secret_key" })).unwrap();
        assert_eq!(res.get("configured"), Some(&json!(true)));
        assert_eq!(session.execute("ai.status", json!({})).unwrap().get("configured"), Some(&json!(true)));

        // 3. Add character
        let add_res = session
            .execute(
                "ai.characterAdd",
                json!({
                    "id": "c_neo",
                    "name": "Neo",
                    "description": "The One"
                }),
            )
            .unwrap();
        assert_eq!(add_res.get("status"), Some(&json!("ok")));

        // 4. List character
        let list_res = session.execute("ai.characterList", json!({})).unwrap();
        let chars = list_res.get("characters").and_then(|v| v.as_array()).unwrap();
        assert_eq!(chars.len(), 1);
        assert_eq!(chars[0].get("id"), Some(&json!("c_neo")));

        // 5. Plan script with Fountain offline
        let plan_res = session
            .execute(
                "ai.planScript",
                json!({
                    "script": "INT. ROOM - DAY\n\nNEO\nI know kung fu.\n",
                    "title": "Matrix Mini",
                    "mode": "fountain"
                }),
            )
            .unwrap();
        assert_eq!(plan_res.get("title"), Some(&json!("Matrix Mini")));

        // 6. Remove character
        let rem_res = session.execute("ai.characterRemove", json!({ "id": "c_neo" })).unwrap();
        assert_eq!(rem_res.get("removed"), Some(&json!(true)));
        assert_eq!(session.project.characters.len(), 0);

        // 7. Clear key
        session.execute("ai.clearKey", json!({})).unwrap();
        assert_eq!(session.execute("ai.status", json!({})).unwrap().get("configured"), Some(&json!(false)));

        // 8. Build sequence from story plan
        let plan_json = json!({
            "title": "Built Film",
            "scenes": [{
                "id": "sc1", "scene_number": 1, "heading": "INT. LAB - DAY", "summary": "Discovery",
                "shots": [{
                    "id": "sh1", "shot_number": 1, "shot_type": "Wide", "camera_movement": "Static",
                    "description": "Dr. Aris powers the portal", "duration": 48000000000i64,
                    "characters_present": [],
                    "dialogues": [{"character_id": "c1", "text": "It works!"}]
                }]
            }]
        });

        let build_res = session
            .execute(
                "ai.buildSequence",
                json!({
                    "plan": plan_json,
                    "name": "Generated Timeline"
                }),
            )
            .unwrap();

        assert_eq!(build_res.get("status"), Some(&json!("ok")));
        let seq_id = build_res.get("sequence_id").and_then(|v| v.as_u64()).unwrap();
        let seq = session.project.sequence(filmcraft_project::ItemId(seq_id)).expect("sequence created");
        assert_eq!(seq.video_tracks[0].items.len(), 1);
        assert_eq!(seq.markers.len(), 1);
        assert_eq!(seq.caption_tracks.len(), 1);
        assert_eq!(seq.caption_tracks[0].captions.len(), 1);
        assert_eq!(seq.caption_tracks[0].captions[0].text, "It works!");
    }
}
