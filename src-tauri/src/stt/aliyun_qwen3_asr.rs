use std::{collections::HashSet, time::Duration};

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Error as WsError, Message},
};
use uuid::Uuid;

use crate::error::AppError;

use super::{SttConfig, SttProvider, TranscriptEvent};

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub const ALIYUN_QWEN3_ASR_PROVIDER: &str = "aliyun-qwen3-asr";
pub const ALIYUN_QWEN3_ASR_REGION_CHINA_MAINLAND: &str = "china-mainland";
pub const ALIYUN_QWEN3_ASR_REGION_INTERNATIONAL: &str = "international";
pub const ALIYUN_QWEN3_ASR_CHINA_URL: &str =
    "wss://dashscope.aliyuncs.com/api-ws/v1/realtime?model=qwen3-asr-flash-realtime";
pub const ALIYUN_QWEN3_ASR_INTERNATIONAL_URL: &str =
    "wss://dashscope-intl.aliyuncs.com/api-ws/v1/realtime?model=qwen3-asr-flash-realtime";
const AUDIO_CHUNK_BYTES: usize = 3200;
const INITIAL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
const FINISH_RESPONSE_TIMEOUT: Duration = Duration::from_secs(8);

enum ServerEvent {
    Transcript(TranscriptEvent),
    SessionCreated,
    SessionUpdated,
    SessionFinished,
    Ignored,
}

pub struct AliyunQwen3AsrProvider {
    ws: Option<WsStream>,
    pending_audio: Vec<u8>,
    completed_item_ids: HashSet<String>,
    url_override: Option<String>,
    appended_audio_bytes: usize,
    /// Latest live transcript (confirmed text + stash) of each segment still
    /// waiting for its final, in speaking order.
    unfinished_segments: Vec<(String, String)>,
}

impl Default for AliyunQwen3AsrProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AliyunQwen3AsrProvider {
    pub fn new() -> Self {
        Self {
            ws: None,
            pending_audio: Vec::with_capacity(AUDIO_CHUNK_BYTES),
            completed_item_ids: HashSet::new(),
            url_override: None,
            appended_audio_bytes: 0,
            unfinished_segments: Vec::new(),
        }
    }

    #[cfg(test)]
    fn with_url(url: String) -> Self {
        let mut provider = Self::new();
        provider.url_override = Some(url);
        provider
    }

