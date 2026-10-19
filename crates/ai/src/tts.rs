//! Dialogue synthesis and voice coordination (Layer 2, offline-first with formant voices).

use std::sync::Arc;

use filmcraft_time::Tick;
use filmcraft_tts::{Params, Voice, voice};

use crate::{AiError, AiTransport, ApiKey, CancellationToken, HttpRequest};

/// Synthesized dialogue audio result.
#[derive(Clone, Debug, PartialEq)]
pub struct SynthesizedDialogue {
    pub character_id: String,
    pub text: String,
    pub sample_rate: u32,
    pub samples: Vec<f32>,
    pub duration: Tick,
}

/// Dialogue voice synthesizer routing between built-in formant voices and optional Gemini TTS.
pub struct VoiceDirector {
    api_key: Option<ApiKey>,
    transport: Option<Arc<dyn AiTransport>>,
    base_url: String,
}

impl VoiceDirector {
    /// Create a local-only voice director using built-in offline voices.
    pub fn new_offline() -> Self {
        VoiceDirector { api_key: None, transport: None, base_url: crate::gemini::DEFAULT_GEMINI_BASE_URL.to_string() }
    }

    /// Create an AI-enabled voice director supporting both offline voices and Gemini TTS.
    pub fn new_with_ai(api_key: ApiKey, transport: Arc<dyn AiTransport>) -> Self {
        VoiceDirector { api_key: Some(api_key), transport: Some(transport), base_url: crate::gemini::DEFAULT_GEMINI_BASE_URL.to_string() }
    }

    /// Synthesize speech for a dialogue line.
    ///
    /// Fallback priority:
    /// 1. If voice_id matches a built-in voice (`filmcraft_tts`), synthesizes offline immediately.
    /// 2. If voice_id starts with `gemini:` and AI transport is configured, routes to Gemini TTS.
    /// 3. Otherwise falls back to the default built-in voice ("formant-female" or "formant-male").
    pub fn synthesize(
        &self,
        character_id: &str,
        text: &str,
        voice_id: Option<&str>,
        pace: f64,
        pitch_semitones: f64,
        cancel: &CancellationToken,
    ) -> Result<SynthesizedDialogue, AiError> {
        cancel.check()?;

        let clean_text = text.trim();
        if clean_text.is_empty() {
            return Ok(SynthesizedDialogue {
                character_id: character_id.to_string(),
                text: String::new(),
                sample_rate: filmcraft_tts::SAMPLE_RATE,
                samples: Vec::new(),
                duration: Tick::ZERO,
            });
        }

        let voice_name = voice_id.unwrap_or("basic-female");

        if voice_name.starts_with("gemini:")
            && let (Some(key), Some(trans)) = (&self.api_key, &self.transport)
        {
            return self.synthesize_gemini_tts(character_id, clean_text, voice_name, key, trans.as_ref(), cancel);
        }

        // Offline synthesis via filmcraft-tts
        let v: Box<dyn Voice> =
            voice(voice_name).or_else(|_| voice("basic-female")).map_err(|e| AiError::InvalidResponse(format!("No built-in voice available: {e}")))?;

        let params = Params {
            pace: pace.clamp(filmcraft_tts::MIN_PACE, filmcraft_tts::MAX_PACE),
            semitones: pitch_semitones.clamp(-filmcraft_tts::MAX_SEMITONES, filmcraft_tts::MAX_SEMITONES),
        };

        let audio = v.synthesize(clean_text, &params).map_err(|e| AiError::InvalidResponse(format!("TTS error: {e}")))?;

        let duration_secs = audio.seconds();
        let duration = Tick::from_seconds_f64(duration_secs);

        Ok(SynthesizedDialogue {
            character_id: character_id.to_string(),
            text: clean_text.to_string(),
            sample_rate: audio.sample_rate,
            samples: audio.samples,
            duration,
        })
    }

    fn synthesize_gemini_tts(
        &self,
        character_id: &str,
        text: &str,
        voice_name: &str,
        key: &ApiKey,
        transport: &dyn AiTransport,
        cancel: &CancellationToken,
    ) -> Result<SynthesizedDialogue, AiError> {
        cancel.check()?;

        let target_voice = voice_name.strip_prefix("gemini:").unwrap_or("Puck");
        let url = format!("{}/models/{}:synthesizeSpeech?key={}", self.base_url.trim_end_matches('/'), crate::catalog::GEMINI_TTS.id, key.expose_secret());

        let payload = serde_json::json!({
            "text": text,
            "voice": { "name": target_voice }
        });

        let body_bytes = serde_json::to_vec(&payload).map_err(|e| AiError::Serialization(e.to_string()))?;

        let http_req = HttpRequest::post_json(&url, body_bytes);
        let resp = transport.send(&http_req, cancel)?;
        let resp_text = resp.text()?;

        let val: serde_json::Value = serde_json::from_str(resp_text).map_err(|e| AiError::InvalidResponse(format!("Invalid Gemini TTS response: {e}")))?;

        let b64 = val.get("audioContent").and_then(|a| a.as_str()).ok_or_else(|| AiError::InvalidResponse("Gemini TTS missing 'audioContent'".into()))?;

        let pcm_bytes = crate::multimodal::base64_decode(b64)?;
        // Interpret raw 24kHz 16-bit LE PCM samples
        let mut samples = Vec::with_capacity(pcm_bytes.len() / 2);
        for chunk in pcm_bytes.chunks_exact(2) {
            let sample_i16 = i16::from_le_bytes([chunk[0], chunk[1]]);
            samples.push(sample_i16 as f32 / 32768.0);
        }

        let duration_secs = samples.len() as f64 / 24_000.0;
        let duration = Tick::from_seconds_f64(duration_secs);

        Ok(SynthesizedDialogue { character_id: character_id.to_string(), text: text.to_string(), sample_rate: 24_000, samples, duration })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_synthesis_produces_valid_samples() {
        let director = VoiceDirector::new_offline();
        let cancel = CancellationToken::new();

        let res = director.synthesize("hero", "Hold on, we are coming!", Some("basic-female"), 1.0, 0.0, &cancel).expect("synthesis succeeds");

        assert_eq!(res.character_id, "hero");
        assert_eq!(res.sample_rate, 24_000);
        assert!(!res.samples.is_empty());
        assert!(res.duration.0 > 0);
    }

    #[test]
    fn empty_text_returns_zero_duration() {
        let director = VoiceDirector::new_offline();
        let cancel = CancellationToken::new();

        let res = director.synthesize("hero", "   \n  ", None, 1.0, 0.0, &cancel).expect("empty succeeds");

        assert_eq!(res.duration, Tick::ZERO);
        assert!(res.samples.is_empty());
    }
}
