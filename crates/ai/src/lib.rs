//! FilmCraft AI integration: provider abstractions, transport, model catalog, and job coordination.
//!
//! - [`AiTransport`]: low-level HTTP transport trait; implemented by `FakeTransport` in tests
//!   and [`HttpTransport`] when the `net` feature is enabled.
//! - [`AiProvider`]: high-level model provider trait for text planning, story generation, and
//!   multimodal synthesis.
//! - [`ApiKey`]: secret API credential wrapper guaranteeing redaction across Debug, Display, and
//!   serialization.
//! - [`CancellationToken`]: cooperative cancellation handle.
//! - [`catalog`]: pinned metadata for supported AI models (Gemini 2.5, Imagen 3, Veo 2, Kokoro).

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]

use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Environment variable name for the Gemini API key.
pub const ENV_GEMINI_API_KEY: &str = "FILMCRAFT_GEMINI_API_KEY";

/// AI integration errors.
#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum AiError {
    #[error("operation cancelled")]
    Cancelled,
    #[error("API key is missing or empty")]
    ApiKeyMissing,
    #[error("HTTP error {status}: {message}")]
    HttpError { status: u16, message: String },
    #[error("network error: {0}")]
    Network(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("rate limited (retry after {retry_after_secs:?} seconds)")]
    RateLimited { retry_after_secs: Option<u64> },
    #[error("content filtered or blocked: {0}")]
    ContentFiltered(String),
    #[error("model not found: {0}")]
    ModelNotFound(String),
    #[error("invalid response from provider: {0}")]
    InvalidResponse(String),
    #[error("cost estimate ${estimated_cost_usd:.4} exceeds maximum budget ${max_budget_usd:.4}")]
    BudgetExceeded { estimated_cost_usd: f64, max_budget_usd: f64 },
    #[error("I/O error: {0}")]
    Io(String),
}

impl From<serde_json::Error> for AiError {
    fn from(e: serde_json::Error) -> Self {
        AiError::Serialization(e.to_string())
    }
}

/// A confidential API key whose value is redacted from Debug, Display, and serialization.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// Wrap a raw API key string. Returns `Err(AiError::ApiKeyMissing)` if trimmed string is empty.
    pub fn new(key: impl Into<String>) -> Result<Self, AiError> {
        let s = key.into();
        if s.trim().is_empty() {
            return Err(AiError::ApiKeyMissing);
        }
        Ok(ApiKey(s))
    }

    /// Read the API key from the environment variable [`ENV_GEMINI_API_KEY`].
    pub fn from_env() -> Result<Self, AiError> {
        match std::env::var(ENV_GEMINI_API_KEY) {
            Ok(val) => Self::new(val),
            Err(_) => Err(AiError::ApiKeyMissing),
        }
    }

    /// Expose the underlying secret key string for provider authorization headers or query parameters.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey([REDACTED])")
    }
}

impl fmt::Display for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl Serialize for ApiKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str("[REDACTED]")
    }
}

impl<'de> Deserialize<'de> for ApiKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        ApiKey::new(s).map_err(serde::de::Error::custom)
    }
}

/// Cooperative cancellation token.
#[derive(Clone, Default, Debug)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn new() -> Self {
        CancellationToken(Arc::new(AtomicBool::new(false)))
    }

    /// Request cooperative cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Check whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// Return `Err(AiError::Cancelled)` if cancellation was requested, otherwise `Ok(())`.
    pub fn check(&self) -> Result<(), AiError> {
        if self.is_cancelled() { Err(AiError::Cancelled) } else { Ok(()) }
    }
}

/// HTTP request method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

/// An outgoing HTTP request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub timeout_ms: Option<u64>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        HttpRequest { method: HttpMethod::Get, url: url.into(), headers: Vec::new(), body: Vec::new(), timeout_ms: Some(30_000) }
    }

    pub fn post_json(url: impl Into<String>, body: Vec<u8>) -> Self {
        HttpRequest {
            method: HttpMethod::Post,
            url: url.into(),
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body,
            timeout_ms: Some(60_000),
        }
    }
}