    fn build_request(
        &self,
        api_key: &str,
        region: Option<&str>,
    ) -> Result<http::Request<()>, AppError> {
        let url = self
            .url_override
            .as_deref()
            .unwrap_or_else(|| qwen_realtime_url(region));
        let uri = url
            .parse::<http::Uri>()
            .map_err(|error| AppError::Config(error.to_string()))?;
        let host = uri
            .authority()
            .map(http::uri::Authority::as_str)
            .ok_or_else(|| AppError::Config("Aliyun Qwen3 ASR endpoint has no host".to_string()))?;
        http::Request::builder()
            .uri(uri.clone())
            .header("Authorization", format!("Bearer {api_key}"))
            .header("User-Agent", "OpenTypeless/1")
            .header("Host", host)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tokio_tungstenite::tungstenite::handshake::client::generate_key(),
            )
            .body(())
            .map_err(|error| AppError::Config(error.to_string()))
    }

    async fn send_json(ws: &mut WsStream, value: serde_json::Value) -> Result<(), AppError> {
        ws.send(Message::Text(value.to_string()))
            .await
            .map_err(|error| AppError::Network(error.to_string()))
    }

    async fn send_session_update(&mut self, config: &SttConfig) -> Result<(), AppError> {
        let mut session = serde_json::json!({
            "input_audio_format": "pcm",
            "sample_rate": config.sample_rate,
            "turn_detection": {
                "type": "server_vad",
                "threshold": 0.0,
                // Each segment ends with "。"; 400 ms split sentences at every
                // thinking pause, so only a longer silence closes a segment.
                "silence_duration_ms": 1200
            }
        });
        if let Some(language) = qwen_language(config.language.as_deref()) {
            session["input_audio_transcription"] = serde_json::json!({ "language": language });
        }

        let ws = self
            .ws
            .as_mut()
            .ok_or_else(|| AppError::Network("Aliyun Qwen3 ASR is not connected".to_string()))?;
        Self::send_json(
            ws,
            serde_json::json!({
                "event_id": event_id(),
                "type": "session.update",
                "session": session,
            }),
        )
        .await
    }

    async fn append_audio(&mut self, audio: &[u8]) -> Result<(), AppError> {
        self.appended_audio_bytes += audio.len();
        let ws = self
            .ws
            .as_mut()
            .ok_or_else(|| AppError::Network("Aliyun Qwen3 ASR is not connected".to_string()))?;
        Self::send_json(
            ws,
            serde_json::json!({
                "event_id": event_id(),
                "type": "input_audio_buffer.append",
                "audio": STANDARD.encode(audio),
            }),
        )
        .await
    }

    async fn flush_pending_audio(&mut self) -> Result<(), AppError> {
        if self.pending_audio.is_empty() {
            return Ok(());
        }
        let audio = std::mem::take(&mut self.pending_audio);
        self.append_audio(&audio).await
    }

    /// Keeps each segment's live transcript until its final arrives, so a final
    /// that never comes (slow network, early close) does not drop the words.
    fn track_segment(&mut self, message: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(message) else {
            return;
        };
        // Without an id a live transcript cannot be matched to its final.
        let Some(item_id) = value["item_id"].as_str().filter(|id| !id.is_empty()) else {
            return;
        };
        match value["type"].as_str() {
            Some("conversation.item.input_audio_transcription.text") => {
                let text = format!(
                    "{}{}",
                    value["text"].as_str().unwrap_or_default(),
                    value["stash"].as_str().unwrap_or_default()
                );
                if text.trim().is_empty() {
                    return;
                }
                match self
                    .unfinished_segments
                    .iter_mut()
                    .find(|(id, _)| id == item_id)
                {
                    Some((_, latest)) => *latest = text,
                    None => self.unfinished_segments.push((item_id.to_string(), text)),
                }
            }
            Some("conversation.item.input_audio_transcription.completed") => {
                self.unfinished_segments.retain(|(id, _)| id != item_id);
            }
            _ => {}
        }
    }

    async fn wait_for_session_event(
        &mut self,
        expected: fn(&ServerEvent) -> bool,
    ) -> Result<(), AppError> {
        let ws = self
            .ws
            .as_mut()
            .ok_or_else(|| AppError::Network("Aliyun Qwen3 ASR is not connected".to_string()))?;
        loop {
            let next = tokio::time::timeout(INITIAL_RESPONSE_TIMEOUT, ws.next())
                .await
                .map_err(|_| {
                    AppError::Network("Aliyun Qwen3 ASR session setup timed out".to_string())
                })?;
            match next {
                Some(Ok(Message::Text(text))) => {
                    let event = parse_server_message(&text)?;
                    if let ServerEvent::Transcript(TranscriptEvent::Error { message }) = &event {
                        return Err(AppError::Config(message.clone()));
                    }
                    if expected(&event) {
                        return Ok(());
                    }
                }
                Some(Ok(Message::Close(_))) | None => {
                    return Err(AppError::Network(
                        "Aliyun Qwen3 ASR WebSocket closed during session setup".to_string(),
                    ));
                }
                Some(Err(error)) => return Err(AppError::Network(error.to_string())),
                _ => {}
            }
        }
    }
}

pub fn qwen_realtime_url(region: Option<&str>) -> &'static str {
    match region {
        Some(ALIYUN_QWEN3_ASR_REGION_INTERNATIONAL) => ALIYUN_QWEN3_ASR_INTERNATIONAL_URL,
        _ => ALIYUN_QWEN3_ASR_CHINA_URL,
    }
}

fn event_id() -> String {
    format!("event_{}", Uuid::new_v4())
}

fn qwen_language(language: Option<&str>) -> Option<&str> {
    let language = language?.trim();
    match language {
        "zh" | "yue" | "en" | "ja" | "de" | "ko" | "ru" | "fr" | "pt" | "ar" | "it" | "es"
        | "hi" | "id" | "th" | "tr" | "uk" | "vi" | "cs" | "da" | "fil" | "fi" | "is" | "ms"
        | "no" | "pl" | "sv" => Some(language),
        _ => None,
    }
}

