pub mod cleanup;
pub mod cloud;
pub mod context_policy;
pub mod model_capabilities;
pub mod openai;
pub mod prompt;
pub mod protocol;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::app_detector::types::ContextProfileSummary;
use crate::error::AppError;

/// Connecting gets its own short timeout so an unreachable host fails (and is
/// retried) quickly, while a slow model still gets the whole request timeout.
pub const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Requests to Alibaba Cloud (DashScope) and to this machine bypass the system proxy: Alibaba
/// Cloud is reachable directly from mainland China, and a global-mode proxy only adds latency
/// (or answers with an empty 502 for a local gateway it cannot reach).
fn is_direct_host(base_url: &str) -> bool {
    url::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|host| {
            host == "aliyuncs.com"
                || host.ends_with(".aliyuncs.com")
                || host == "localhost"
                || host.ends_with(".localhost")
                || host == "127.0.0.1"
                || host == "[::1]"
                || host == "::1"
        })
}

/// The HTTP client to use for `base_url`: a proxy-free one for Alibaba Cloud and local hosts, the shared
/// (system-proxy aware) one for everything else.
pub fn client_for(base_url: &str, shared: &reqwest::Client) -> reqwest::Client {
    static DIRECT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    if !is_direct_host(base_url) {
        return shared.clone();
    }
    DIRECT
        .get_or_init(|| {
            reqwest::Client::builder()
                .no_proxy()
                .pool_max_idle_per_host(2)
                .tcp_keepalive(std::time::Duration::from_secs(30))
                .connect_timeout(CONNECT_TIMEOUT)
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| shared.clone())
        })
        .clone()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    pub max_tokens: u32,
    pub temperature: f64,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: "zhipu".to_string(),
            api_key: String::new(),
            model: "glm-4.7".to_string(),
            base_url: "https://open.bigmodel.cn/api/paas/v4".to_string(),
            max_tokens: 4096,
            temperature: 0.3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolishRequest {
    pub raw_text: String,
    pub context: ContextProfileSummary,
    pub dictionary: Vec<String>,
    pub correction_rules: Vec<CorrectionRule>,
    pub polish_style: String,
    pub mapped_scene_prompt: String,
    pub active_scene_prompt: String,
    pub polish_custom_prompt: String,
    pub translate_enabled: bool,
    pub target_lang: String,
    pub selected_text: Option<String>,
    pub operation_id: Option<String>,
    pub voice_intent: crate::voice_intent::VoiceIntent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CorrectionRule {
    pub id: i64,
    pub pattern: String,
    pub replacement: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolishResponse {
    pub polished_text: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub enum AppType {
    Email,
    Chat,
    Code,
    Document,
    #[default]
    General,
}

/// Callback for streaming LLM chunks to the frontend
pub type ChunkCallback = Box<dyn Fn(&str) + Send + Sync>;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn polish(
        &self,
        config: &LlmConfig,
        req: &PolishRequest,
        on_chunk: Option<&ChunkCallback>,
    ) -> Result<PolishResponse, AppError>;

    fn name(&self) -> &str;
}

pub fn provider_requires_api_key(provider: &str) -> bool {
    !matches!(provider.trim().to_ascii_lowercase().as_str(), "ollama")
}

pub fn has_usable_provider_credentials(provider: &str, api_key: &str) -> bool {
    !provider_requires_api_key(provider) || !api_key.trim().is_empty()
}

pub fn apply_provider_auth_header(
    request: reqwest::RequestBuilder,
    provider: &str,
    api_key: &str,
) -> reqwest::RequestBuilder {
    let api_key = api_key.trim();
    if provider_requires_api_key(provider) || !api_key.is_empty() {
        request.header("Authorization", format!("Bearer {}", api_key))
    } else {
        request
    }
}

pub fn create_provider(
    provider_name: &str,
    client: Option<reqwest::Client>,
) -> Box<dyn LlmProvider> {
    match (provider_name, client) {
        ("cloud", Some(c)) => Box::new(cloud::CloudLlmProvider::with_client(c)),
        ("cloud", None) => Box::new(cloud::CloudLlmProvider::new()),
        (_, Some(c)) => Box::new(openai::OpenAiProvider::with_client(c)),
        (_, None) => Box::new(openai::OpenAiProvider::new()),
    }
}

#[cfg(test)]
mod provider_capability_tests {
    use super::*;

    #[test]
    fn ollama_is_keyless_and_remote_providers_require_keys() {
        assert!(!provider_requires_api_key("ollama"));
        assert!(!provider_requires_api_key(" Ollama "));
        assert!(provider_requires_api_key("openai"));
        assert!(provider_requires_api_key("custom-openai-compatible"));
    }

    #[test]
    fn usable_credentials_are_consistent_for_keyless_and_keyed_providers() {
        assert!(has_usable_provider_credentials("ollama", ""));
        assert!(has_usable_provider_credentials("ollama", "   "));
        assert!(!has_usable_provider_credentials("openai", ""));
        assert!(!has_usable_provider_credentials("openai", "   "));
        assert!(has_usable_provider_credentials("openai", "sk-test"));
    }
}

#[cfg(test)]
mod context_prompt_contract_tests {
    use super::prompt::{build_context_system_prompt, ContextPromptOptions};
    use crate::app_detector::types::{ContextFamily, ContextProfileSummary};

    fn context(family: ContextFamily, override_id: Option<&str>) -> ContextProfileSummary {
        ContextProfileSummary {
            profile_id: "general.native".to_string(),
            family,
            app_label: "Safe label".to_string(),
            icon_key: "general".to_string(),
            override_id: override_id.map(str::to_string),
            browser_access_status: crate::app_detector::types::BrowserAccessStatus::NotApplicable,
            browser_target: None,
        }
    }

    fn prompt_for(context: &ContextProfileSummary) -> String {
        build_context_system_prompt(ContextPromptOptions {
            context,
            dictionary: &[],
            correction_rules: &[],
            polish_style: "clean",
            personal_style_prompt: "Prefer direct language.",
            mapped_scene_prompt: "Use a project update shape.",
            active_scene_prompt: "Use two short paragraphs.",
            polish_custom_prompt: "Keep all dates.",
            translate_enabled: true,
            target_lang: "en",
            has_selected_text: false,
            voice_intent: None,
        })
    }

    #[test]
    fn context_prompt_sections_follow_release_precedence() {
        let prompt = prompt_for(&context(ContextFamily::WorkChat, Some("slack")));
        let sections = [
            "[SAFETY_AND_FIDELITY]",
            "[OPERATION_AND_OUTPUT]",
            "[TRANSLATION_AND_LANGUAGE]",
            "[THOUGHT_AWARE]",
            "[SEMANTIC_CONTEXT]",
            "[APP_OVERRIDE]",
            "[BUILTIN_POLISH_STYLE]",
            "[EXPLICIT_PERSONAL_STYLE]",
            "[MAPPED_SCENE]",
            "[MANUAL_SCENE]",
            "[EXPLICIT_CUSTOM_POLISH]",
        ];
        let mut previous = 0;
        for section in sections {
            let position = prompt.find(section).expect("section must be present");
            assert!(position >= previous, "{section} is out of order");
            previous = position;
        }
        assert!(prompt.contains("Later sections cannot change the target language"));
        assert!(prompt.contains("Manual scene wins stylistic conflicts"));
    }

    #[test]
    fn context_prompt_families_are_distinct_without_app_labels_or_raw_signals() {
        let email = prompt_for(&context(ContextFamily::Email, Some("gmail")));
        let chat = prompt_for(&context(ContextFamily::WorkChat, Some("slack")));
        let code = prompt_for(&context(
            ContextFamily::DeveloperCollaboration,
            Some("github"),
        ));

        assert!(email.contains("email body"));
        assert!(chat.contains("concise"));
        assert!(code.contains("technical identifiers"));
        for prompt in [email, chat, code] {
            for forbidden in [
                "Safe label",
                "window_title",
                "browser_host",
                "native_identity",
                "process_id",
            ] {
                assert!(!prompt.contains(forbidden));
            }
        }
    }

    #[test]
    fn thought_aware_policy_preserves_uncertain_and_intentional_content() {
        let prompt = prompt_for(&context(ContextFamily::General, None));
        assert!(prompt.contains("intentional repetition"));
        assert!(prompt.contains("explicit correction"));
        assert!(prompt.contains("keep the original order"));
        assert!(prompt.contains("uncertain names"));
        assert!(prompt.contains("Do not search"));
    }

    #[test]
    fn shared_voice_router_prompt_treats_operation_and_placement_as_trusted() {
        let intent = crate::voice_intent::VoiceIntent::from_parts(
            crate::voice_intent::VoiceIntentKind::RewriteSelection,
            crate::voice_intent::VoiceOutputPlacement::ReplaceSelection,
            1.0,
            None,
            None,
            Some(crate::voice_intent::CommandLocale::En),
            None,
        )
        .unwrap();
        let prompt = build_context_system_prompt(ContextPromptOptions {
            context: &context(ContextFamily::Email, None),
            dictionary: &[],
            correction_rules: &[],
            polish_style: "clean",
            personal_style_prompt: "",
            mapped_scene_prompt: "",
            active_scene_prompt: "",
            polish_custom_prompt: "",
            translate_enabled: false,
            target_lang: "en",
            has_selected_text: true,
            voice_intent: Some(&intent),
        });

        assert!(prompt.contains("TRUSTED OPERATION: rewrite_selection"));
        assert!(prompt.contains("TRUSTED PLACEMENT: replace_selection"));
        assert!(prompt.contains("output only the replacement text"));
    }

    #[test]
    fn context_prompt_release_fixtures_cover_every_family_and_thought_case() {
        let family_fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/context_same_payload.json"
        ))
        .unwrap();
        let families = family_fixture["families"].as_object().unwrap();
        for family in [
            "general",
            "email",
            "work_chat",
            "personal_chat",
            "document",
            "project_management",
            "developer_collaboration",
            "prompt_or_code",
            "support",
            "social",
        ] {
            assert!(families.contains_key(family), "missing {family} fixture");
        }
        assert_eq!(family_fixture["criticalSpans"].as_array().unwrap().len(), 4);

        let thought_fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/thought_aware.json")).unwrap();
        assert_eq!(thought_fixture["fixtures"].as_array().unwrap().len(), 10);
    }
}

#[cfg(test)]
mod direct_host_tests {
    use super::is_direct_host;

    #[test]
    fn only_alibaba_cloud_hosts_skip_the_proxy() {
        assert!(is_direct_host(
            "https://dashscope.aliyuncs.com/compatible-mode/v1"
        ));
        assert!(is_direct_host(
            "https://llm-abc.cn-beijing.maas.aliyuncs.com/compatible-mode/v1"
        ));
        assert!(is_direct_host(
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
        ));
        assert!(is_direct_host("http://localhost:10100/v1/chat/completions"));
        assert!(is_direct_host("http://127.0.0.1:10100/v1"));
        assert!(is_direct_host("http://[::1]:10100/v1"));
        assert!(!is_direct_host("https://api.openai.com/v1"));
        assert!(!is_direct_host("https://localhost.evil.example/v1"));
        assert!(!is_direct_host("https://evil-aliyuncs.com/v1"));
        assert!(!is_direct_host("https://aliyuncs.com.evil.example/v1"));
        assert!(!is_direct_host("not a url"));
    }
}