/// An incoming HTTP response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn ok_json(body: impl Into<Vec<u8>>) -> Self {
        HttpResponse { status: 200, headers: vec![("content-type".to_string(), "application/json".to_string())], body: body.into() }
    }

    pub fn text(&self) -> Result<&str, AiError> {
        std::str::from_utf8(&self.body).map_err(|e| AiError::InvalidResponse(format!("UTF-8 decoding failed: {e}")))
    }
}

/// Low-level HTTP transport trait.
pub trait AiTransport: Send + Sync {
    /// Send an HTTP request with cancellation checks.
    fn send(&self, req: &HttpRequest, cancel: &CancellationToken) -> Result<HttpResponse, AiError>;
}

/// Scripted mock transport for tests without network calls.
#[derive(Default)]
pub struct FakeTransport {
    responses: Mutex<Vec<Result<HttpResponse, AiError>>>,
    sent_requests: Mutex<Vec<HttpRequest>>,
}

impl FakeTransport {
    pub fn new() -> Self {
        FakeTransport { responses: Mutex::new(Vec::new()), sent_requests: Mutex::new(Vec::new()) }
    }

    pub fn push_response(&self, resp: Result<HttpResponse, AiError>) {
        let mut r = self.responses.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        r.push(resp);
    }

    pub fn sent_requests(&self) -> Vec<HttpRequest> {
        let r = self.sent_requests.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        r.clone()
    }
}

impl AiTransport for FakeTransport {
    fn send(&self, req: &HttpRequest, cancel: &CancellationToken) -> Result<HttpResponse, AiError> {
        cancel.check()?;
        {
            let mut sent = self.sent_requests.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            sent.push(req.clone());
        }
        let mut resp = self.responses.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if resp.is_empty() {
            return Err(AiError::Network("FakeTransport: no response queued".into()));
        }
        resp.remove(0)
    }
}

/// Pure-Rust HTTPS transport using `ureq` and `rustls-rustcrypto`.
#[cfg(feature = "net")]
pub struct HttpTransport {
    agent: ureq::Agent,
}

#[cfg(feature = "net")]
impl Default for HttpTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "net")]
impl HttpTransport {
    pub fn new() -> Self {
        let provider = std::sync::Arc::new(rustls_rustcrypto::provider());
        let agent = ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .provider(ureq::tls::TlsProvider::Rustls)
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .unversioned_rustls_crypto_provider(provider)
                    .build(),
            )
            .timeout_connect(Some(std::time::Duration::from_secs(30)))
            .build()
            .into();
        HttpTransport { agent }
    }
}

#[cfg(feature = "net")]
impl AiTransport for HttpTransport {
    fn send(&self, req: &HttpRequest, cancel: &CancellationToken) -> Result<HttpResponse, AiError> {
        use std::io::Read;

        cancel.check()?;
        let method = match req.method {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
        };

        let mut builder = self.agent.request(method, &req.url);
        for (k, v) in &req.headers {
            builder = builder.header(k, v);
        }

        let resp = if req.body.is_empty() { builder.call() } else { builder.send(&req.body[..]) };

        cancel.check()?;

        let mut resp = resp.map_err(|e| {
            // Strip any sensitive query parameters from logged error strings
            let err_str = e.to_string();
            let sanitized = if let Some(idx) = err_str.find("key=") { format!("{}key=[REDACTED]...", &err_str[..idx]) } else { err_str };
            AiError::Network(sanitized)
        })?;

        let status = resp.status().as_u16();
        let mut body_bytes = Vec::new();
        let mut reader = resp.body_mut().as_reader();
        let mut buf = [0u8; 8192];
        loop {
            cancel.check()?;
            let n = reader.read(&mut buf).map_err(|e| AiError::Network(e.to_string()))?;
            if n == 0 {
                break;
            }
            body_bytes.extend_from_slice(&buf[..n]);
        }

        if status >= 400 {
            let msg = String::from_utf8_lossy(&body_bytes).to_string();
            if status == 429 {
                return Err(AiError::RateLimited { retry_after_secs: None });
            }
            return Err(AiError::HttpError { status, message: msg });
        }

        Ok(HttpResponse { status, headers: Vec::new(), body: body_bytes })
    }
}