fn parse_server_message(text: &str) -> Result<ServerEvent, AppError> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| AppError::Config(error.to_string()))?;
    match value["type"].as_str().unwrap_or_default() {
        "session.created" => Ok(ServerEvent::SessionCreated),
        "session.updated" => Ok(ServerEvent::SessionUpdated),
        "session.finished" => Ok(ServerEvent::SessionFinished),
        "input_audio_buffer.speech_started" => {
            Ok(ServerEvent::Transcript(TranscriptEvent::SpeechStarted))
        }
        "input_audio_buffer.speech_stopped" => {
            Ok(ServerEvent::Transcript(TranscriptEvent::SpeechEnded))
        }
        "conversation.item.input_audio_transcription.text" => {
            let text = format!(
                "{}{}",
                value["text"].as_str().unwrap_or_default(),
                value["stash"].as_str().unwrap_or_default()
            );
            if text.trim().is_empty() {
                Ok(ServerEvent::Ignored)
            } else {
                Ok(ServerEvent::Transcript(TranscriptEvent::Partial { text }))
            }
        }
        "conversation.item.input_audio_transcription.completed" => {
            let text = value["transcript"].as_str().unwrap_or_default().to_string();
            if text.trim().is_empty() {
                Ok(ServerEvent::Ignored)
            } else {
                Ok(ServerEvent::Transcript(TranscriptEvent::Final {
                    text,
                    confidence: 1.0,
                }))
            }
        }
        "conversation.item.input_audio_transcription.failed" | "error" => {
            let message = value["error"]["message"]
                .as_str()
                .or_else(|| value["message"].as_str())
                .unwrap_or("Unknown Aliyun Qwen3 ASR error")
                .to_string();
            Ok(ServerEvent::Transcript(TranscriptEvent::Error { message }))
        }
        _ => Ok(ServerEvent::Ignored),
    }
}

fn completed_item_id(message: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(message).ok()?;
    (value["type"].as_str() == Some("conversation.item.input_audio_transcription.completed"))
        .then(|| value["item_id"].as_str().unwrap_or_default().to_string())
        .filter(|item_id| !item_id.is_empty())
}

/// Summarises a server event for diagnostics without logging any transcript text.
fn describe_server_message(message: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(message) else {
        return format!("unparseable message ({} bytes)", message.len());
    };
    let kind = value["type"].as_str().unwrap_or("<no type>");
    let chars = |field: &str| value[field].as_str().map_or(0, |text| text.chars().count());
    match kind {
        "conversation.item.input_audio_transcription.text" => format!(
            "{kind} (item {}, text {} chars, stash {} chars)",
            value["item_id"].as_str().unwrap_or("?"),
            chars("text"),
            chars("stash")
        ),
        "conversation.item.input_audio_transcription.completed" => format!(
            "{kind} (item {}, {} chars)",
            value["item_id"].as_str().unwrap_or("?"),
            chars("transcript")
        ),
        "error" | "conversation.item.input_audio_transcription.failed" => {
            format!("{kind}: {}", value["error"])
        }
        _ => kind.to_string(),
    }
}

fn map_connect_error(error: WsError) -> AppError {
    match error {
        WsError::Http(response) if response.status() == 401 || response.status() == 403 => {
            AppError::Auth("Aliyun Qwen3 ASR authentication failed. Make sure the API key was created in the selected Beijing or Singapore region and has realtime ASR access.".to_string())
        }
        WsError::Http(response) => AppError::Api {
            status: response.status().as_u16(),
            body: response.status().to_string(),
        },
        other => AppError::Network(other.to_string()),
    }
}

#[async_trait]
impl SttProvider for AliyunQwen3AsrProvider {
    async fn connect(&mut self, config: &SttConfig) -> Result<(), AppError> {
        if config.api_key.trim().is_empty() {
            return Err(AppError::Auth(
                "Aliyun Qwen3 ASR API key is empty".to_string(),
            ));
        }

        let request =
            self.build_request(config.api_key.trim(), config.provider_region.as_deref())?;
        let (ws, _) = connect_async(request).await.map_err(map_connect_error)?;
        self.ws = Some(ws);
        self.pending_audio.clear();
        self.completed_item_ids.clear();
        self.appended_audio_bytes = 0;
        self.unfinished_segments.clear();
        self.wait_for_session_event(|event| matches!(event, ServerEvent::SessionCreated))
            .await?;
        self.send_session_update(config).await?;
        self.wait_for_session_event(|event| matches!(event, ServerEvent::SessionUpdated))
            .await?;
        tracing::info!("Aliyun Qwen3 ASR WebSocket connected");
        Ok(())
    }

    async fn send_audio(&mut self, chunk: &[u8]) -> Result<(), AppError> {
        self.pending_audio.extend_from_slice(chunk);
        while self.pending_audio.len() >= AUDIO_CHUNK_BYTES {
            let audio: Vec<u8> = self.pending_audio.drain(..AUDIO_CHUNK_BYTES).collect();
            self.append_audio(&audio).await?;
        }
        Ok(())
    }

