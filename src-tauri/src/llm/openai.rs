use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;

use crate::error::AppError;

use super::{
    prompt, protocol, ChunkCallback, LlmConfig, LlmProvider, PolishRequest, PolishResponse,
};

pub struct OpenAiProvider {
    client: Client,
}

impl Default for OpenAiProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenAiProvider {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    pub fn with_client(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn polish(
        &self,
        config: &LlmConfig,
        req: &PolishRequest,
        on_chunk: Option<&ChunkCallback>,
    ) -> Result<PolishResponse, AppError> {
        let has_selected_text = req
            .selected_text
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty());

        let system_prompt = prompt::build_context_system_prompt(prompt::ContextPromptOptions {
            context: &req.context,
            dictionary: &req.dictionary,
            correction_rules: &req.correction_rules,
            polish_style: &req.polish_style,
            personal_style_prompt: "",
            mapped_scene_prompt: &req.mapped_scene_prompt,
            active_scene_prompt: &req.active_scene_prompt,
            polish_custom_prompt: &req.polish_custom_prompt,
            translate_enabled: req.translate_enabled,
            target_lang: &req.target_lang,
            has_selected_text,
            voice_intent: Some(&req.voice_intent),
        });

        let mut messages = vec![serde_json::json!({ "role": "system", "content": system_prompt })];
        if has_selected_text {
            messages.push(serde_json::json!({
                "role": "user",
                "content": format!("<selected_text>\n{}\n</selected_text>", req.selected_text.as_ref().unwrap())
            }));
        }
        messages.push(serde_json::json!({
            "role": "user",
            "content": format!("<transcription>\n{}\n</transcription>", req.raw_text)
        }));

        let api_kind = protocol::detect_api_kind(&config.provider, &config.base_url);
        let endpoint = protocol::chat_endpoint(&config.provider, &config.base_url)
            .map_err(AppError::Config)?;
        let mut body = protocol::build_chat_body(
            &config.provider,
            &config.base_url,
            &config.model,
            messages,
            config.max_tokens,
            config.temperature,
            on_chunk.is_some(),
        );

        // GLM-4.7/4.5/5 default to thinking mode, but without explicitly enabling it
        // the API may return content in reasoning_content only, leaving content empty.
        // Explicitly enable thinking so both fields are properly populated.
        // Thinking mode also requires temperature >= 0.6 (recommended 1.0).
        if config.model.starts_with("glm-") {
            if let Some(obj) = body.as_object_mut() {
                obj.insert(
                    "thinking".to_string(),
                    serde_json::json!({"type": "enabled"}),
                );
                obj.insert("temperature".to_string(), serde_json::json!(1.0));
                obj.insert("top_p".to_string(), serde_json::json!(0.95));
            }
        }

        // Retry the initial connection (not once streaming starts)
        #[allow(unused_assignments)]
        let mut response = None;
        let mut last_error: Option<AppError> = None;
        let mut attempt = 0u32;

        loop {
            let request = self
                .client
                .post(&endpoint)
                .header("Content-Type", "application/json");
            match protocol::apply_auth_headers(
                request,
                &config.provider,
                &config.base_url,
                &config.api_key,
            )
            .json(&body)
            .timeout(
                protocol::request_timeout(&config.provider, &config.base_url, &config.model)
                    + protocol::transcript_time_budget(&req.raw_text),
            )
            .send()
            .await
            {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        response = Some(resp);
                        break;
                    } else if status.as_u16() >= 500 && attempt < 2 {
                        let body_text = resp.text().await.unwrap_or_default();
                        tracing::warn!(
                            "LLM server error {} (attempt {}/3), retrying",
                            status,
                            attempt + 1
                        );
                        last_error = Some(AppError::Api {
                            status: status.as_u16(),
                            body: body_text,
                        });
                        attempt += 1;
                        tokio::time::sleep(std::time::Duration::from_millis(
                            1000 * 2u64.pow(attempt - 1),
                        ))
                        .await;
                        continue;
                    } else {
                        let status = resp.status();
                        let text = resp.text().await.unwrap_or_default();
                        // Truncate at a valid UTF-8 char boundary to avoid panic on multi-byte chars
                        let truncate_at = text
                            .char_indices()
                            .take_while(|&(i, _)| i < 200)
                            .last()
                            .map(|(i, c)| i + c.len_utf8())
                            .unwrap_or(text.len());
                        let sanitized = &text[..truncate_at];
                        return Err(AppError::Api {
                            status: status.as_u16(),
                            body: sanitized.to_string(),
                        });
                    }
                }
                Err(e) if e.is_timeout() && attempt < 2 => {
                    tracing::warn!(
                        "LLM connection timeout (attempt {}/3), retrying",
                        attempt + 1
                    );
                    last_error = Some(e.into());
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(
                        1000 * 2u64.pow(attempt - 1),
                    ))
                    .await;
                    continue;
                }
                Err(e) if e.is_connect() && attempt < 2 => {
                    tracing::warn!(
                        "LLM connection failed (attempt {}/3), retrying",
                        attempt + 1
                    );
                    last_error = Some(e.into());
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(
                        1000 * 2u64.pow(attempt - 1),
                    ))
                    .await;
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
        }

        let response = response.ok_or_else(|| last_error.unwrap())?;

        if let Some(callback) = on_chunk {
            // Streaming mode
            let mut full_text = String::new();
            let mut reasoning_chars = 0usize;
            let mut stream = response.bytes_stream();

            let mut buffer = String::new();
            let mut stream_done = false;
            while !stream_done {
                let Some(chunk) = stream.next().await else {
                    break;
                };
                let chunk = chunk?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                // Process SSE lines
                while let Some(line_end) = buffer.find('\n') {
                    let line = buffer[..line_end].trim().to_string();
                    buffer = buffer[line_end + 1..].to_string();

                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            stream_done = true;
                            break;
                        }
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                            let event = protocol::parse_stream_event(api_kind, &v);
                            if let Some(error) = event.error {
                                return Err(AppError::Config(error));
                            }
                            if let Some(content) = event.text {
                                if !content.is_empty() {
                                    full_text.push_str(&content);
                                    callback(&content);
                                }
                            }

                            // Only counted for diagnostics: reasoning_content is the
                            // model's thinking, never the polished text.
                            if let Some(rc) = event.reasoning {
                                reasoning_chars += rc.chars().count();
                            }
                            stream_done = event.done;
                        }
                    }
                }
            }

            if full_text.trim().is_empty() {
                // Typically a reasoning model that spent the whole token budget
                // thinking. Fail so the pipeline outputs the raw transcript instead.
                tracing::warn!(
                    "LLM streaming returned no content ({} chars of reasoning_content)",
                    reasoning_chars
                );
                return Err(empty_answer_error(reasoning_chars));
            }

            Ok(PolishResponse {
                polished_text: full_text,
            })
        } else {
            // Non-streaming mode
            let v: serde_json::Value = response.json().await?;
            let text = protocol::response_text(api_kind, &v);

            if text.trim().is_empty() {
                let reasoning_chars = protocol::response_reasoning(api_kind, &v).chars().count();
                tracing::warn!(
                    "LLM non-streaming returned no content ({} chars of reasoning_content)",
                    reasoning_chars
                );
                return Err(empty_answer_error(reasoning_chars));
            }

            Ok(PolishResponse {
                polished_text: text,
            })
        }
    }

    fn name(&self) -> &str {
        "OpenAI"
    }
}