/// Model capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelCapability {
    TextPlanning,
    ComplexReasoning,
    ImageGeneration,
    VideoGeneration,
    TextToSpeech,
}

/// Model pricing per unit.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pricing {
    pub input_usd_per_million: f64,
    pub output_usd_per_million: f64,
    pub per_image_usd: f64,
    pub per_video_second_usd: f64,
}

impl Pricing {
    pub const fn zero() -> Self {
        Pricing { input_usd_per_million: 0.0, output_usd_per_million: 0.0, per_image_usd: 0.0, per_video_second_usd: 0.0 }
    }
}

/// Model metadata and pricing.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ModelInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub provider: &'static str,
    pub capabilities: &'static [ModelCapability],
    pub context_window: u32,
    pub pricing: Pricing,
}

pub mod gemini;
pub mod multimodal;

pub use gemini::GeminiProvider;
pub use multimodal::{AspectRatio, GenerateImageRequest, GenerateVideoRequest, GeneratedImage, GeneratedVideo, MultimodalClient};

pub mod catalog {
    use super::*;

    pub const GEMINI_2_5_FLASH: ModelInfo = ModelInfo {
        id: "gemini-2.5-flash",
        name: "Gemini 2.5 Flash",
        provider: "Google",
        capabilities: &[ModelCapability::TextPlanning],
        context_window: 1_048_576,
        pricing: Pricing { input_usd_per_million: 0.10, output_usd_per_million: 0.40, per_image_usd: 0.0, per_video_second_usd: 0.0 },
    };

    pub const GEMINI_2_5_PRO: ModelInfo = ModelInfo {
        id: "gemini-2.5-pro",
        name: "Gemini 2.5 Pro",
        provider: "Google",
        capabilities: &[ModelCapability::TextPlanning, ModelCapability::ComplexReasoning],
        context_window: 2_097_152,
        pricing: Pricing { input_usd_per_million: 1.25, output_usd_per_million: 5.00, per_image_usd: 0.0, per_video_second_usd: 0.0 },
    };

    pub const IMAGEN_3: ModelInfo = ModelInfo {
        id: "imagen-3.0-generate-002",
        name: "Imagen 3",
        provider: "Google",
        capabilities: &[ModelCapability::ImageGeneration],
        context_window: 0,
        pricing: Pricing { input_usd_per_million: 0.0, output_usd_per_million: 0.0, per_image_usd: 0.030, per_video_second_usd: 0.0 },
    };

    pub const VEO_2: ModelInfo = ModelInfo {
        id: "veo-2.0-generate-001",
        name: "Veo 2",
        provider: "Google",
        capabilities: &[ModelCapability::VideoGeneration],
        context_window: 0,
        pricing: Pricing { input_usd_per_million: 0.0, output_usd_per_million: 0.0, per_image_usd: 0.0, per_video_second_usd: 0.20 },
    };

    pub const GEMINI_TTS: ModelInfo = ModelInfo {
        id: "gemini-2.5-flash-tts",
        name: "Gemini Flash TTS",
        provider: "Google",
        capabilities: &[ModelCapability::TextToSpeech],
        context_window: 0,
        pricing: Pricing { input_usd_per_million: 15.00, output_usd_per_million: 0.0, per_image_usd: 0.0, per_video_second_usd: 0.0 },
    };

    pub const KOKORO_82M: ModelInfo = ModelInfo {
        id: "kokoro-82m",
        name: "Kokoro 82M (Local)",
        provider: "Local (Candle)",
        capabilities: &[ModelCapability::TextToSpeech],
        context_window: 0,
        pricing: Pricing::zero(),
    };

