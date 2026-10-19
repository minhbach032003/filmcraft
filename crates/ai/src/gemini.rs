//! Google Gemini provider implementation (Layer 2, offline-capable with `FakeTransport`).

use std::sync::Arc;
use std::time::Duration;

use crate::{AiError, AiProvider, AiTransport, ApiKey, CancellationToken, CostEstimate, GenerateRequest, GenerateResponse, Usage, catalog};

/// Default base URL for Google Generative Language API.
pub const DEFAULT_GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

/// Gemini API provider.
pub struct GeminiProvider {
    api_key: ApiKey,
    transport: Arc<dyn AiTransport>,
    base_url: String,
    max_retries: u32,
    initial_backoff_ms: u64,
}

impl GeminiProvider {
    /// Create a new Gemini provider with the given API key and transport.
    pub fn new(api_key: ApiKey, transport: Arc<dyn AiTransport>) -> Self {
        GeminiProvider { api_key, transport, base_url: DEFAULT_GEMINI_BASE_URL.to_string(), max_retries: 3, initial_backoff_ms: 500 }
    }

    /// Override the base URL (useful for custom gateways or testing).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Set the maximum number of retry attempts for 429/5xx errors.
    pub fn with_max_retries(mut self, max: u32) -> Self {
        self.max_retries = max;
        self
    }

    /// Set the initial exponential backoff duration in milliseconds.
    pub fn with_initial_backoff_ms(mut self, ms: u64) -> Self {
        self.initial_backoff_ms = ms;
        self
    }

    /// Calculate pseudo-jitter backoff duration for a given attempt.
    fn calculate_backoff(&self, attempt: u32, retry_after: Option<u64>) -> Duration {
        if let Some(secs) = retry_after {
            return Duration::from_secs(secs);
        }
        // Exponential backoff: base * 2^attempt capped at 30 seconds
        let mult = 1u64.checked_shl(attempt).unwrap_or(32);
        let ms = (self.initial_backoff_ms.saturating_mul(mult)).min(30_000);
        Duration::from_millis(ms)
    }

    /// Build the Gemini request payload for generateContent.
    fn build_payload(req: &GenerateRequest) -> serde_json::Value {
        let parts = vec![serde_json::json!({ "text": req.prompt })];
        let contents = vec![serde_json::json!({
            "role": "user",
            "parts": parts,
        })];

        let mut body = serde_json::json!({
            "contents": contents,
        });

        if let Some(ref sys) = req.system_instruction {
            body["systemInstruction"] = serde_json::json!({
                "parts": [{ "text": sys }]
            });
        }

        let mut gen_config = serde_json::Map::new();
        if let Some(t) = req.temperature {
            gen_config.insert("temperature".to_string(), serde_json::json!(t));
        }
        if let Some(m) = req.max_output_tokens {
            gen_config.insert("maxOutputTokens".to_string(), serde_json::json!(m));
        }
        if let Some(ref schema_str) = req.response_schema {
            gen_config.insert("responseMimeType".to_string(), serde_json::json!("application/json"));
            if let Ok(parsed_schema) = serde_json::from_str::<serde_json::Value>(schema_str) {
                gen_config.insert("responseSchema".to_string(), parsed_schema);
            }
        }

        if !gen_config.is_empty() {
            body["generationConfig"] = serde_json::Value::Object(gen_config);
        }

        body
    }

    /// Parse a raw Gemini response body into GenerateResponse.
    fn parse_response(body_str: &str) -> Result<GenerateResponse, AiError> {
        let val: serde_json::Value = serde_json::from_str(body_str).map_err(|e| AiError::InvalidResponse(format!("Invalid JSON from Gemini: {e}")))?;

        // Check for prompt feedback blocking
        if let Some(block_reason) = val.get("promptFeedback").and_then(|pf| pf.get("blockReason")) {
            return Err(AiError::ContentFiltered(format!("Prompt blocked by Gemini: {block_reason}")));
        }

        let candidates =
            val.get("candidates").and_then(|c| c.as_array()).ok_or_else(|| AiError::InvalidResponse("Missing 'candidates' array in response".into()))?;

        if candidates.is_empty() {
            return Err(AiError::InvalidResponse("Empty 'candidates' array from Gemini".into()));
        }

        let first_candidate = &candidates[0];
        let finish_reason = first_candidate.get("finishReason").and_then(|f| f.as_str()).map(|s| s.to_string());

        if matches!(finish_reason.as_deref(), Some("SAFETY" | "RECITATION")) {
            let reason = finish_reason.unwrap_or_default();
            return Err(AiError::ContentFiltered(format!("Generation stopped due to {reason}")));
        }

        let parts = first_candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
            .ok_or_else(|| AiError::InvalidResponse("Candidate missing content parts".into()))?;

        let mut combined_text = String::new();
        for p in parts {
            if let Some(t) = p.get("text").and_then(|s| s.as_str()) {
                combined_text.push_str(t);
            }
        }

        let mut usage = Usage::default();
        if let Some(meta) = val.get("usageMetadata") {
            if let Some(in_tokens) = meta.get("promptTokenCount").and_then(|v| v.as_u64()) {
                usage.input_tokens = in_tokens.min(u32::MAX as u64) as u32;
            }
            if let Some(out_tokens) = meta.get("candidatesTokenCount").and_then(|v| v.as_u64()) {
                usage.output_tokens = out_tokens.min(u32::MAX as u64) as u32;
            }
        }

        Ok(GenerateResponse { text: combined_text, finish_reason, usage })
    }
}

