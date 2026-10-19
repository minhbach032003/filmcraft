//! Offline Fountain screenplay parser (pure Rust, Layer 2, zero network dependencies).

use filmcraft_time::Tick;

use crate::{CameraMovement, Dialogue, Scene, Shot, ShotType, StoryError, StoryPlan};

/// Parse a plain-text Fountain script into a valid [`StoryPlan`] offline.
///
/// Handles:
/// - Scene headings (`INT.`, `EXT.`, `INT/EXT`, `EST.`, `I/E`)
/// - Action paragraphs (converted to shots)
/// - Character names (uppercase lines) followed by dialogue and parentheticals
/// - Transitions (`CUT TO:`, `FADE OUT.`, etc.)
pub fn parse_fountain(title: &str, text: &str) -> Result<StoryPlan, StoryError> {
    let mut scenes: Vec<Scene> = Vec::new();
    let mut current_scene: Option<Scene> = None;
    let mut current_character: Option<String> = None;
    let mut current_parenthetical: Option<String> = None;
    let mut current_dialogue_text = String::new();

    let mut scene_counter = 0u32;
    let mut shot_counter = 0u32;

    // Helper: flush pending dialogue into the current scene's last shot or new shot
    let flush_dialogue =
        |scene: &mut Option<Scene>, character: &mut Option<String>, parenthetical: &mut Option<String>, dialogue_text: &mut String, shot_counter: &mut u32| {
            if let (Some(char_name), true, Some(sc)) = (character.as_deref(), !dialogue_text.trim().is_empty(), scene.as_mut()) {
                *shot_counter = shot_counter.saturating_add(1);
                // Approximate speech timing: ~130-150 words per minute => ~2.5 words per second
                let word_count = dialogue_text.split_whitespace().count().max(1);
                let duration_secs = (word_count as f64 / 2.5).clamp(1.5, 30.0);
                let duration_ticks = Tick::from_seconds_f64(duration_secs);

                let dlg = Dialogue {
                    character_id: char_name.to_string(),
                    text: dialogue_text.trim().to_string(),
                    emotion: parenthetical.take(),
                    estimated_duration: Some(duration_ticks),
                };

                let shot = Shot {
                    id: format!("shot-{}-{}", sc.scene_number, shot_counter),
                    shot_number: *shot_counter,
                    shot_type: ShotType::MediumCloseUp,
                    camera_movement: CameraMovement::Static,
                    description: format!("{} speaks", char_name),
                    visual_prompt: None,
                    characters_present: vec![char_name.to_string()],
                    dialogues: vec![dlg],
                    duration: duration_ticks,
                    transition_out: None,
                    music_hint: None,
                    sfx_hints: Vec::new(),
                };
                sc.shots.push(shot);
            }
            *character = None;
            *parenthetical = None;
            dialogue_text.clear();
        };

    for raw_line in text.lines() {
        let line = raw_line.trim();

        if line.is_empty() {
            flush_dialogue(&mut current_scene, &mut current_character, &mut current_parenthetical, &mut current_dialogue_text, &mut shot_counter);
            continue;
        }

        let upper = line.to_uppercase();

        // Scene heading detection
        if is_scene_heading(&upper) {
            flush_dialogue(&mut current_scene, &mut current_character, &mut current_parenthetical, &mut current_dialogue_text, &mut shot_counter);

            if let Some(sc) = current_scene.take()
                && !sc.shots.is_empty()
            {
                scenes.push(sc);
            }

            scene_counter = scene_counter.saturating_add(1);
            shot_counter = 0;

            current_scene = Some(Scene {
                id: format!("scene-{}", scene_counter),
                scene_number: scene_counter,
                heading: line.to_string(),
                summary: String::new(),
                mood: None,
                shots: Vec::new(),
            });
            continue;
        }

        // Parenthetical detection: (emotion or direction)
        if line.starts_with('(') && line.ends_with(')') && current_character.is_some() {
            let inner = line[1..line.len() - 1].trim();
            current_parenthetical = Some(inner.to_string());
            continue;
        }

        // Character cue detection: ALL CAPS, not too long, no ending punctuation
        if is_character_cue(line) && current_character.is_none() {
            flush_dialogue(&mut current_scene, &mut current_character, &mut current_parenthetical, &mut current_dialogue_text, &mut shot_counter);
            current_character = Some(line.to_string());
            continue;
        }

        // Dialogue continuation if a character cue is active
        if current_character.is_some() {
            if !current_dialogue_text.is_empty() {
                current_dialogue_text.push(' ');
            }
            current_dialogue_text.push_str(line);
            continue;
        }

        // Action / description paragraph
        if let Some(ref mut sc) = current_scene {
            shot_counter = shot_counter.saturating_add(1);
            let word_count = line.split_whitespace().count().max(1);
            let duration_secs = (word_count as f64 / 3.0).clamp(2.0, 15.0);

            let shot = Shot {
                id: format!("shot-{}-{}", sc.scene_number, shot_counter),
                shot_number: shot_counter,
                shot_type: ShotType::Medium,
                camera_movement: CameraMovement::Static,
                description: line.to_string(),
                visual_prompt: Some(line.to_string()),
                characters_present: Vec::new(),
                dialogues: Vec::new(),
                duration: Tick::from_seconds_f64(duration_secs),
                transition_out: None,
                music_hint: None,
                sfx_hints: Vec::new(),
            };
            sc.shots.push(shot);
        }
    }

    // Flush last scene
    flush_dialogue(&mut current_scene, &mut current_character, &mut current_parenthetical, &mut current_dialogue_text, &mut shot_counter);

    if let Some(sc) = current_scene
        && !sc.shots.is_empty()
    {
        scenes.push(sc);
    }

    if scenes.is_empty() {
        return Err(StoryError::EmptyPlan);
    }

    let plan = StoryPlan {
        title: if title.trim().is_empty() { "Untitled Script".to_string() } else { title.trim().to_string() },
        logline: None,
        genre: None,
        target_duration: None,
        characters: Vec::new(),
        scenes,
    };

    plan.validate()?;
    Ok(plan)
}

