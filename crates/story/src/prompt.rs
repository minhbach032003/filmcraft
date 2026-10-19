//! Story prompt generator and JSON response schema definitions.

use crate::Character;

/// Construct a system instruction and user prompt to instruct Gemini to produce a valid `StoryPlan`.
/// Returns `(user_prompt, system_instruction, response_schema_json)`.
pub fn build_story_prompt(script: &str, characters: &[Character], target_duration_secs: Option<f64>) -> (String, String, String) {
    let mut prompt = String::new();

    prompt.push_str("You are an expert Film Director and Screenplay Editor for FilmCraft.\n");
    prompt.push_str("Analyze the provided screenplay / script text and break it down into a production-ready StoryPlan JSON.\n\n");

    if !characters.is_empty() {
        prompt.push_str("### Available Character Library:\n");
        for ch in characters {
            prompt.push_str(&format!("- ID: \"{}\" | Name: \"{}\"\n  Description: {}\n", ch.id, ch.name, ch.description));
            if let Some(ref style) = ch.style_notes {
                prompt.push_str(&format!("  Style Notes: {}\n", style));
            }
        }
        prompt.push_str("\nRules: Always link dialogue and characters_present using the exact Character ID listed above.\n\n");
    }

    if let Some(secs) = target_duration_secs {
        prompt.push_str(&format!("Target Total Duration: Approximately {:.1} seconds.\n\n", secs));
    }

    prompt.push_str("### Script Text:\n\"\"\"\n");
    prompt.push_str(script.trim());
    prompt.push_str("\n\"\"\"\n\n");

    prompt.push_str("Instructions:\n");
    prompt.push_str("1. Group into numbered scenes starting from 1.\n");
    prompt.push_str("2. Break scenes into cinematic shots numbered sequentially starting from 1.\n");
    prompt.push_str("3. Assign appropriate shot_type (Wide, Medium, CloseUp, etc.) and camera_movement (Static, Tracking, PanLeft, etc.).\n");
    prompt.push_str("4. Duration must be specified in integer ticks (where 1 second = 254016000000 ticks) or approximate duration.\n");

    let system_instruction = "You are a professional film director and sequence editor. You always respond strictly with valid StoryPlan JSON matching the provided schema, with zero conversational commentary.".to_string();

    let response_schema = r#"{
  "type": "object",
  "properties": {
    "title": { "type": "string" },
    "logline": { "type": "string" },
    "genre": { "type": "string" },
    "scenes": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "id": { "type": "string" },
          "scene_number": { "type": "integer" },
          "heading": { "type": "string" },
          "summary": { "type": "string" },
          "shots": {
            "type": "array",
            "items": {
              "type": "object",
              "properties": {
                "id": { "type": "string" },
                "shot_number": { "type": "integer" },
                "shot_type": { "type": "string" },
                "camera_movement": { "type": "string" },
                "description": { "type": "string" },
                "visual_prompt": { "type": "string" },
                "characters_present": { "type": "array", "items": { "type": "string" } },
                "duration": { "type": "integer" },
                "dialogues": {
                  "type": "array",
                  "items": {
                    "type": "object",
                    "properties": {
                      "character_id": { "type": "string" },
                      "text": { "type": "string" },
                      "emotion": { "type": "string" }
                    },
                    "required": ["character_id", "text"]
                  }
                }
              },
              "required": ["id", "shot_number", "shot_type", "camera_movement", "description", "duration"]
            }
          }
        },
        "required": ["id", "scene_number", "heading", "shots"]
      }
    }
  },
  "required": ["title", "scenes"]
}"#
    .to_string();

    (prompt, system_instruction, response_schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_story_prompt_includes_characters_and_script() {
        let chars = vec![Character {
            id: "maya".to_string(),
            name: "Maya Vance".to_string(),
            aliases: vec!["pilot".to_string()],
            description: "Astronaut pilot in orange flight suit".to_string(),
            reference_assets: Vec::new(),
            voice: None,
            style_notes: Some("Grounded realism".to_string()),
            provenance: None,
        }];

        let (prompt, sys, schema) = build_story_prompt("INT. COCKPIT\nMaya checks alarms.", &chars, Some(60.0));

        assert!(prompt.contains("Maya Vance"));
        assert!(prompt.contains("ID: \"maya\""));
        assert!(prompt.contains("Target Total Duration: Approximately 60.0 seconds"));
        assert!(prompt.contains("INT. COCKPIT"));
        assert!(sys.contains("StoryPlan JSON"));
        assert!(schema.contains("\"required\": [\"title\", \"scenes\"]"));
    }
}