fn empty_answer_error(reasoning_chars: usize) -> AppError {
    if reasoning_chars > 0 {
        AppError::Config(format!(
            "the model only returned its reasoning ({reasoning_chars} chars) and no polished text; \
             use a non-reasoning model or lower its reasoning effort"
        ))
    } else {
        AppError::Config("the model returned no polished text".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Serves one OpenAI-compatible streaming reply made of the given SSE events.
    async fn serve_stream(events: &[serde_json::Value]) -> String {
        let mut body = String::new();
        for event in events {
            body.push_str(&format!("data: {event}\n\n"));
        }
        body.push_str("data: [DONE]\n\n");

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            // Read the headers and the JSON body (sent with Content-Length).
            loop {
                let n = socket.read(&mut buf).await.unwrap();
                request.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.ok();
        });
        format!("http://{addr}/v1/chat/completions")
    }

    fn config(base_url: String) -> LlmConfig {
        LlmConfig {
            provider: "openai-compatible".to_string(),
            api_key: String::new(),
            model: "gpt-5.5".to_string(),
            base_url,
            max_tokens: 4096,
            temperature: 0.3,
        }
    }

    fn request() -> PolishRequest {
        PolishRequest {
            raw_text: "把定时任务模型改成 Gemini".to_string(),
            context: crate::app_detector::types::ContextProfile::general_native().summary(),
            dictionary: Vec::new(),
            correction_rules: Vec::new(),
            polish_style: "clean".to_string(),
            mapped_scene_prompt: String::new(),
            active_scene_prompt: String::new(),
            polish_custom_prompt: String::new(),
            translate_enabled: false,
            target_lang: String::new(),
            selected_text: None,
            operation_id: None,
            voice_intent: crate::voice_intent::VoiceIntent {
                kind: crate::voice_intent::VoiceIntentKind::DictateInsert,
                placement: crate::voice_intent::VoiceOutputPlacement::InsertAtCursor,
                confidence: 1.0,
                search_provider: None,
                payload: None,
                grammar_locale: None,
                fallback_reason: None,
            },
        }
    }

    async fn polish_streaming(events: &[serde_json::Value]) -> (Result<String, AppError>, String) {
        let url = serve_stream(events).await;
        let typed = Arc::new(Mutex::new(String::new()));
        let sink = typed.clone();
        let on_chunk: ChunkCallback = Box::new(move |chunk| sink.lock().unwrap().push_str(chunk));
        let result = OpenAiProvider::new()
            .polish(&config(url), &request(), Some(&on_chunk))
            .await
            .map(|response| response.polished_text);
        let typed = typed.lock().unwrap().clone();
        (result, typed)
    }

    #[tokio::test]
    async fn streamed_reasoning_without_an_answer_fails_instead_of_being_typed() {
        let thinking = serde_json::json!({
            "choices": [{"delta": {"reasoning_content": "We need answer only polished text. Need parse."}}]
        });
        let (result, typed) = polish_streaming(&[thinking.clone(), thinking]).await;

        let error = result.expect_err("reasoning alone must not count as polished text");
        assert!(error.to_string().contains("only returned its reasoning"));
        assert_eq!(typed, "", "nothing may reach the target app");
    }

    #[tokio::test]
    async fn streamed_answer_after_reasoning_returns_only_the_answer() {
        let (result, typed) = polish_streaming(&[
            serde_json::json!({"choices": [{"delta": {"reasoning_content": "Need parse."}}]}),
            serde_json::json!({"choices": [{"delta": {"content": "把定时任务模型"}}]}),
            serde_json::json!({"choices": [{"delta": {"content": "改成 Gemini"}}]}),
        ])
        .await;

        assert_eq!(result.unwrap(), "把定时任务模型改成 Gemini");
        assert_eq!(typed, "把定时任务模型改成 Gemini");
    }
}
