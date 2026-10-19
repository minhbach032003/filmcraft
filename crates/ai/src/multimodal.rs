//! Multimodal media generation: Imagen (images) & Veo (video) (Layer 2, offline-capable).

use std::sync::Arc;
use std::time::Duration;

use crate::{AiError, AiTransport, ApiKey, CancellationToken, CostEstimate, HttpRequest, catalog};

/// Maximum allowed download size for generated images (25 MB).
pub const MAX_IMAGE_BYTES: usize = 25 * 1024 * 1024;
/// Maximum allowed download size for generated video clips (100 MB).
pub const MAX_VIDEO_BYTES: usize = 100 * 1024 * 1024;

/// Aspect ratio specification for media generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AspectRatio {
    Wide16x9,
    Standard4x3,
    Square1x1,
    Vertical9x16,
}

impl AspectRatio {
    pub fn as_str(&self) -> &'static str {
        match self {
            AspectRatio::Wide16x9 => "16:9",
            AspectRatio::Standard4x3 => "4:3",
            AspectRatio::Square1x1 => "1:1",
            AspectRatio::Vertical9x16 => "9:16",
        }
    }
}

/// Request parameters for generating an image via Imagen.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GenerateImageRequest {
    pub prompt: String,
    pub aspect_ratio: AspectRatio,
    pub sample_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_image_bytes: Option<Vec<u8>>,
    pub dry_run: bool,
}

impl GenerateImageRequest {
    pub fn new(prompt: impl Into<String>) -> Self {
        GenerateImageRequest {
            prompt: prompt.into(),
            aspect_ratio: AspectRatio::Wide16x9,
            sample_count: 1,
            negative_prompt: None,
            reference_image_bytes: None,
            dry_run: false,
        }
    }
}

/// Generated image artifact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedImage {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

/// Request parameters for generating a video clip via Veo.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GenerateVideoRequest {
    pub prompt: String,
    pub duration_seconds: u32,
    pub aspect_ratio: AspectRatio,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_image_bytes: Option<Vec<u8>>,
    pub dry_run: bool,
}

impl GenerateVideoRequest {
    pub fn new(prompt: impl Into<String>, duration_seconds: u32) -> Self {
        GenerateVideoRequest {
            prompt: prompt.into(),
            duration_seconds: duration_seconds.clamp(1, 60),
            aspect_ratio: AspectRatio::Wide16x9,
            reference_image_bytes: None,
            dry_run: false,
        }
    }
}

/// Generated video artifact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedVideo {
    pub mime_type: String,
    pub bytes: Vec<u8>,
    pub duration_seconds: u32,
}

/// Multimodal client for Imagen & Veo.
pub struct MultimodalClient {
    api_key: ApiKey,
    transport: Arc<dyn AiTransport>,
    base_url: String,
}

