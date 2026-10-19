//! Hostile JSON and boundary validation tests for `filmcraft-story`.
//! Ensures parser never crashes, never panics, and returns strict, actionable errors.

use filmcraft_story::{StoryError, parse_plan};

#[test]
fn case_01_empty_string_fails() {
    assert!(parse_plan("").is_err());
    assert!(parse_plan("   \n\t  ").is_err());
}

#[test]
fn case_02_malformed_json_fails() {
    assert!(parse_plan("{ invalid json ").is_err());
    assert!(parse_plan("[1, 2, 3]").is_err());
}

#[test]
fn case_03_empty_plan_fails() {
    let json = r#"{"title": "Test", "scenes": []}"#;
    assert_eq!(parse_plan(json).unwrap_err(), StoryError::EmptyPlan);
}

#[test]
fn case_04_scene_with_no_shots_fails() {
    let json = r#"{
        "title": "Test",
        "scenes": [
            {"id": "sc1", "scene_number": 1, "heading": "INT. TEST", "summary": "", "shots": []}
        ]
    }"#;
    assert_eq!(parse_plan(json).unwrap_err(), StoryError::EmptyScene { scene_id: "sc1".into() });
}

#[test]
fn case_05_negative_duration_fails() {
    let json = r#"{
        "title": "Test",
        "scenes": [{
            "id": "sc1", "scene_number": 1, "heading": "INT. TEST", "summary": "",
            "shots": [{
                "id": "sh1", "shot_number": 1, "shot_type": "Wide", "camera_movement": "Static",
                "description": "test", "duration": -100
            }]
        }]
    }"#;
    match parse_plan(json).unwrap_err() {
        StoryError::InvalidDuration { .. } => {}
        other => panic!("expected InvalidDuration, got {other:?}"),
    }
}

#[test]
fn case_06_zero_duration_fails() {
    let json = r#"{
        "title": "Test",
        "scenes": [{
            "id": "sc1", "scene_number": 1, "heading": "INT. TEST", "summary": "",
            "shots": [{
                "id": "sh1", "shot_number": 1, "shot_type": "Wide", "camera_movement": "Static",
                "description": "test", "duration": 0
            }]
        }]
    }"#;
    match parse_plan(json).unwrap_err() {
        StoryError::InvalidDuration { .. } => {}
        other => panic!("expected InvalidDuration, got {other:?}"),
    }
}

#[test]
fn case_07_unknown_character_id_in_dialogue_fails() {
    let json = r#"{
        "title": "Test",
        "characters": [{"id": "c1", "name": "Alice", "description": ""}],
        "scenes": [{
            "id": "sc1", "scene_number": 1, "heading": "INT. TEST", "summary": "",
            "shots": [{
                "id": "sh1", "shot_number": 1, "shot_type": "Wide", "camera_movement": "Static",
                "description": "test", "duration": 1000,
                "dialogues": [{"character_id": "c_unknown", "text": "Hello"}]
            }]
        }]
    }"#;
    match parse_plan(json).unwrap_err() {
        StoryError::UnknownCharacter { character_id, .. } => assert_eq!(character_id, "c_unknown"),
        other => panic!("expected UnknownCharacter, got {other:?}"),
    }
}

#[test]
fn case_08_unknown_character_in_characters_present_fails() {
    let json = r#"{
        "title": "Test",
        "characters": [{"id": "c1", "name": "Alice", "description": ""}],
        "scenes": [{
            "id": "sc1", "scene_number": 1, "heading": "INT. TEST", "summary": "",
            "shots": [{
                "id": "sh1", "shot_number": 1, "shot_type": "Wide", "camera_movement": "Static",
                "description": "test", "duration": 1000,
                "characters_present": ["ghost_char"]
            }]
        }]
    }"#;
    match parse_plan(json).unwrap_err() {
        StoryError::UnknownCharacter { character_id, .. } => assert_eq!(character_id, "ghost_char"),
        other => panic!("expected UnknownCharacter, got {other:?}"),
    }
}