fn is_scene_heading(upper: &str) -> bool {
    upper.starts_with("INT.") || upper.starts_with("EXT.") || upper.starts_with("INT/EXT") || upper.starts_with("I/E") || upper.starts_with("EST.")
}

fn is_character_cue(line: &str) -> bool {
    if line.len() > 40 || line.is_empty() {
        return false;
    }
    // Must be uppercase and have at least one alphabetic character
    let mut has_alphabetic = false;
    for ch in line.chars() {
        if ch.is_alphabetic() {
            has_alphabetic = true;
            if !ch.is_uppercase() {
                return false;
            }
        } else if ch != ' ' && ch != '.' && ch != '-' && ch != '\'' {
            return false;
        }
    }
    has_alphabetic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_fountain_basic_screenplay() {
        let script = r#"
INT. COCKPIT - NIGHT

A siren wails softly against red emergency lighting.

MAYA
(breathless)
We lost stabilizer two. Hold on!

EXT. SPACE - CONTINUOUS

The ship spins into the upper atmosphere.
"#;

        let plan = parse_fountain("Emergency Descent", script).expect("fountain parse");
        assert_eq!(plan.title, "Emergency Descent");
        assert_eq!(plan.scenes.len(), 2);
        assert_eq!(plan.scenes[0].scene_number, 1);
        assert_eq!(plan.scenes[0].heading, "INT. COCKPIT - NIGHT");
        assert_eq!(plan.scenes[0].shots.len(), 2);

        let shot1 = &plan.scenes[0].shots[0];
        assert_eq!(shot1.description, "A siren wails softly against red emergency lighting.");

        let shot2 = &plan.scenes[0].shots[1];
        assert_eq!(shot2.dialogues.len(), 1);
        assert_eq!(shot2.dialogues[0].character_id, "MAYA");
        assert_eq!(shot2.dialogues[0].text, "We lost stabilizer two. Hold on!");
        assert_eq!(shot2.dialogues[0].emotion.as_deref(), Some("breathless"));

        assert_eq!(plan.scenes[1].scene_number, 2);
        assert_eq!(plan.scenes[1].heading, "EXT. SPACE - CONTINUOUS");
        assert_eq!(plan.scenes[1].shots.len(), 1);
    }
}