impl MultimodalClient {
    pub fn new(api_key: ApiKey, transport: Arc<dyn AiTransport>) -> Self {
        MultimodalClient { api_key, transport, base_url: crate::gemini::DEFAULT_GEMINI_BASE_URL.to_string() }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Estimate cost for an image generation request.
    pub fn estimate_image_cost(&self, req: &GenerateImageRequest) -> Result<CostEstimate, AiError> {
        let count = req.sample_count.clamp(1, 8);
        let per_image = catalog::IMAGEN_3.pricing.per_image_usd;
        Ok(CostEstimate {
            estimated_input_tokens: 0,
            estimated_output_tokens: 0,
            estimated_images: count,
            estimated_video_seconds: 0.0,
            estimated_cost_usd: count as f64 * per_image,
        })
    }

    /// Estimate cost for a video generation request.
    pub fn estimate_video_cost(&self, req: &GenerateVideoRequest) -> Result<CostEstimate, AiError> {
        let secs = req.duration_seconds.clamp(1, 60) as f64;
        let per_sec = catalog::VEO_2.pricing.per_video_second_usd;
        Ok(CostEstimate {
            estimated_input_tokens: 0,
            estimated_output_tokens: 0,
            estimated_images: 0,
            estimated_video_seconds: secs,
            estimated_cost_usd: secs * per_sec,
        })
    }

    /// Generate an image using Imagen 3.
    pub fn generate_image(&self, req: &GenerateImageRequest, cancel: &CancellationToken) -> Result<GeneratedImage, AiError> {
        cancel.check()?;

        if req.dry_run {
            return Ok(GeneratedImage { mime_type: "image/png".to_string(), bytes: vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] });
        }

        let url = format!("{}/models/{}:predict?key={}", self.base_url.trim_end_matches('/'), catalog::IMAGEN_3.id, self.api_key.expose_secret());

        let mut instance = serde_json::json!({
            "prompt": req.prompt,
        });

        if let Some(ref ref_bytes) = req.reference_image_bytes {
            // Include image reference as base64
            let b64 = base64_encode(ref_bytes);
            instance["image"] = serde_json::json!({ "bytesBase64Encoded": b64 });
        }

        let payload = serde_json::json!({
            "instances": [instance],
            "parameters": {
                "sampleCount": req.sample_count.clamp(1, 4),
                "aspectRatio": req.aspect_ratio.as_str(),
            }
        });

        let body_bytes = serde_json::to_vec(&payload).map_err(|e| AiError::Serialization(e.to_string()))?;

        let http_req = HttpRequest::post_json(&url, body_bytes);
        let resp = self.transport.send(&http_req, cancel)?;
        let text = resp.text()?;

        let val: serde_json::Value = serde_json::from_str(text).map_err(|e| AiError::InvalidResponse(format!("Invalid Imagen JSON: {e}")))?;

        let predictions =
            val.get("predictions").and_then(|p| p.as_array()).ok_or_else(|| AiError::InvalidResponse("Missing 'predictions' in Imagen response".into()))?;

        if predictions.is_empty() {
            return Err(AiError::InvalidResponse("Empty 'predictions' array from Imagen".into()));
        }

        let b64 = predictions[0]
            .get("bytesBase64Encoded")
            .and_then(|b| b.as_str())
            .ok_or_else(|| AiError::InvalidResponse("Prediction missing 'bytesBase64Encoded'".into()))?;

        let image_bytes = base64_decode(b64)?;
        if image_bytes.len() > MAX_IMAGE_BYTES {
            return Err(AiError::InvalidResponse(format!("Generated image too large: {} bytes > limit {}", image_bytes.len(), MAX_IMAGE_BYTES)));
        }

        // Validate basic image magic bytes (PNG, JPEG, WebP)
        let mime = detect_image_mime(&image_bytes).ok_or_else(|| AiError::InvalidResponse("Generated bytes are not a valid PNG/JPEG/WebP image".into()))?;

        Ok(GeneratedImage { mime_type: mime.to_string(), bytes: image_bytes })
    }

    /// Generate a video clip using Veo 2 (handles submit + poll loop).
    pub fn generate_video(&self, req: &GenerateVideoRequest, cancel: &CancellationToken) -> Result<GeneratedVideo, AiError> {
        cancel.check()?;

        if req.dry_run {
            return Ok(GeneratedVideo {
                mime_type: "video/mp4".to_string(),
                bytes: vec![0x00, 0x00, 0x00, 0x18, b'f', b't', b'y', b'p', b'm', b'p', b'4', b'2'],
                duration_seconds: req.duration_seconds,
            });
        }

        let url = format!("{}/models/{}:predictLongRunning?key={}", self.base_url.trim_end_matches('/'), catalog::VEO_2.id, self.api_key.expose_secret());

        let mut instance = serde_json::json!({
            "prompt": req.prompt,
        });

        if let Some(ref ref_bytes) = req.reference_image_bytes {
            let b64 = base64_encode(ref_bytes);
            instance["image"] = serde_json::json!({ "bytesBase64Encoded": b64 });
        }

        let payload = serde_json::json!({
            "instances": [instance],
            "parameters": {
                "durationSeconds": req.duration_seconds,
                "aspectRatio": req.aspect_ratio.as_str(),
            }
        });

        let body_bytes = serde_json::to_vec(&payload).map_err(|e| AiError::Serialization(e.to_string()))?;

        let http_req = HttpRequest::post_json(&url, body_bytes);
        let resp = self.transport.send(&http_req, cancel)?;
        let text = resp.text()?;

        let initial_val: serde_json::Value = serde_json::from_str(text).map_err(|e| AiError::InvalidResponse(format!("Invalid Veo initial response: {e}")))?;

        let operation_name =
            initial_val.get("name").and_then(|n| n.as_str()).ok_or_else(|| AiError::InvalidResponse("Veo response missing operation 'name'".into()))?;

        // Poll operation until completion or cancellation
        let poll_url = format!("{}/{}?key={}", self.base_url.trim_end_matches('/'), operation_name.trim_start_matches('/'), self.api_key.expose_secret());

        let mut attempts = 0u32;
        let poll_interval = Duration::from_millis(500);

        loop {
            cancel.check()?;
            attempts = attempts.saturating_add(1);
            if attempts > 240 {
                return Err(AiError::Network("Veo generation timed out after polling limit".into()));
            }

            std::thread::sleep(poll_interval);
            cancel.check()?;

            let poll_req = HttpRequest::get(&poll_url);
            let poll_resp = self.transport.send(&poll_req, cancel)?;
            let poll_text = poll_resp.text()?;

            let poll_val: serde_json::Value = serde_json::from_str(poll_text).map_err(|e| AiError::InvalidResponse(format!("Invalid Veo poll JSON: {e}")))?;

            if let Some(done) = poll_val.get("done").and_then(|d| d.as_bool())
                && done
            {
                if let Some(err_val) = poll_val.get("error") {
                    return Err(AiError::InvalidResponse(format!("Veo job failed: {err_val}")));
                }

                let response = poll_val.get("response").ok_or_else(|| AiError::InvalidResponse("Completed Veo job missing 'response'".into()))?;

                let b64 = response
                    .get("video")
                    .and_then(|v| v.get("bytesBase64Encoded"))
                    .and_then(|b| b.as_str())
                    .ok_or_else(|| AiError::InvalidResponse("Veo response missing video base64 payload".into()))?;

                let video_bytes = base64_decode(b64)?;
                if video_bytes.len() > MAX_VIDEO_BYTES {
                    return Err(AiError::InvalidResponse(format!("Generated video exceeds max limit: {} bytes > {}", video_bytes.len(), MAX_VIDEO_BYTES)));
                }

                return Ok(GeneratedVideo { mime_type: "video/mp4".to_string(), bytes: video_bytes, duration_seconds: req.duration_seconds });
            }
        }
    }
}