#[test]
fn case_09_duplicate_character_id_fails() {
    let json = r#"{
        "title": "Test",
        "characters": [
            {"id": "c1", "name": "Alice", "description": ""},
            {"id": "c1", "name": "Bob", "description": ""}
        ],
        "scenes": [{
            "id": "sc1", "scene_number": 1, "heading": "INT. TEST", "summary": "",
            "shots": [{
                "id": "sh1", "shot_number": 1, "shot_type": "Wide", "camera_movement": "Static",
                "description": "test", "duration": 1000
            }]
        }]
    }"#;
    assert_eq!(parse_plan(json).unwrap_err(), StoryError::DuplicateCharacterId("c1".into()));
}

#[test]
fn case_10_duplicate_scene_id_fails() {
    let json = r#"{
        "title": "Test",
        "scenes": [
            {
                "id": "sc1", "scene_number": 1, "heading": "INT. ONE", "summary": "",
                "shots": [{"id": "sh1", "shot_number": 1, "shot_type": "Wide", "camera_movement": "Static", "description": "", "duration": 100}]
            },
            {
                "id": "sc1", "scene_number": 2, "heading": "INT. TWO", "summary": "",
                "shots": [{"id": "sh2", "shot_number": 1, "shot_type": "Wide", "camera_movement": "Static", "description": "", "duration": 100}]
            }
        ]
    }"#;
    assert_eq!(parse_plan(json).unwrap_err(), StoryError::DuplicateSceneId("sc1".into()));
}

#[test]
fn case_11_duplicate_shot_id_fails() {
    let json = r#"{
        "title": "Test",
        "scenes": [{
            "id": "sc1", "scene_number": 1, "heading": "INT. ONE", "summary": "",
            "shots": [
                {"id": "sh_dup", "shot_number": 1, "shot_type": "Wide", "camera_movement": "Static", "description": "", "duration": 100},
                {"id": "sh_dup", "shot_number": 2, "shot_type": "Wide", "camera_movement": "Static", "description": "", "duration": 100}
            ]
        }]
    }"#;
    assert_eq!(parse_plan(json).unwrap_err(), StoryError::DuplicateShotId("sh_dup".into()));
}

#[test]
fn case_12_non_sequential_scene_number_fails() {
    let json = r#"{
        "title": "Test",
        "scenes": [{
            "id": "sc1", "scene_number": 5, "heading": "INT. ONE", "summary": "",
            "shots": [{"id": "sh1", "shot_number": 1, "shot_type": "Wide", "camera_movement": "Static", "description": "", "duration": 100}]
        }]
    }"#;
    assert_eq!(parse_plan(json).unwrap_err(), StoryError::InvalidSceneNumber(5));
}

#[test]
fn case_13_non_sequential_shot_number_fails() {
    let json = r#"{
        "title": "Test",
        "scenes": [{
            "id": "sc1", "scene_number": 1, "heading": "INT. ONE", "summary": "",
            "shots": [{"id": "sh1", "shot_number": 2, "shot_type": "Wide", "camera_movement": "Static", "description": "", "duration": 100}]
        }]
    }"#;
    assert_eq!(parse_plan(json).unwrap_err(), StoryError::InvalidShotNumber { scene_id: "sc1".into(), shot_number: 2 });
}

#[test]
fn case_14_negative_transition_duration_fails() {
    let json = r#"{
        "title": "Test",
        "scenes": [{
            "id": "sc1", "scene_number": 1, "heading": "INT. ONE", "summary": "",
            "shots": [{
                "id": "sh1", "shot_number": 1, "shot_type": "Wide", "camera_movement": "Static", "description": "", "duration": 100,
                "transition_out": {"kind": "Dissolve", "duration": -50}
            }]
        }]
    }"#;
    match parse_plan(json).unwrap_err() {
        StoryError::InvalidDuration { detail, .. } => assert!(detail.contains("negative")),
        other => panic!("expected InvalidDuration, got {other:?}"),
    }
}

