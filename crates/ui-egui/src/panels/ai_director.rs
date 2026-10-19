//! AI Director panel: script breakdown, character management, AI planning, and sequence generation.
//!
//! Automation ids: `ai.apiKey`, `ai.setKey`, `ai.scriptText`, `ai.plan`, `ai.build`, `ai.charId`, `ai.charName`.

use egui::{Rect, vec2};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::FilmcraftApp;

const PAD: f32 = 14.0;

/// State for the AI Director panel (serde so agents can inspect it).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AiDirectorDraft {
    pub api_key_input: String,
    pub script_input: String,
    pub script_title: String,
    pub plan_mode: String,
    pub plan_json: String,
    pub status_message: String,
    pub new_char_id: String,
    pub new_char_name: String,
    pub new_char_desc: String,
}

impl Default for AiDirectorDraft {
    fn default() -> Self {
        AiDirectorDraft {
            api_key_input: String::new(),
            script_input: "INT. COCKPIT - NIGHT\n\nALICE\nSystems are holding. Initiating descent.\n".to_string(),
            script_title: "Descent".to_string(),
            plan_mode: "fountain".to_string(),
            plan_json: String::new(),
            status_message: "Ready".to_string(),
            new_char_id: String::new(),
            new_char_name: String::new(),
            new_char_desc: String::new(),
        }
    }
}

pub fn show(app: &mut FilmcraftApp, ui: &mut egui::Ui, rect: Rect) {
    let t = app.tokens;
    let mut draft = app.ui.ai_director_draft.clone();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(8.0, 6.0))));
    let ui = &mut child;

    egui::ScrollArea::vertical().id_salt("ai-director-scroll").show(ui, |ui| {
        ui.add_space(PAD);

        // 1. API Key Section
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Gemini API Key:").strong());
            let key_edit = egui::TextEdit::singleline(&mut draft.api_key_input).password(true).desired_width(200.0).hint_text("AIzaSy...");
            ui.add(key_edit);

            if ui.button("Save Key").clicked() && !draft.api_key_input.trim().is_empty() {
                let _ = app.session.execute("ai.setKey", json!({ "key": draft.api_key_input.trim() }));
                draft.status_message = "API key saved in memory".to_string();
            }

            if ui.button("Clear").clicked() {
                let _ = app.session.execute("ai.clearKey", json!({}));
                draft.api_key_input.clear();
                draft.status_message = "API key cleared".to_string();
            }
        });

        ui.add_space(PAD);
        ui.separator();

        // 2. Character Library Section
        ui.label(egui::RichText::new("Character Library").strong());
        ui.horizontal(|ui| {
            ui.label("ID:");
            ui.add(egui::TextEdit::singleline(&mut draft.new_char_id).desired_width(80.0).hint_text("alice"));
            ui.label("Name:");
            ui.add(egui::TextEdit::singleline(&mut draft.new_char_name).desired_width(120.0).hint_text("Alice"));
            ui.label("Desc:");
            ui.add(egui::TextEdit::singleline(&mut draft.new_char_desc).desired_width(160.0).hint_text("Pilot in suit"));

            if ui.button("+ Add Character").clicked() && !draft.new_char_id.trim().is_empty() && !draft.new_char_name.trim().is_empty() {
                let _ = app.session.execute(
                    "ai.characterAdd",
                    json!({
                        "id": draft.new_char_id.trim(),
                        "name": draft.new_char_name.trim(),
                        "description": draft.new_char_desc.trim()
                    }),
                );
                draft.new_char_id.clear();
                draft.new_char_name.clear();
                draft.new_char_desc.clear();
            }
        });

        // Character list
        let characters = app.session.project.characters.clone();
        if !characters.is_empty() {
            ui.indent("char_list", |ui| {
                for (id, ch) in characters {
                    ui.horizontal(|ui| {
                        ui.label(format!("• {} ({}) — {}", ch.name, id, ch.description));
                        if ui.small_button("✕").clicked() {
                            let _ = app.session.execute("ai.characterRemove", json!({ "id": id }));
                        }
                    });
                }
            });
        }

        ui.add_space(PAD);
        ui.separator();

        // 3. Script Input Section
        ui.horizontal(|ui| {
            ui.label("Script Title:");
            ui.add(egui::TextEdit::singleline(&mut draft.script_title).desired_width(200.0));
            ui.label("Mode:");
            ui.radio_value(&mut draft.plan_mode, "fountain".into(), "Fountain (Offline)");
            ui.radio_value(&mut draft.plan_mode, "ai".into(), "Gemini AI");
        });

        ui.label("Script Text (Fountain / Screenplay format):");
        let script_edit = egui::TextEdit::multiline(&mut draft.script_input).desired_width(f32::INFINITY).desired_rows(8);
        ui.add(script_edit);

        ui.add_space(PAD);

        // 4. Action Buttons (Plan & Build)
        ui.horizontal(|ui| {
            if ui.button("1. Plan Script (Generate StoryPlan)").clicked() {
                match app.session.execute(
                    "ai.planScript",
                    json!({
                        "script": draft.script_input,
                        "title": draft.script_title,
                        "mode": draft.plan_mode,
                        "dry_run": false
                    }),
                ) {
                    Ok(plan_val) => {
                        if let Ok(pretty) = serde_json::to_string_pretty(&plan_val) {
                            draft.plan_json = pretty;
                            draft.status_message = "StoryPlan generated successfully".to_string();
                        }
                    }
                    Err(e) => {
                        draft.status_message = format!("Planning error: {e}");
                    }
                }
            }

            if ui.button("2. Build Timeline Sequence").clicked() && !draft.plan_json.trim().is_empty() {
                if let Ok(plan_obj) = serde_json::from_str::<serde_json::Value>(&draft.plan_json) {
                    match app.session.execute(
                        "ai.buildSequence",
                        json!({
                            "plan": plan_obj,
                            "name": draft.script_title
                        }),
                    ) {
                        Ok(res) => {
                            draft.status_message = format!("Sequence built successfully: {}", res);
                        }
                        Err(e) => {
                            draft.status_message = format!("Build error: {e}");
                        }
                    }
                } else {
                    draft.status_message = "Plan JSON is invalid".to_string();
                }
            }
        });

        // Status message
        if !draft.status_message.is_empty() {
            ui.label(egui::RichText::new(&draft.status_message).color(t.accent));
        }

        // Plan JSON preview / editor
        if !draft.plan_json.is_empty() {
            ui.add_space(PAD);
            ui.label(egui::RichText::new("StoryPlan JSON (editable before Build):").strong());
            let plan_edit = egui::TextEdit::multiline(&mut draft.plan_json).desired_width(f32::INFINITY).desired_rows(10);
            ui.add(plan_edit);
        }
    });

    app.ui.ai_director_draft = draft;
}