    async fn recv_transcript(&mut self) -> Result<Option<TranscriptEvent>, AppError> {
        let Some(ws) = self.ws.as_mut() else {
            // Closed: never resolve, so the caller's select! keeps serving audio and abort
            // instead of spinning on an ended stream.
            return std::future::pending().await;
        };
        match ws.next().await {
            Some(Ok(Message::Text(text))) => {
                tracing::debug!("Qwen3 ASR event: {}", describe_server_message(&text));
                self.track_segment(&text);
                let item_id = completed_item_id(&text);
                match parse_server_message(&text)? {
                    ServerEvent::Transcript(TranscriptEvent::Final { text, confidence }) => {
                        if let Some(item_id) = item_id {
                            if !self.completed_item_ids.insert(item_id) {
                                return Ok(None);
                            }
                        }
                        Ok(Some(TranscriptEvent::Final { text, confidence }))
                    }
                    ServerEvent::Transcript(event) => Ok(Some(event)),
                    _ => Ok(None),
                }
            }
            Some(Ok(Message::Close(frame))) => {
                tracing::warn!(
                    "Qwen3 ASR server closed the session mid-recording: {:?}",
                    frame
                );
                self.ws = None;
                Ok(None)
            }
            None => {
                tracing::warn!("Qwen3 ASR stream ended mid-recording");
                self.ws = None;
                Ok(None)
            }
            Some(Err(error)) => Ok(Some(TranscriptEvent::Error {
                message: error.to_string(),
            })),
            _ => Ok(None),
        }
    }