impl AiProvider for GeminiProvider {
    fn id(&self) -> &'static str {
        "gemini"
    }

    fn generate(&self, req: &GenerateRequest, cancel: &CancellationToken) -> Result<GenerateResponse, AiError> {
        cancel.check()?;

        // If dry run, synthesize a response with zero network interaction
        if req.dry_run {
            let cost = self.estimate_cost(req)?;
            return Ok(GenerateResponse {
                text: "{\"dry_run\": true}".to_string(),
                finish_reason: Some("DRY_RUN".to_string()),
                usage: Usage { input_tokens: cost.estimated_input_tokens, output_tokens: cost.estimated_output_tokens },
            });
        }

        let model_id = if req.model_id.is_empty() { catalog::GEMINI_2_5_FLASH.id } else { &req.model_id };

        // Construct request URL: {base_url}/models/{model_id}:generateContent?key={key}
        // Note: The key is included in the URL for the transport call; errors sanitized by transport.
        let url = format!("{}/models/{}:generateContent?key={}", self.base_url.trim_end_matches('/'), model_id, self.api_key.expose_secret());

        let payload = Self::build_payload(req);
        let body_bytes = serde_json::to_vec(&payload).map_err(|e| AiError::Serialization(format!("Failed to serialize request: {e}")))?;

        let mut attempts = 0;
        loop {
            cancel.check()?;

            let http_req = crate::HttpRequest::post_json(&url, body_bytes.clone());
            match self.transport.send(&http_req, cancel) {
                Ok(resp) => {
                    let text = resp.text()?;
                    return Self::parse_response(text);
                }
                Err(err) => {
                    let (can_retry, retry_after) = match err {
                        AiError::RateLimited { retry_after_secs } => (true, retry_after_secs),
                        AiError::HttpError { status, .. } if (500..=599).contains(&status) => (true, None),
                        AiError::Network(_) => (true, None),
                        _ => (false, None),
                    };

                    if can_retry && attempts < self.max_retries {
                        attempts += 1;
                        let backoff = self.calculate_backoff(attempts, retry_after);
                        // Sleep in small increments checking cancellation
                        let step = Duration::from_millis(50);
                        let mut elapsed = Duration::ZERO;
                        while elapsed < backoff {
                            cancel.check()?;
                            std::thread::sleep(step.min(backoff - elapsed));
                            elapsed += step;
                        }
                        continue;
                    }

                    return Err(err);
                }
            }
        }
    }

    fn estimate_cost(&self, req: &GenerateRequest) -> Result<CostEstimate, AiError> {
        let model_id = if req.model_id.is_empty() { catalog::GEMINI_2_5_FLASH.id } else { &req.model_id };

        let model_info = catalog::find_model(model_id).ok_or_else(|| AiError::ModelNotFound(model_id.to_string()))?;

        // Conservative heuristic: ~4 characters per token
        let input_chars = req.prompt.len() + req.system_instruction.as_deref().unwrap_or("").len() + req.response_schema.as_deref().unwrap_or("").len();
        let estimated_in = ((input_chars / 4).max(1) as u32).min(10_000_000);

        let estimated_out = req.max_output_tokens.unwrap_or(2048).min(32768);

        let in_cost = (estimated_in as f64 / 1_000_000.0) * model_info.pricing.input_usd_per_million;
        let out_cost = (estimated_out as f64 / 1_000_000.0) * model_info.pricing.output_usd_per_million;

        Ok(CostEstimate {
            estimated_input_tokens: estimated_in,
            estimated_output_tokens: estimated_out,
            estimated_images: 0,
            estimated_video_seconds: 0.0,
            estimated_cost_usd: in_cost + out_cost,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FakeTransport;

    #[test]
    fn dry_run_returns_estimate_without_calling_transport() {
        let transport = Arc::new(FakeTransport::new());
        let key = ApiKey::new("dummy-key").unwrap();
        let provider = GeminiProvider::new(key, transport.clone());

        let mut req = GenerateRequest::new("gemini-2.5-flash", "Write a story");
        req.dry_run = true;

        let cancel = CancellationToken::new();
        let res = provider.generate(&req, &cancel).unwrap();

        assert_eq!(res.finish_reason.as_deref(), Some("DRY_RUN"));
        assert!(res.usage.input_tokens > 0);
        assert_eq!(transport.sent_requests().len(), 0);
    }

    #[test]
    fn successful_generate_parses_json() {
        let transport = Arc::new(FakeTransport::new());
        let json_body = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [{ "text": "Once upon a time in FilmCraft..." }]
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 15,
                "candidatesTokenCount": 8
            }
        });

        transport.push_response(Ok(crate::HttpResponse::ok_json(serde_json::to_vec(&json_body).unwrap())));

        let key = ApiKey::new("test-key").unwrap();
        let provider = GeminiProvider::new(key, transport.clone());

        let req = GenerateRequest::new("gemini-2.5-flash", "Tell me a story");
        let cancel = CancellationToken::new();
        let res = provider.generate(&req, &cancel).unwrap();

        assert_eq!(res.text, "Once upon a time in FilmCraft...");
        assert_eq!(res.finish_reason.as_deref(), Some("STOP"));
        assert_eq!(res.usage.input_tokens, 15);
        assert_eq!(res.usage.output_tokens, 8);
    }

    #[test]
    fn retry_on_429_eventually_succeeds() {
        let transport = Arc::new(FakeTransport::new());
        // First 2 calls return 429
        transport.push_response(Err(AiError::RateLimited { retry_after_secs: None }));
        transport.push_response(Err(AiError::RateLimited { retry_after_secs: None }));
        // Third call succeeds
        let json_body = serde_json::json!({
            "candidates": [{
                "content": { "parts": [{ "text": "Recovered!" }] }
            }]
        });
        transport.push_response(Ok(crate::HttpResponse::ok_json(serde_json::to_vec(&json_body).unwrap())));

        let key = ApiKey::new("test-key").unwrap();
        let provider = GeminiProvider::new(key, transport.clone()).with_initial_backoff_ms(1); // fast backoff for test

        let req = GenerateRequest::new("gemini-2.5-flash", "Retry test");
        let cancel = CancellationToken::new();
        let res = provider.generate(&req, &cancel).unwrap();

        assert_eq!(res.text, "Recovered!");
        assert_eq!(transport.sent_requests().len(), 3);
    }

    #[test]
    fn safety_blocks_return_content_filtered() {
        let transport = Arc::new(FakeTransport::new());
        let json_body = serde_json::json!({
            "promptFeedback": {
                "blockReason": "SAFETY"
            }
        });
        transport.push_response(Ok(crate::HttpResponse::ok_json(serde_json::to_vec(&json_body).unwrap())));

        let key = ApiKey::new("test-key").unwrap();
        let provider = GeminiProvider::new(key, transport.clone());

        let req = GenerateRequest::new("gemini-2.5-flash", "unsafe prompt");
        let cancel = CancellationToken::new();
        let err = provider.generate(&req, &cancel).unwrap_err();

        match err {
            AiError::ContentFiltered(msg) => assert!(msg.contains("SAFETY")),
            other => panic!("expected ContentFiltered, got {other:?}"),
        }
    }

    #[test]
    fn hostile_response_empty_candidates_returns_err() {
        let transport = Arc::new(FakeTransport::new());
        let json_body = serde_json::json!({ "candidates": [] });
        transport.push_response(Ok(crate::HttpResponse::ok_json(serde_json::to_vec(&json_body).unwrap())));

        let key = ApiKey::new("test-key").unwrap();
        let provider = GeminiProvider::new(key, transport.clone());

        let req = GenerateRequest::new("gemini-2.5-flash", "test");
        let cancel = CancellationToken::new();
        let err = provider.generate(&req, &cancel).unwrap_err();

        match err {
            AiError::InvalidResponse(msg) => assert!(msg.contains("Empty 'candidates'")),
            other => panic!("expected InvalidResponse, got {other:?}"),
        }
    }
}
