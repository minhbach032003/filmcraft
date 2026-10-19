//! FilmCraft story & screenplay domain models.
//!
//! - [`StoryPlan`]: top-level screenplay breakdown into scenes, shots, dialogues, and characters.
//! - [`Character`]: character specification (id, visual traits, voice, style notes, provenance).
//! - [`Scene`] and [`Shot`]: structural scene breakdown with camera directions, shot framing,
//!   and exact [`Tick`] timings.
//! - [`Dialogue`]: spoken dialogue line linked to a character ID.
//! - [`Transition`]: transition into the subsequent shot (Cut, Dissolve, Wipe, etc.).

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]

use std::collections::HashSet;

use filmcraft_time::Tick;
use serde::{Deserialize, Serialize};

pub mod fountain;
pub mod prompt;

pub use fountain::parse_fountain;
pub use prompt::build_story_prompt;

/// Parse and validate a story plan JSON string.
///
/// Strips markdown fences if present (e.g. ```json ... ```) and runs strict validation:
/// non-empty scenes, positive durations, sequential numbers, character ID checks, length bounds.
pub fn parse_plan(json_str: &str) -> Result<StoryPlan, StoryError> {
    let trimmed = json_str.trim();
    if trimmed.is_empty() {
        return Err(StoryError::Serialization("Empty JSON input".to_string()));
    }

    // Strip Markdown code block delimiters if returned by an LLM
    let clean_json = if trimmed.starts_with("```") {
        let lines: Vec<&str> = trimmed.lines().collect();
        if lines.len() >= 2 && lines.first().map(|l| l.starts_with("```")).unwrap_or(false) && lines.last().map(|l| l.starts_with("```")).unwrap_or(false) {
            lines[1..lines.len() - 1].join("\n")
        } else {
            trimmed.to_string()
        }
    } else {
        trimmed.to_string()
    };

    let plan = StoryPlan::from_json(&clean_json)?;
    plan.validate()?;
    Ok(plan)
}
pub const MAX_TITLE_BYTES: usize = 1024;
/// Maximum logline or summary length in bytes.
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
/// Maximum number of scenes in a single plan.
pub const MAX_SCENES: usize = 10_000;
/// Maximum number of shots in a single scene.
pub const MAX_SHOTS_PER_SCENE: usize = 10_000;

/// Story & screenplay validation errors.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum StoryError {
    #[error("story plan has no scenes")]
    EmptyPlan,
    #[error("scene {scene_id} has no shots")]
    EmptyScene { scene_id: String },
    #[error("invalid duration for {location}: {detail}")]
    InvalidDuration { location: String, detail: String },
    #[error("unknown character ID `{character_id}` referenced in {location}")]
    UnknownCharacter { character_id: String, location: String },
    #[error("duplicate character ID `{0}`")]
    DuplicateCharacterId(String),
    #[error("duplicate scene ID `{0}`")]
    DuplicateSceneId(String),
    #[error("duplicate shot ID `{0}`")]
    DuplicateShotId(String),
    #[error("invalid scene number {0} (must be sequential starting from 1)")]
    InvalidSceneNumber(u32),
    #[error("invalid shot number {shot_number} in scene {scene_id} (must be sequential starting from 1)")]
    InvalidShotNumber { scene_id: String, shot_number: u32 },
    #[error("field `{field}` exceeds length limit ({bytes} bytes > {max} bytes)")]
    TextTooLong { field: &'static str, bytes: usize, max: usize },
    #[error("serialization error: {0}")]
    Serialization(String),
}

impl From<serde_json::Error> for StoryError {
    fn from(e: serde_json::Error) -> Self {
        StoryError::Serialization(e.to_string())
    }
}

/// Provenance metadata tracking the origin of an entity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub creator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

/// Speech synthesis voice configuration for a character.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VoiceConfig {
    pub voice_id: String,
    #[serde(default)]
    pub pitch: f64,
    #[serde(default = "default_pace")]
    pub pace: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
}

fn default_pace() -> f64 {
    1.0
}

/// Character specification in the project character library.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Character {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_assets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<VoiceConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
}

/// Shot framing type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShotType {
    Establishing,
    ExtremeWide,
    Wide,
    MediumWide,
    Medium,
    MediumCloseUp,
    CloseUp,
    ExtremeCloseUp,
    OverTheShoulder,
    PointOfView,
    Insert,
    Cutaway,
}

/// Camera movement directive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CameraMovement {
    Static,
    PanLeft,
    PanRight,
    TiltUp,
    TiltDown,
    Tracking,
    Crane,
    Handheld,
    ZoomIn,
    ZoomOut,
    Dolly,
}