    pub const MODELS: &[ModelInfo] = &[GEMINI_2_5_FLASH, GEMINI_2_5_PRO, IMAGEN_3, VEO_2, GEMINI_TTS, KOKORO_82M];

    /// Retrieve the default list of supported models.
    pub fn default_catalog() -> &'static [ModelInfo] {
        MODELS
    }

    /// Find a model by its identifier.
    pub fn find_model(id: &str) -> Option<&'static ModelInfo> {
        MODELS.iter().find(|m| m.id == id)
    }
}

/// Token and USD cost estimation for an operation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CostEstimate {
    pub estimated_input_tokens: u32,
    pub estimated_output_tokens: u32,
    pub estimated_images: u32,
    pub estimated_video_seconds: f64,
    pub estimated_cost_usd: f64,
}

/// Generation request options.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenerateRequest {
    pub model_id: String,
    pub prompt: String,
    pub system_instruction: Option<String>,
    pub response_schema: Option<String>,
    pub temperature: Option<f32>,
    pub max_output_tokens: Option<u32>,
    pub dry_run: bool,
}

impl GenerateRequest {
    pub fn new(model_id: impl Into<String>, prompt: impl Into<String>) -> Self {
        GenerateRequest {
            model_id: model_id.into(),
            prompt: prompt.into(),
            system_instruction: None,
            response_schema: None,
            temperature: None,
            max_output_tokens: None,
            dry_run: false,
        }
    }
}

/// Usage statistics from a generation request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Generation response payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerateResponse {
    pub text: String,
    pub finish_reason: Option<String>,
    pub usage: Usage,
}

/// High-level AI provider trait.
pub trait AiProvider: Send + Sync {
    /// Provider identifier (e.g. "gemini", "mock").
    fn id(&self) -> &'static str;

    /// Execute a generation request.
    fn generate(&self, req: &GenerateRequest, cancel: &CancellationToken) -> Result<GenerateResponse, AiError>;

    /// Estimate cost before dispatching request.
    fn estimate_cost(&self, req: &GenerateRequest) -> Result<CostEstimate, AiError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_redacts_in_debug_and_display() {
        let key = ApiKey::new("secret_gemini_key_12345").expect("valid key");
        assert_eq!(format!("{key:?}"), "ApiKey([REDACTED])");
        assert_eq!(format!("{key}"), "[REDACTED]");
        assert_eq!(key.expose_secret(), "secret_gemini_key_12345");

        let json = serde_json::to_string(&key).expect("json serialize");
        assert_eq!(json, "\"[REDACTED]\"");
    }

    #[test]
    fn api_key_rejects_empty() {
        assert_eq!(ApiKey::new("   "), Err(AiError::ApiKeyMissing));
    }

    #[test]
    fn cancellation_token_triggers() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        assert_eq!(token.check(), Ok(()));

        let cloned = token.clone();
        cloned.cancel();

        assert!(token.is_cancelled());
        assert_eq!(token.check(), Err(AiError::Cancelled));
    }

    #[test]
    fn fake_transport_delivers_and_records() {
        let transport = FakeTransport::new();
        transport.push_response(Ok(HttpResponse::ok_json(b"{\"result\":\"ok\"}")));

        let cancel = CancellationToken::new();
        let req = HttpRequest::get("https://example.com/api");
        let resp = transport.send(&req, &cancel).expect("response");

        assert_eq!(resp.status, 200);
        assert_eq!(resp.text().expect("utf8"), "{\"result\":\"ok\"}");

        let sent = transport.sent_requests();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].url, "https://example.com/api");
    }

    #[test]
    fn catalog_finds_models() {
        let flash = catalog::find_model("gemini-2.5-flash").expect("found flash");
        assert_eq!(flash.provider, "Google");
        assert!(flash.capabilities.contains(&ModelCapability::TextPlanning));

        let kokoro = catalog::find_model("kokoro-82m").expect("found kokoro");
        assert_eq!(kokoro.pricing.input_usd_per_million, 0.0);

        assert!(catalog::find_model("nonexistent").is_none());
    }
}