    async fn disconnect(&mut self) -> Result<Option<String>, AppError> {
        self.flush_pending_audio().await?;
        let Some(mut ws) = self.ws.take() else {
            tracing::warn!("Qwen3 ASR disconnect without a connection");
            return Ok(None);
        };
        let finish_started = tokio::time::Instant::now();
        tracing::info!(
            "Qwen3 ASR finishing: {:.1}s of audio sent, {} segment(s) completed so far",
            self.appended_audio_bytes as f64 / 32_000.0,
            self.completed_item_ids.len()
        );
        Self::send_json(
            &mut ws,
            serde_json::json!({
                "event_id": event_id(),
                "type": "session.finish",
            }),
        )
        .await?;

        let deadline = tokio::time::Instant::now() + FINISH_RESPONSE_TIMEOUT;
        let mut final_texts = Vec::new();
        while tokio::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            match tokio::time::timeout(remaining, ws.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    tracing::debug!("Qwen3 ASR finish event: {}", describe_server_message(&text));
                    self.track_segment(&text);
                    let item_id = completed_item_id(&text);
                    match parse_server_message(&text)? {
                        ServerEvent::SessionFinished => break,
                        ServerEvent::Transcript(TranscriptEvent::Final { text, .. }) => {
                            let is_new = item_id
                                .map(|item_id| self.completed_item_ids.insert(item_id))
                                .unwrap_or(true);
                            if is_new {
                                final_texts.push(text);
                            }
                        }
                        ServerEvent::Transcript(TranscriptEvent::Error { message }) => {
                            return Err(AppError::Config(message));
                        }
                        _ => {}
                    }
                }
                Ok(Some(Ok(Message::Close(frame)))) => {
                    tracing::warn!("Qwen3 ASR closed before session.finished: {:?}", frame);
                    break;
                }
                Ok(None) => {
                    tracing::warn!("Qwen3 ASR stream ended before session.finished");
                    break;
                }
                Err(_) => {
                    tracing::warn!(
                        "Qwen3 ASR gave no session.finished within {}s",
                        FINISH_RESPONSE_TIMEOUT.as_secs()
                    );
                    break;
                }
                Ok(Some(Ok(_))) => {}
                Ok(Some(Err(error))) => return Err(AppError::Network(error.to_string())),
            }
        }
        let _ = ws.close(None).await;
        if !self.unfinished_segments.is_empty() {
            tracing::warn!(
                "Qwen3 ASR kept the live transcript of {} segment(s) that never got a final",
                self.unfinished_segments.len()
            );
            final_texts.extend(self.unfinished_segments.drain(..).map(|(_, text)| text));
        }
        tracing::info!(
            "Aliyun Qwen3 ASR disconnected after {}ms with {} final segment(s)",
            finish_started.elapsed().as_millis(),
            final_texts.len()
        );
        Ok((!final_texts.is_empty()).then(|| final_texts.join(" ")))
    }

    fn name(&self) -> &str {
        "Aliyun Qwen3 Realtime ASR"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    fn test_config(language: Option<&str>) -> SttConfig {
        SttConfig {
            api_key: "test-key".to_string(),
            language: language.map(str::to_string),
            smart_format: true,
            sample_rate: 16_000,
            resource_id: None,
            operation_id: None,
            managed_audio: None,
            provider_region: None,
        }
    }

    #[test]
    fn builds_authorized_beijing_realtime_request() {
        let provider = AliyunQwen3AsrProvider::new();
        let request = provider
            .build_request("test-key", Some(ALIYUN_QWEN3_ASR_REGION_CHINA_MAINLAND))
            .unwrap();

        assert_eq!(request.uri(), ALIYUN_QWEN3_ASR_CHINA_URL);
        assert_eq!(request.headers()["authorization"], "Bearer test-key");
        assert_eq!(request.headers()["user-agent"], "OpenTypeless/1");
        assert_eq!(request.headers()["host"], "dashscope.aliyuncs.com");
    }

    #[test]
    fn builds_authorized_international_realtime_request() {
        let provider = AliyunQwen3AsrProvider::new();
        let request = provider
            .build_request("test-key", Some(ALIYUN_QWEN3_ASR_REGION_INTERNATIONAL))
            .unwrap();

        assert_eq!(request.uri(), ALIYUN_QWEN3_ASR_INTERNATIONAL_URL);
        assert_eq!(request.headers()["authorization"], "Bearer test-key");
        assert_eq!(request.headers()["host"], "dashscope-intl.aliyuncs.com");
    }

    #[test]
    fn missing_or_unknown_region_falls_back_to_beijing() {
        assert_eq!(qwen_realtime_url(None), ALIYUN_QWEN3_ASR_CHINA_URL);
        assert_eq!(
            qwen_realtime_url(Some("not-a-region")),
            ALIYUN_QWEN3_ASR_CHINA_URL
        );
    }

    #[test]
    fn maps_supported_languages_and_falls_back_to_auto_detect() {
        assert_eq!(qwen_language(Some("zh")), Some("zh"));
        assert_eq!(qwen_language(Some("nl")), None);
        assert_eq!(qwen_language(None), None);
    }

    #[test]
    fn parses_partial_using_confirmed_text_and_stash() {
        let event = parse_server_message(
            r#"{"type":"conversation.item.input_audio_transcription.text","text":"hello ","stash":"world"}"#,
        )
        .unwrap();

        match event {
            ServerEvent::Transcript(TranscriptEvent::Partial { text }) => {
                assert_eq!(text, "hello world");
            }
            _ => panic!("expected partial transcript"),
        }
    }

    #[test]
    fn parses_completed_transcript_and_error() {
        let completed = parse_server_message(
            r#"{"type":"conversation.item.input_audio_transcription.completed","transcript":"hello world"}"#,
        )
        .unwrap();
        let error =
            parse_server_message(r#"{"type":"error","error":{"message":"bad key"}}"#).unwrap();

        assert!(matches!(
            completed,
            ServerEvent::Transcript(TranscriptEvent::Final { text, .. }) if text == "hello world"
        ));
        assert!(matches!(
            error,
            ServerEvent::Transcript(TranscriptEvent::Error { message }) if message == "bad key"
        ));
    }

    #[tokio::test]
    async fn streams_100ms_audio_and_drains_the_final_transcript() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            ws.send(Message::Text(
                serde_json::json!({ "type": "session.created" }).to_string(),
            ))
            .await
            .unwrap();

            let update = ws.next().await.unwrap().unwrap().into_text().unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&update).unwrap()["type"],
                "session.update"
            );
            ws.send(Message::Text(
                serde_json::json!({ "type": "session.updated" }).to_string(),
            ))
            .await
            .unwrap();

            let append = ws.next().await.unwrap().unwrap().into_text().unwrap();
            let append: serde_json::Value = serde_json::from_str(&append).unwrap();
            assert_eq!(append["type"], "input_audio_buffer.append");
            assert_eq!(
                STANDARD
                    .decode(append["audio"].as_str().unwrap())
                    .unwrap()
                    .len(),
                3200
            );
            ws.send(Message::Text(
                serde_json::json!({
                    "type": "conversation.item.input_audio_transcription.text",
                    "text": "hello ",
                    "stash": "world"
                })
                .to_string(),
            ))
            .await
            .unwrap();

            let finish = ws.next().await.unwrap().unwrap().into_text().unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&finish).unwrap()["type"],
                "session.finish"
            );
            ws.send(Message::Text(
                serde_json::json!({
                    "type": "conversation.item.input_audio_transcription.completed",
                    "item_id": "item-1",
                    "transcript": "hello world"
                })
                .to_string(),
            ))
            .await
            .unwrap();
            ws.send(Message::Text(
                serde_json::json!({ "type": "session.finished" }).to_string(),
            ))
            .await
            .unwrap();
        });

        let mut provider = AliyunQwen3AsrProvider::with_url(format!("ws://{address}"));
        provider.connect(&test_config(Some("zh"))).await.unwrap();
        for _ in 0..5 {
            provider.send_audio(&vec![1; 640]).await.unwrap();
        }
        assert!(matches!(
            provider.recv_transcript().await.unwrap(),
            Some(TranscriptEvent::Partial { text }) if text == "hello world"
        ));
        assert_eq!(
            provider.disconnect().await.unwrap().as_deref(),
            Some("hello world")
        );
        server.await.unwrap();
    }

    async fn accept_session(
        listener: TcpListener,
    ) -> tokio_tungstenite::WebSocketStream<tokio::net::TcpStream> {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        ws.send(Message::Text(
            serde_json::json!({ "type": "session.created" }).to_string(),
        ))
        .await
        .unwrap();
        ws.next().await.unwrap().unwrap();
        ws.send(Message::Text(
            serde_json::json!({ "type": "session.updated" }).to_string(),
        ))
        .await
        .unwrap();
        ws
    }

    fn partial(item_id: &str, text: &str, stash: &str) -> Message {
        Message::Text(
            serde_json::json!({
                "type": "conversation.item.input_audio_transcription.text",
                "item_id": item_id,
                "text": text,
                "stash": stash
            })
            .to_string(),
        )
    }

    fn completed(item_id: &str, transcript: &str) -> Message {
        Message::Text(
            serde_json::json!({
                "type": "conversation.item.input_audio_transcription.completed",
                "item_id": item_id,
                "transcript": transcript
            })
            .to_string(),
        )
    }

    #[tokio::test]
    async fn keeps_the_live_transcript_when_the_final_never_arrives() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut ws = accept_session(listener).await;
            ws.next().await.unwrap().unwrap(); // audio
            ws.send(partial("item-1", "今天", "天气")).await.unwrap();
            ws.next().await.unwrap().unwrap(); // session.finish
            ws.send(partial("item-1", "今天天气", "不错"))
                .await
                .unwrap();
            ws.close(None).await.unwrap();
        });

        let mut provider = AliyunQwen3AsrProvider::with_url(format!("ws://{address}"));
        provider.connect(&test_config(Some("zh"))).await.unwrap();
        provider.send_audio(&[1; 3200]).await.unwrap();
        assert!(matches!(
            provider.recv_transcript().await.unwrap(),
            Some(TranscriptEvent::Partial { .. })
        ));
        assert_eq!(
            provider.disconnect().await.unwrap().as_deref(),
            Some("今天天气不错")
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn adds_the_unfinished_segment_after_the_completed_ones() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut ws = accept_session(listener).await;
            ws.next().await.unwrap().unwrap(); // audio
            ws.send(partial("item-1", "第一句", "")).await.unwrap();
            ws.send(completed("item-1", "第一句。")).await.unwrap();
            ws.send(partial("item-2", "第二", "句")).await.unwrap();
            ws.next().await.unwrap().unwrap(); // session.finish
            ws.close(None).await.unwrap();
        });

        let mut provider = AliyunQwen3AsrProvider::with_url(format!("ws://{address}"));
        provider.connect(&test_config(Some("zh"))).await.unwrap();
        provider.send_audio(&[1; 3200]).await.unwrap();
        let mut finals = Vec::new();
        for _ in 0..3 {
            if let Some(TranscriptEvent::Final { text, .. }) =
                provider.recv_transcript().await.unwrap()
            {
                finals.push(text);
            }
        }
        assert_eq!(finals, ["第一句。"]);
        assert_eq!(
            provider.disconnect().await.unwrap().as_deref(),
            Some("第二句")
        );
        server.await.unwrap();
    }

    #[test]
    fn maps_unauthorized_websocket_upgrade_to_auth_error() {
        let response = http::Response::builder().status(401).body(None).unwrap();
        assert!(matches!(
            map_connect_error(WsError::Http(response)),
            AppError::Auth(_)
        ));
    }
}