/// Kind of transition between shots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionKind {
    Cut,
    Dissolve,
    Wipe,
    FadeToBlack,
    FadeToWhite,
}

/// Visual transition out of a shot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    pub kind: TransitionKind,
    #[serde(default)]
    pub duration: Tick,
}

impl Default for Transition {
    fn default() -> Self {
        Transition { kind: TransitionKind::Cut, duration: Tick::ZERO }
    }
}

/// A line of spoken dialogue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dialogue {
    pub character_id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emotion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_duration: Option<Tick>,
}

/// A single shot within a scene.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shot {
    pub id: String,
    pub shot_number: u32,
    pub shot_type: ShotType,
    pub camera_movement: CameraMovement,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub characters_present: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dialogues: Vec<Dialogue>,
    pub duration: Tick,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_out: Option<Transition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub music_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sfx_hints: Vec<String>,
}

/// A dramatic scene consisting of multiple shots.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub id: String,
    pub scene_number: u32,
    pub heading: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mood: Option<String>,
    pub shots: Vec<Shot>,
}

impl Scene {
    /// Calculate total duration of all shots in this scene.
    pub fn duration(&self) -> Tick {
        let mut total = 0i64;
        for s in &self.shots {
            total = total.saturating_add(s.duration.0.max(0));
        }
        Tick(total)
    }
}

/// Complete story plan / screenplay breakdown.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoryPlan {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_duration: Option<Tick>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub characters: Vec<Character>,
    pub scenes: Vec<Scene>,
}