fn detect_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 8 && bytes[0..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        Some("image/png")
    } else if bytes.len() >= 3 && bytes[0..3] == [0xFF, 0xD8, 0xFF] {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

// Minimal pure-Rust base64 helpers to avoid pulling extra crates
const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);

        out.push(B64_ALPHABET[(b0 >> 2) as usize] as char);
        out.push(B64_ALPHABET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(B64_ALPHABET[(((b1 & 0x0F) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64_ALPHABET[(b2 & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn base64_decode(input: &str) -> Result<Vec<u8>, AiError> {
    let mut table = [255u8; 256];
    for (i, &b) in B64_ALPHABET.iter().enumerate() {
        table[b as usize] = i as u8;
    }
    let clean: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if !clean.len().is_multiple_of(4) {
        return Err(AiError::InvalidResponse("Invalid base64 payload length".into()));
    }

    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in clean.chunks(4) {
        let c0 = table[chunk[0] as usize];
        let c1 = table[chunk[1] as usize];
        if c0 == 255 || c1 == 255 {
            return Err(AiError::InvalidResponse("Invalid base64 char".into()));
        }
        out.push((c0 << 2) | (c1 >> 4));
        if chunk[2] != b'=' {
            let c2 = table[chunk[2] as usize];
            if c2 == 255 {
                return Err(AiError::InvalidResponse("Invalid base64 char".into()));
            }
            out.push(((c1 & 0x0F) << 4) | (c2 >> 2));
            if chunk[3] != b'=' {
                let c3 = table[chunk[3] as usize];
                if c3 == 255 {
                    return Err(AiError::InvalidResponse("Invalid base64 char".into()));
                }
                out.push(((c2 & 0x03) << 6) | c3);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FakeTransport;

    #[test]
    fn dry_run_image_and_video() {
        let transport = Arc::new(FakeTransport::new());
        let key = ApiKey::new("dummy").unwrap();
        let client = MultimodalClient::new(key, transport);
        let cancel = CancellationToken::new();

        let mut img_req = GenerateImageRequest::new("Sci-fi ship");
        img_req.dry_run = true;
        let img = client.generate_image(&img_req, &cancel).unwrap();
        assert_eq!(img.mime_type, "image/png");
        assert_eq!(&img.bytes[0..4], &[0x89, b'P', b'N', b'G']);

        let mut vid_req = GenerateVideoRequest::new("Spaceship warp", 5);
        vid_req.dry_run = true;
        let vid = client.generate_video(&vid_req, &cancel).unwrap();
        assert_eq!(vid.mime_type, "video/mp4");
        assert_eq!(vid.duration_seconds, 5);
    }

    #[test]
    fn generate_image_successful_parse() {
        let transport = Arc::new(FakeTransport::new());
        let png_bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00];
        let b64 = base64_encode(&png_bytes);

        let json_body = serde_json::json!({
            "predictions": [{
                "bytesBase64Encoded": b64
            }]
        });

        transport.push_response(Ok(crate::HttpResponse::ok_json(serde_json::to_vec(&json_body).unwrap())));

        let key = ApiKey::new("test").unwrap();
        let client = MultimodalClient::new(key, transport.clone());
        let req = GenerateImageRequest::new("A cute cat");
        let cancel = CancellationToken::new();

        let img = client.generate_image(&req, &cancel).unwrap();
        assert_eq!(img.mime_type, "image/png");
        assert_eq!(img.bytes, png_bytes);
    }

    #[test]
    fn base64_round_trip() {
        let data = b"FilmCraft Pure Rust Clean-Room NLE";
        let encoded = base64_encode(data);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }
}
