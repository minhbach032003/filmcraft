//! Project Character Library (Layer 1, serialization & project file persistence).

use serde::{Deserialize, Serialize};

use crate::ItemId;

/// Maximum character name or alias length in bytes.
pub const MAX_CHARACTER_NAME_BYTES: usize = 256;
/// Maximum character description length in bytes.
pub const MAX_CHARACTER_DESC_BYTES: usize = 16 * 1024;
/// Maximum style notes length in bytes.
pub const MAX_CHARACTER_STYLE_BYTES: usize = 16 * 1024;

/// Provenance metadata tracking model generation origin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterProvenance {
    pub creator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

/// Voice synthesis configuration for character dialogue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharacterVoice {
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

/// A character specification stored within the project document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectCharacter {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    pub description: String,
    /// Reference media assets in the project used as consistency anchors for Imagen / Veo.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_item_ids: Vec<ItemId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<CharacterVoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<CharacterProvenance>,
}

impl ProjectCharacter {
    /// Validate character fields against bounds.
    pub fn validate(&self) -> Result<(), String> {
        let trimmed_id = self.id.trim();
        if trimmed_id.is_empty() {
            return Err("Character ID cannot be empty".to_string());
        }
        if self.name.trim().is_empty() {
            return Err(format!("Character '{}' name cannot be empty", self.id));
        }
        if self.name.len() > MAX_CHARACTER_NAME_BYTES {
            return Err(format!("Character '{}' name exceeds length limit", self.id));
        }
        for alias in &self.aliases {
            if alias.len() > MAX_CHARACTER_NAME_BYTES {
                return Err(format!("Character '{}' alias '{}' exceeds length limit", self.id, alias));
            }
        }
        if self.description.len() > MAX_CHARACTER_DESC_BYTES {
            return Err(format!("Character '{}' description exceeds length limit", self.id));
        }
        if let Some(ref s) = self.style_notes
            && s.len() > MAX_CHARACTER_STYLE_BYTES
        {
            return Err(format!("Character '{}' style notes exceed length limit", self.id));
        }
        Ok(())
    }
}