impl StoryPlan {
    /// Parse a `StoryPlan` from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, StoryError> {
        let plan: Self = serde_json::from_str(json)?;
        Ok(plan)
    }

    /// Serialize this plan to a formatted JSON string.
    pub fn to_json_pretty(&self) -> Result<String, StoryError> {
        serde_json::to_string_pretty(self).map_err(StoryError::from)
    }

    /// Calculate total planned duration across all scenes.
    pub fn total_duration(&self) -> Tick {
        let mut total = 0i64;
        for sc in &self.scenes {
            total = total.saturating_add(sc.duration().0);
        }
        Tick(total)
    }

    /// Validate the story plan structure, bounds, character references, and sequential numbering.
    pub fn validate(&self) -> Result<(), StoryError> {
        if self.title.len() > MAX_TITLE_BYTES {
            return Err(StoryError::TextTooLong { field: "title", bytes: self.title.len(), max: MAX_TITLE_BYTES });
        }
        if let Some(ref l) = self.logline
            && l.len() > MAX_TEXT_BYTES
        {
            return Err(StoryError::TextTooLong { field: "logline", bytes: l.len(), max: MAX_TEXT_BYTES });
        }
        if self.scenes.is_empty() {
            return Err(StoryError::EmptyPlan);
        }
        if self.scenes.len() > MAX_SCENES {
            return Err(StoryError::TextTooLong { field: "scenes", bytes: self.scenes.len(), max: MAX_SCENES });
        }

        let mut known_characters = HashSet::new();
        for ch in &self.characters {
            if ch.id.trim().is_empty() {
                return Err(StoryError::DuplicateCharacterId("empty".into()));
            }
            if !known_characters.insert(&ch.id) {
                return Err(StoryError::DuplicateCharacterId(ch.id.clone()));
            }
        }

        let mut scene_ids = HashSet::new();
        let mut expected_scene_num = 1u32;

        for scene in &self.scenes {
            if scene.id.trim().is_empty() || !scene_ids.insert(&scene.id) {
                return Err(StoryError::DuplicateSceneId(scene.id.clone()));
            }
            if scene.scene_number != expected_scene_num {
                return Err(StoryError::InvalidSceneNumber(scene.scene_number));
            }
            expected_scene_num = expected_scene_num.saturating_add(1);

            if scene.shots.is_empty() {
                return Err(StoryError::EmptyScene { scene_id: scene.id.clone() });
            }
            if scene.shots.len() > MAX_SHOTS_PER_SCENE {
                return Err(StoryError::TextTooLong { field: "shots", bytes: scene.shots.len(), max: MAX_SHOTS_PER_SCENE });
            }

            let mut shot_ids = HashSet::new();
            let mut expected_shot_num = 1u32;

            for shot in &scene.shots {
                if shot.id.trim().is_empty() || !shot_ids.insert(&shot.id) {
                    return Err(StoryError::DuplicateShotId(shot.id.clone()));
                }
                if shot.shot_number != expected_shot_num {
                    return Err(StoryError::InvalidShotNumber { scene_id: scene.id.clone(), shot_number: shot.shot_number });
                }
                expected_shot_num = expected_shot_num.saturating_add(1);

                if shot.duration.0 <= 0 {
                    return Err(StoryError::InvalidDuration {
                        location: format!("scene {} shot {}", scene.id, shot.id),
                        detail: format!("duration must be > 0 ticks, got {}", shot.duration.0),
                    });
                }

                if let Some(tr) = shot.transition_out
                    && tr.duration.0 < 0
                {
                    return Err(StoryError::InvalidDuration {
                        location: format!("scene {} shot {} transition", scene.id, shot.id),
                        detail: "transition duration cannot be negative".into(),
                    });
                }

                // If characters are specified in character library, validate references
                if !known_characters.is_empty() {
                    for cp in &shot.characters_present {
                        if !known_characters.contains(cp) {
                            return Err(StoryError::UnknownCharacter { character_id: cp.clone(), location: format!("shot {} characters_present", shot.id) });
                        }
                    }
                    for dlg in &shot.dialogues {
                        if !known_characters.contains(&dlg.character_id) {
                            return Err(StoryError::UnknownCharacter {
                                character_id: dlg.character_id.clone(),
                                location: format!("shot {} dialogue", shot.id),
                            });
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_validate_adr_json_example() {
        let json = r#"{
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
}"#;

        let plan = StoryPlan::from_json(json).expect("valid story plan json");
        assert_eq!(plan.title, "Orbital Descent");
        assert_eq!(plan.characters.len(), 1);
        assert_eq!(plan.scenes.len(), 1);
        assert_eq!(plan.scenes[0].shots.len(), 2);
        assert_eq!(plan.scenes[0].shots[0].shot_type, ShotType::CloseUp);
        assert_eq!(plan.scenes[0].shots[1].camera_movement, CameraMovement::Tracking);

        // Duration calculation: 1016064000000 + 508032000000 = 1524096000000 ticks (6.0s)
        assert_eq!(plan.total_duration(), Tick(1_524_096_000_000));
        assert_eq!(plan.validate(), Ok(()));
    }

    #[test]
    fn rejects_unknown_character() {
        let mut plan = StoryPlan {
            title: "Test".into(),
            logline: None,
            genre: None,
            target_duration: None,
            characters: vec![Character {
                id: "c1".into(),
                name: "Char 1".into(),
                aliases: vec![],
                description: "Desc".into(),
                reference_assets: vec![],
                voice: None,
                style_notes: None,
                provenance: None,
            }],
            scenes: vec![Scene {
                id: "s1".into(),
                scene_number: 1,
                heading: "INT. ROOM - DAY".into(),
                summary: "Summary".into(),
                mood: None,
                shots: vec![Shot {
                    id: "sh1".into(),
                    shot_number: 1,
                    shot_type: ShotType::Medium,
                    camera_movement: CameraMovement::Static,
                    description: "Desc".into(),
                    visual_prompt: None,
                    characters_present: vec!["unknown_char".into()],
                    dialogues: vec![],
                    duration: Tick(254_016_000_000),
                    transition_out: None,
                    music_hint: None,
                    sfx_hints: vec![],
                }],
            }],
        };

        assert!(matches!(plan.validate(), Err(StoryError::UnknownCharacter { .. })));

        // Fix character ID
        plan.scenes[0].shots[0].characters_present = vec!["c1".into()];
        assert_eq!(plan.validate(), Ok(()));
    }

    #[test]
    fn rejects_non_sequential_shot_numbers() {
        let plan = StoryPlan {
            title: "Test".into(),
            logline: None,
            genre: None,
            target_duration: None,
            characters: vec![],
            scenes: vec![Scene {
                id: "s1".into(),
                scene_number: 1,
                heading: "INT. ROOM - DAY".into(),
                summary: "Summary".into(),
                mood: None,
                shots: vec![Shot {
                    id: "sh1".into(),
                    shot_number: 2, // should be 1
                    shot_type: ShotType::Medium,
                    camera_movement: CameraMovement::Static,
                    description: "Desc".into(),
                    visual_prompt: None,
                    characters_present: vec![],
                    dialogues: vec![],
                    duration: Tick(254_016_000_000),
                    transition_out: None,
                    music_hint: None,
                    sfx_hints: vec![],
                }],
            }],
        };

        assert!(matches!(plan.validate(), Err(StoryError::InvalidShotNumber { .. })));
    }
}
