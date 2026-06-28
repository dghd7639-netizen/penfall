use crate::audio::{AudioCaptureHandle, AudioConfig};
use crate::pipeline::PipelineState;
use crate::storage;
use crate::stt::{self, SttConfig, TranscriptEvent};
use crate::{api_base_url, with_desktop_client_version, SessionTokenStore};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tauri::Emitter;
use tokio::sync::Notify;

pub const ASK_MAX_QUESTION_CHARS: usize = 500;
pub const ASK_OUTPUT_TOKEN_LIMIT: u32 = 80;
const ASK_STT_FINALIZE_TIMEOUT_SECS: u64 = 120;

#[derive(Default)]
pub struct AskDictationState(Arc<Mutex<AskDictationStateInner>>);

#[derive(Default)]
struct AskDictationStateInner {
    session: Option<AskDictationSession>,
    processing: bool,
}

impl AskDictationState {
    pub fn is_recording(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .session
            .is_some()
    }

    pub fn is_busy(&self) -> bool {
        let guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        guard.session.is_some() || guard.processing
    }

    fn set_processing(&self, processing: bool) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .processing = processing;
    }
}

pub struct AskDictationSession {
    handle: AudioCaptureHandle,
    operation_id: String,
    transcript: Arc<Mutex<String>>,
    error: Arc<Mutex<Option<String>>>,
    done: Arc<Notify>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskDictationResult {
    question: String,
    answer: String,
}

fn emit_capsule_state(app: &tauri::AppHandle, state: PipelineState) {
    let _ = app.emit("pipeline:state", state);
}

fn synthetic_operation_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (now >> 96) as u32,
        (now >> 80) as u16,
        (now >> 64) as u16,
        (now >> 48) as u16,
        now & 0x0000_ffff_ffff_ffff_ffffu128
    )
}

pub fn validate_ask_question(question: &str) -> Result<String, String> {
    let trimmed = question.trim().to_string();
    if trimmed.is_empty() {
        return Err("Question is required".to_string());
    }
    if trimmed.chars().count() > ASK_MAX_QUESTION_CHARS {
        return Err(format!(
            "Question is too long (max {} characters)",
            ASK_MAX_QUESTION_CHARS
        ));
    }
    Ok(trimmed)
}

fn ask_messages(question: &str) -> Vec<serde_json::Value> {
    vec![
        json!({
            "role": "system",
            "content": "Answer clearly and directly in the same language as the user. Keep the answer under 40 words. Do not use web search, external browsing, or selected-text context."
        }),
        json!({ "role": "user", "content": question }),
    ]
}

pub fn build_byok_ask_body(question: &str, model: &str) -> Result<serde_json::Value, String> {
    let question = validate_ask_question(question)?;
    Ok(json!({
        "model": model,
        "messages": ask_messages(&question),
        "max_tokens": ASK_OUTPUT_TOKEN_LIMIT,
        "temperature": 0.2,
        "stream": false
    }))
}

fn should_use_byok(config: &storage::AppConfig) -> bool {
    if config.llm_provider == "cloud" {
        return false;
    }
    if config.llm_base_url.trim().is_empty() || config.llm_model.trim().is_empty() {
        return false;
    }
    !config.llm_api_key.trim().is_empty() || config.llm_provider == "ollama"
}

fn should_use_cloud(config: &storage::AppConfig) -> bool {
    config.llm_provider == "cloud"
}

fn build_ask_stt_config(
    config: &storage::AppConfig,
    api_key: String,
    operation_id: String,
) -> SttConfig {
    SttConfig {
        api_key,
        language: if config.stt_language == "multi" {
            None
        } else {
            Some(config.stt_language.clone())
        },
        smart_format: true,
        sample_rate: 16000,
        resource_id: if config.stt_provider == stt::volcengine::VOLCENGINE_DOUBAO_PROVIDER {
            Some(config.stt_volcengine_resource_id.clone())
        } else {
            None
        },
        operation_id: Some(operation_id),
    }
}

fn ask_stt_api_key(config: &storage::AppConfig, token_store: &SessionTokenStore) -> String {
    if config.stt_provider == "cloud" {
        return token_store
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
    }
    if config.stt_provider == stt::config::CUSTOM_WHISPER_PROVIDER {
        return config.stt_custom_api_key.clone();
    }
    config.stt_api_key.clone()
}

fn append_final_transcript(transcript: &Arc<Mutex<String>>, text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return transcript.lock().unwrap_or_else(|e| e.into_inner()).clone();
    }

    let mut current = transcript.lock().unwrap_or_else(|e| e.into_inner());
    if !current.trim().is_empty() && !current.ends_with(' ') {
        current.push(' ');
    }
    current.push_str(text);
    current.trim().to_string()
}

async fn answer_question(
    config: &storage::AppConfig,
    client: &reqwest::Client,
    token_store: &SessionTokenStore,
    question: &str,
    operation_id: Option<&str>,
) -> Result<String, String> {
    if should_use_byok(config) {
        return ask_via_byok(client, config, question).await;
    }

    if should_use_cloud(config) {
        return ask_via_cloud(client, token_store, question, operation_id).await;
    }

    Err("Configure a BYOK LLM provider or choose Cloud LLM to use Ask.".to_string())
}