#[test]
fn case_15_markdown_fences_stripped_successfully() {
    let json = "```json\n{\n  \"title\": \"Markdown Test\",\n  \"scenes\": [{\n    \"id\": \"sc1\", \"scene_number\": 1, \"heading\": \"INT. TEST\", \"summary\": \"\",\n    \"shots\": [{\"id\": \"sh1\", \"shot_number\": 1, \"shot_type\": \"Wide\", \"camera_movement\": \"Static\", \"description\": \"\", \"duration\": 500000000}]\n  }]\n}\n```";
    let plan = parse_plan(json).expect("stripped markdown parsed successfully");
    assert_eq!(plan.title, "Markdown Test");
}

#[test]
fn case_16_excessive_title_length_fails() {
    let huge_title = "A".repeat(2000);
    let json = format!(
        r#"{{"title": "{huge_title}", "scenes": [{{"id": "sc1", "scene_number": 1, "heading": "H", "summary": "", "shots": [{{"id": "sh1", "shot_number": 1, "shot_type": "Wide", "camera_movement": "Static", "description": "", "duration": 100}}]}}]}}"#
    );
    match parse_plan(&json).unwrap_err() {
        StoryError::TextTooLong { field, .. } => assert_eq!(field, "title"),
        other => panic!("expected TextTooLong, got {other:?}"),
    }
}

#[test]
fn case_17_excessive_logline_length_fails() {
    let huge_logline = "B".repeat(70000);
    let json = format!(
        r#"{{"title": "Valid", "logline": "{huge_logline}", "scenes": [{{"id": "sc1", "scene_number": 1, "heading": "H", "summary": "", "shots": [{{"id": "sh1", "shot_number": 1, "shot_type": "Wide", "camera_movement": "Static", "description": "", "duration": 100}}]}}]}}"#
    );
    match parse_plan(&json).unwrap_err() {
        StoryError::TextTooLong { field, .. } => assert_eq!(field, "logline"),
        other => panic!("expected TextTooLong, got {other:?}"),
    }
}

#[test]
fn case_18_empty_scene_id_fails() {
    let json = r#"{
        "title": "Test",
        "scenes": [{
            "id": "   ", "scene_number": 1, "heading": "INT. ONE", "summary": "",
            "shots": [{"id": "sh1", "shot_number": 1, "shot_type": "Wide", "camera_movement": "Static", "description": "", "duration": 100}]
        }]
    }"#;
    assert!(parse_plan(json).is_err());
}

#[test]
fn case_19_empty_shot_id_fails() {
    let json = r#"{
        "title": "Test",
        "scenes": [{
            "id": "sc1", "scene_number": 1, "heading": "INT. ONE", "summary": "",
            "shots": [{"id": "   ", "shot_number": 1, "shot_type": "Wide", "camera_movement": "Static", "description": "", "duration": 100}]
        }]
    }"#;
    assert!(parse_plan(json).is_err());
}

#[test]
fn case_20_whitespace_only_character_id_fails() {
    let json = r#"{
        "title": "Test",
        "characters": [{"id": "   ", "name": "Space", "description": ""}],
        "scenes": [{
            "id": "sc1", "scene_number": 1, "heading": "INT. ONE", "summary": "",
            "shots": [{"id": "sh1", "shot_number": 1, "shot_type": "Wide", "camera_movement": "Static", "description": "", "duration": 100}]
        }]
    }"#;
    assert!(parse_plan(json).is_err());
}

#[test]
fn case_21_valid_plan_round_trip() {
    let json = r#"{
        "title": "Roundtrip Plan",
        "characters": [{"id": "hero", "name": "Hero", "description": "Protagonist"}],
        "scenes": [{
            "id": "sc1", "scene_number": 1, "heading": "EXT. SUNSET", "summary": "Departure",
            "shots": [{
                "id": "sh1", "shot_number": 1, "shot_type": "Wide", "camera_movement": "Static",
                "description": "Looking out at the horizon", "duration": 254016000000,
                "characters_present": ["hero"],
                "dialogues": [{"character_id": "hero", "text": "It's time to go."}]
            }]
        }]
    }"#;
    let plan = parse_plan(json).expect("valid plan");
    let serialized = plan.to_json_pretty().expect("serialize");
    let reloaded = parse_plan(&serialized).expect("reload");
    assert_eq!(plan, reloaded);
}
