//! Tests for Character Library integration in `filmcraft-project`.

use filmcraft_project::{CharacterVoice, ItemId, ItemKind, Label, Project, ProjectCharacter};

#[test]
fn project_without_characters_deserializes_with_empty_map() {
    let proj = Project::new("Old Project");
    let json = serde_json::to_string(&proj).expect("serialize old project");

    // Deserializing without "characters" field must succeed and give empty map
    let reloaded: Project = serde_json::from_str(&json).expect("deserialize old project");
    assert!(reloaded.characters.is_empty());
}

#[test]
fn add_and_retrieve_character() {
    let mut proj = Project::new("Character Test");
    let ch = ProjectCharacter {
        id: "maya".to_string(),
        name: "Maya Vance".to_string(),
        aliases: vec!["pilot".to_string()],
        description: "Spaceship pilot".to_string(),
        reference_item_ids: Vec::new(),
        voice: Some(CharacterVoice { voice_id: "voice-1".to_string(), pitch: 0.0, pace: 1.0, style: None }),
        style_notes: Some("Realistic sci-fi".to_string()),
        provenance: None,
    };

    proj.add_character(ch.clone()).expect("add character");
    assert_eq!(proj.character("maya"), Some(&ch));

    let json = serde_json::to_string_pretty(&proj).expect("serialize");
    let reloaded: Project = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(reloaded.character("maya"), Some(&ch));
}

#[test]
fn character_validation_rejects_empty_id_and_name() {
    let empty_id = ProjectCharacter {
        id: "   ".to_string(),
        name: "Test".to_string(),
        aliases: Vec::new(),
        description: "Desc".to_string(),
        reference_item_ids: Vec::new(),
        voice: None,
        style_notes: None,
        provenance: None,
    };
    assert!(empty_id.validate().is_err());

    let empty_name = ProjectCharacter {
        id: "c1".to_string(),
        name: "".to_string(),
        aliases: Vec::new(),
        description: "Desc".to_string(),
        reference_item_ids: Vec::new(),
        voice: None,
        style_notes: None,
        provenance: None,
    };
    assert!(empty_name.validate().is_err());
}

#[test]
fn validate_characters_checks_reference_item_ids() {
    let mut proj = Project::new("Ref Test");
    let ghost_id = ItemId(9999);

    let ch = ProjectCharacter {
        id: "ghost_ref".to_string(),
        name: "Ghost".to_string(),
        aliases: Vec::new(),
        description: "".to_string(),
        reference_item_ids: vec![ghost_id],
        voice: None,
        style_notes: None,
        provenance: None,
    };

    proj.characters.insert(ch.id.clone(), ch);
    assert!(proj.validate_characters().is_err());

    // Now add the actual item
    let real_id =
        proj.add_item("ref_image.png", Label::Cerulean, ItemKind::Graphic { width: 1920, height: 1080, rate: filmcraft_time::FrameRate::FPS_24 }, None);
    proj.characters.get_mut("ghost_ref").unwrap().reference_item_ids = vec![real_id];
    assert!(proj.validate_characters().is_ok());
}

#[test]
fn remove_character_works() {
    let mut proj = Project::new("Remove Test");
    let ch = ProjectCharacter {
        id: "temp".to_string(),
        name: "Temporary".to_string(),
        aliases: Vec::new(),
        description: "".to_string(),
        reference_item_ids: Vec::new(),
        voice: None,
        style_notes: None,
        provenance: None,
    };

    proj.add_character(ch).unwrap();
    assert!(proj.character("temp").is_some());
    let removed = proj.remove_character("temp");
    assert!(removed.is_some());
    assert!(proj.character("temp").is_none());
}