fn response_error(status: reqwest::StatusCode, text: String) -> String {
    let sanitized: String = text.chars().take(200).collect();
    format!("Ask request failed ({}): {}", status.as_u16(), sanitized)
}

async fn ask_via_byok(
    client: &reqwest::Client,
    config: &storage::AppConfig,
    question: &str,
) -> Result<String, String> {
    let parsed =
        url::Url::parse(&config.llm_base_url).map_err(|e| format!("Invalid LLM base URL: {e}"))?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err("LLM base URL must use http or https scheme".to_string());
    }

    let url = format!(
        "{}/chat/completions",
        config.llm_base_url.trim_end_matches('/')
    );
    let mut request = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&build_byok_ask_body(question, &config.llm_model)?)
        .timeout(std::time::Duration::from_secs(30));

    if !config.llm_api_key.trim().is_empty() {
        request = request.header("Authorization", format!("Bearer {}", config.llm_api_key));
    }

    let resp = request.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(response_error(status, text));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string())
}

async fn ask_via_cloud(
    client: &reqwest::Client,
    token_store: &SessionTokenStore,
    question: &str,
    operation_id: Option<&str>,
) -> Result<String, String> {
    let token = token_store
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if token.trim().is_empty() {
        return Err("Configure a BYOK LLM provider or choose Cloud LLM to use Ask.".to_string());
    }

    let operation_id = operation_id
        .map(str::to_string)
        .unwrap_or_else(synthetic_operation_id);
    let stage_key = format!("{operation_id}:ask");
    let body = json!({
        "question": question,
        "context": {
            "operationId": operation_id,
            "stageKey": stage_key,
            "requestType": "ask_anything",
            "clientVersion": crate::desktop_client_version()
        }
    });

    let resp =
        with_desktop_client_version(client.post(format!("{}/api/proxy/ask", api_base_url())))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(45))
            .send()
            .await
            .map_err(|e| e.to_string())?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(response_error(status, text));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(body["answer"].as_str().unwrap_or("").trim().to_string())
}

#[tauri::command]
pub async fn ask_anything(
    question: String,
    config_state: tauri::State<'_, storage::ConfigManager>,
    token_store: tauri::State<'_, SessionTokenStore>,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<String, String> {
    let question = validate_ask_question(&question)?;
    let config = config_state.load().await.map_err(|e| e.to_string())?;

    answer_question(&config, &client, &token_store, &question, None).await
}

#[tauri::command]
pub async fn start_ask_dictation(
    app: tauri::AppHandle,
    state: tauri::State<'_, AskDictationState>,
    config_state: tauri::State<'_, storage::ConfigManager>,
    token_store: tauri::State<'_, SessionTokenStore>,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<(), String> {
    if state.is_busy() {
        return Ok(());
    }

    let config = config_state.load().await.map_err(|e| e.to_string())?;
    let stt_api_key = ask_stt_api_key(&config, &token_store);
    if stt::config::stt_provider_requires_api_key(&config.stt_provider) && stt_api_key.is_empty() {
        return Err(
            "STT API key is not configured. Please set it in Settings -> Speech Recognition."
                .to_string(),
        );
    }

    let custom_whisper_config = if config.stt_provider == stt::config::CUSTOM_WHISPER_PROVIDER {
        Some(stt::config::build_custom_whisper_config(
            &config.stt_custom_base_url,
            &config.stt_custom_model,
        )?)
    } else {
        None
    };
    let operation_id = synthetic_operation_id();
    let stt_config = build_ask_stt_config(&config, stt_api_key, operation_id.clone());
    let mut provider = stt::create_provider(
        &config.stt_provider,
        custom_whisper_config,
        Some(client.inner().clone()),
    )
    .map_err(|e| e.to_string())?;
    provider
        .connect(&stt_config)
        .await
        .map_err(|e| e.to_string())?;

    let (mut handle, mut audio_rx) =
        AudioCaptureHandle::start(AudioConfig::default()).map_err(|e| e.to_string())?;
    let transcript = Arc::new(Mutex::new(String::new()));
    let error = Arc::new(Mutex::new(None::<String>));
    let done = Arc::new(Notify::new());

    {
        let mut guard = state.0.lock().unwrap_or_else(|e| e.into_inner());
        if guard.session.is_some() || guard.processing {
            handle.stop();
            return Ok(());
        }
        guard.session = Some(AskDictationSession {
            handle,
            operation_id,
            transcript: transcript.clone(),
            error: error.clone(),
            done: done.clone(),
        });
    }

    emit_capsule_state(&app, PipelineState::Recording);

    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                chunk = audio_rx.recv() => {
                    match chunk {
                        Some(data) => {
                            if let Err(e) = provider.send_audio(&data).await {
                                let message = e.to_string();
                                *error.lock().unwrap_or_else(|err| err.into_inner()) = Some(message.clone());
                                let _ = app.emit("ask:error", message);
                                break;
                            }
                        }
                        None => {
                            match provider.disconnect().await {
                                Ok(Some(text)) => {
                                    let current = append_final_transcript(&transcript, &text);
                                    let _ = app.emit("ask:final", current);
                                }
                                Ok(None) => {}
                                Err(e) => {
                                    let message = e.to_string();
                                    *error.lock().unwrap_or_else(|err| err.into_inner()) = Some(message.clone());
                                    let _ = app.emit("ask:error", message);
                                }
                            }
                            break;
                        }
                    }
                }
                event = provider.recv_transcript() => {
                    match event {
                        Ok(Some(TranscriptEvent::Partial { text })) => {
                            let _ = app.emit("ask:partial", text);
                        }
                        Ok(Some(TranscriptEvent::Final { text, .. })) => {
                            let current = append_final_transcript(&transcript, &text);
                            let _ = app.emit("ask:final", current);
                        }
                        Ok(Some(TranscriptEvent::Error { message })) => {
                            *error.lock().unwrap_or_else(|err| err.into_inner()) = Some(message.clone());
                            let _ = app.emit("ask:error", message);
                            break;
                        }
                        Err(e) => {
                            let message = e.to_string();
                            *error.lock().unwrap_or_else(|err| err.into_inner()) = Some(message.clone());
                            let _ = app.emit("ask:error", message);
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        done.notify_waiters();
    });

    Ok(())
}

#[tauri::command]
pub async fn stop_ask_dictation(
    app: tauri::AppHandle,
    state: tauri::State<'_, AskDictationState>,
    config_state: tauri::State<'_, storage::ConfigManager>,
    token_store: tauri::State<'_, SessionTokenStore>,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<AskDictationResult, String> {
    let mut session = {
        let mut guard = state.0.lock().unwrap_or_else(|e| e.into_inner());
        if guard.processing {
            return Err("Ask is already processing".to_string());
        }
        let session = guard
            .session
            .take()
            .ok_or_else(|| "Ask dictation is not recording".to_string())?;
        guard.processing = true;
        session
    };

    let result = async {
        session.handle.stop();
        emit_capsule_state(&app, PipelineState::Polishing);

        tokio::select! {
            _ = session.done.notified() => {}
            _ = tokio::time::sleep(std::time::Duration::from_secs(ASK_STT_FINALIZE_TIMEOUT_SECS)) => {
                return Err("Ask dictation timed out".to_string());
            }
        }

        if let Some(message) = session
            .error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Err(message);
        }

        let question = validate_ask_question(
            &session
                .transcript
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
        )?;

        let config = config_state.load().await.map_err(|e| e.to_string())?;
        let answer = answer_question(
            &config,
            &client,
            &token_store,
            &question,
            Some(&session.operation_id),
        )
        .await?;

        Ok(AskDictationResult { question, answer })
    }
    .await;

    state.set_processing(false);
    match &result {
        Ok(_) => emit_capsule_state(&app, PipelineState::Outputting),
        Err(message) => {
            emit_capsule_state(&app, PipelineState::Idle);
            let _ = app.emit("ask:error", message.clone());
        }
    }

    result
}

#[tauri::command]
pub fn abort_ask_dictation(state: tauri::State<'_, AskDictationState>) -> Result<(), String> {
    let mut guard = state.0.lock().unwrap_or_else(|e| e.into_inner());
    guard.processing = false;
    if let Some(mut session) = guard.session.take() {
        session.handle.stop();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_body_uses_low_output_cap_and_no_web_search() {
        let body = build_byok_ask_body("What is OpenTypeless?", "test-model").unwrap();

        assert_eq!(body["model"], "test-model");
        assert_eq!(body["max_tokens"], ASK_OUTPUT_TOKEN_LIMIT);
        assert_eq!(body["stream"], false);

        let messages = body["messages"].as_array().unwrap();
        let system_prompt = messages[0]["content"].as_str().unwrap();
        assert!(system_prompt.contains("40 words"));
        assert!(system_prompt.contains("Do not use web search"));
    }

    #[test]
    fn ask_question_validation_rejects_empty_or_oversized_questions() {
        assert!(validate_ask_question("   ").is_err());
        assert!(validate_ask_question(&"x".repeat(ASK_MAX_QUESTION_CHARS + 1)).is_err());
        assert_eq!(
            validate_ask_question("  Explain polish mode.  ").unwrap(),
            "Explain polish mode."
        );
    }

    #[test]
    fn ask_dictation_stt_config_uses_cloud_token_and_multi_language() {
        let mut config = storage::AppConfig::default();
        config.stt_provider = "cloud".to_string();
        config.stt_language = "multi".to_string();

        let stt_config = build_ask_stt_config(
            &config,
            "session-token".to_string(),
            "operation-1".to_string(),
        );

        assert_eq!(stt_config.api_key, "session-token");
        assert_eq!(stt_config.language, None);
        assert_eq!(stt_config.operation_id.as_deref(), Some("operation-1"));
    }
}
