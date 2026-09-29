//! Alibaba Cloud Model Studio `qwen-audio-3.0-asr-flash-streaming` (task-based WebSocket
//! protocol shared with Fun-ASR realtime: `run-task` / `result-generated` / `finish-task`).
//!
//! Unlike the older `qwen3-asr-flash-realtime` client, this endpoint lives on a per-workspace
//! host, so the connection needs both a region and the user's Workspace ID. Both arrive in
//! `SttConfig::provider_region` as `"<region>/<workspace id>"` (see [`region_setting`]).

use std::{collections::HashSet, time::Duration};

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Error as WsError, Message},
};
use uuid::Uuid;

use crate::error::AppError;

use super::aliyun_qwen3_asr::{
    ALIYUN_QWEN3_ASR_REGION_CHINA_MAINLAND, ALIYUN_QWEN3_ASR_REGION_INTERNATIONAL,
};
use super::{SttConfig, SttProvider, TranscriptEvent};

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub const ALIYUN_QWEN_AUDIO_PROVIDER: &str = "aliyun-qwen-audio";
pub const ALIYUN_QWEN_AUDIO_MODEL: &str = "qwen-audio-3.0-asr-flash-streaming";
/// 100 ms of 16 kHz 16-bit mono PCM.
const AUDIO_CHUNK_BYTES: usize = 3200;
const TASK_START_TIMEOUT: Duration = Duration::from_secs(5);
const FINISH_RESPONSE_TIMEOUT: Duration = Duration::from_secs(8);

/// Packs the region and Workspace ID for `SttConfig::provider_region`.
pub fn region_setting(region: &str, workspace_id: &str) -> String {
    format!("{}/{}", region.trim(), workspace_id.trim())
}

/// Accepts the bare Workspace ID and also a pasted API host or WebSocket URL (with or without
/// `<...>` placeholders), and returns just the ID.
fn workspace_label(input: &str) -> &str {
    let input = input.trim();
    let input = input.split_once("://").map_or(input, |(_, rest)| rest);
    let input = input.split('/').next().unwrap_or_default();
    let input = input.trim_matches(|c| c == '<' || c == '>' || c == ' ');
    let label = input.split('.').next().unwrap_or_default();
    label.trim_matches(|c| c == '<' || c == '>')
}

/// Builds the WebSocket URL from a [`region_setting`] value.
pub fn endpoint_url(region_setting: Option<&str>) -> Result<String, AppError> {
    let (region, workspace) = region_setting
        .unwrap_or_default()
        .split_once('/')
        .unwrap_or((region_setting.unwrap_or_default(), ""));
    // `split_once` keeps everything after the first '/', so a pasted URL stays intact.
    let workspace = workspace_label(workspace);
    if workspace.is_empty() {
        return Err(AppError::Config(
            "Qwen-Audio streaming needs your Workspace ID. Enter it in the speech settings."
                .to_string(),
        ));
    }
    if !workspace
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(AppError::Config(
            "The Workspace ID may only contain letters, digits, '-' and '_'.".to_string(),
        ));
    }
    let region_host = match region.trim() {
        ALIYUN_QWEN3_ASR_REGION_INTERNATIONAL => "ap-southeast-1",
        ALIYUN_QWEN3_ASR_REGION_CHINA_MAINLAND | "" => "cn-beijing",
        other => {
            return Err(AppError::Config(format!(
                "Unknown Qwen-Audio region: {other}"
            )))
        }
    };
    Ok(format!(
        "wss://{workspace}.{region_host}.maas.aliyuncs.com/api-ws/v1/inference"
    ))
}

#[derive(Debug, PartialEq)]
enum ServerEvent {
    TaskStarted,
    TaskFinished,
    Sentence {
        id: Option<i64>,
        text: String,
        ended: bool,
    },
    Failed(String),
    Ignored,
}

fn parse_server_message(text: &str) -> Result<ServerEvent, AppError> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| AppError::Config(error.to_string()))?;
    match value["header"]["event"].as_str().unwrap_or_default() {
        "task-started" => Ok(ServerEvent::TaskStarted),
        "task-finished" => Ok(ServerEvent::TaskFinished),
        "task-failed" => {
            let code = value["header"]["error_code"].as_str().unwrap_or("UNKNOWN");
            let message = value["header"]["error_message"]
                .as_str()
                .unwrap_or("Unknown Qwen-Audio streaming error");
            Ok(ServerEvent::Failed(format!("{code}: {message}")))
        }
        "result-generated" => {
            let sentence = &value["payload"]["output"]["sentence"];
            let text = sentence["text"].as_str().unwrap_or_default().to_string();
            if text.trim().is_empty() {
                return Ok(ServerEvent::Ignored);
            }
            Ok(ServerEvent::Sentence {
                id: sentence["sentence_id"].as_i64(),
                text,
                ended: sentence["sentence_end"].as_bool().unwrap_or(false),
            })
        }
        _ => Ok(ServerEvent::Ignored),
    }
}

fn map_connect_error(error: WsError) -> AppError {
    match error {
        WsError::Http(response) if response.status() == 401 || response.status() == 403 => {
            AppError::Auth("Qwen-Audio streaming authentication failed. Check the API key, the Workspace ID and the selected region.".to_string())
        }
        WsError::Http(response) => AppError::Api {
            status: response.status().as_u16(),
            body: response.status().to_string(),
        },
        other => AppError::Network(other.to_string()),
    }
}

pub struct AliyunQwenAudioProvider {
    ws: Option<WsStream>,
    task_id: String,
    pending_audio: Vec<u8>,
    sent_audio_bytes: usize,
    finished_sentence_ids: HashSet<i64>,
}

impl Default for AliyunQwenAudioProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AliyunQwenAudioProvider {
    pub fn new() -> Self {
        Self {
            ws: None,
            task_id: String::new(),
            pending_audio: Vec::with_capacity(AUDIO_CHUNK_BYTES),
            sent_audio_bytes: 0,
            finished_sentence_ids: HashSet::new(),
        }
    }

    fn build_request(url: &str, api_key: &str) -> Result<http::Request<()>, AppError> {
        let uri = url
            .parse::<http::Uri>()
            .map_err(|error| AppError::Config(error.to_string()))?;
        let host = uri
            .authority()
            .map(http::uri::Authority::as_str)
            .ok_or_else(|| AppError::Config("Qwen-Audio endpoint has no host".to_string()))?;
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

    fn run_task_message(&self, sample_rate: u32) -> serde_json::Value {
        serde_json::json!({
            "header": {
                "action": "run-task",
                "task_id": self.task_id,
                "streaming": "duplex"
            },
            "payload": {
                "task_group": "audio",
                "task": "asr",
                "function": "recognition",
                "model": ALIYUN_QWEN_AUDIO_MODEL,
                "parameters": {
                    "format": "pcm",
                    "sample_rate": sample_rate
                },
                "input": {}
            }
        })
    }

    fn finish_task_message(&self) -> serde_json::Value {
        serde_json::json!({
            "header": {
                "action": "finish-task",
                "task_id": self.task_id,
                "streaming": "duplex"
            },
            "payload": { "input": {} }
        })
    }

    async fn send_text(ws: &mut WsStream, value: serde_json::Value) -> Result<(), AppError> {
        ws.send(Message::Text(value.to_string()))
            .await
            .map_err(|error| AppError::Network(error.to_string()))
    }

    async fn send_audio_frame(&mut self, audio: Vec<u8>) -> Result<(), AppError> {
        self.sent_audio_bytes += audio.len();
        let ws = self.ws.as_mut().ok_or_else(|| {
            AppError::Network("Qwen-Audio streaming is not connected".to_string())
        })?;
        ws.send(Message::Binary(audio))
            .await
            .map_err(|error| AppError::Network(error.to_string()))
    }

    /// Records a finished sentence once; `false` when it was already delivered.
    fn mark_finished(&mut self, id: Option<i64>) -> bool {
        id.is_none_or(|id| self.finished_sentence_ids.insert(id))
    }
}

#[async_trait]
impl SttProvider for AliyunQwenAudioProvider {
    async fn connect(&mut self, config: &SttConfig) -> Result<(), AppError> {
        let api_key = config.api_key.trim();
        if api_key.is_empty() {
            return Err(AppError::Auth(
                "Qwen-Audio streaming API key is empty".to_string(),
            ));
        }
        let url = endpoint_url(config.provider_region.as_deref())?;
        let request = Self::build_request(&url, api_key)?;
        let (ws, _) = connect_async(request).await.map_err(map_connect_error)?;

        self.ws = Some(ws);
        self.task_id = Uuid::new_v4().simple().to_string();
        self.pending_audio.clear();
        self.sent_audio_bytes = 0;
        self.finished_sentence_ids.clear();

        let run_task = self.run_task_message(config.sample_rate);
        let ws = self.ws.as_mut().expect("just connected");
        Self::send_text(ws, run_task).await?;

        loop {
            let next = tokio::time::timeout(TASK_START_TIMEOUT, ws.next())
                .await
                .map_err(|_| {
                    AppError::Network("Qwen-Audio streaming task start timed out".to_string())
                })?;
            match next {
                Some(Ok(Message::Text(text))) => match parse_server_message(&text)? {
                    ServerEvent::TaskStarted => break,
                    ServerEvent::Failed(message) => return Err(AppError::Config(message)),
                    _ => {}
                },
                Some(Ok(Message::Close(_))) | None => {
                    return Err(AppError::Network(
                        "Qwen-Audio streaming closed before the task started".to_string(),
                    ));
                }
                Some(Err(error)) => return Err(AppError::Network(error.to_string())),
                _ => {}
            }
        }
        tracing::info!("Aliyun Qwen-Audio streaming task started");
        Ok(())
    }

    async fn send_audio(&mut self, chunk: &[u8]) -> Result<(), AppError> {
        self.pending_audio.extend_from_slice(chunk);
        while self.pending_audio.len() >= AUDIO_CHUNK_BYTES {
            let audio: Vec<u8> = self.pending_audio.drain(..AUDIO_CHUNK_BYTES).collect();
            self.send_audio_frame(audio).await?;
        }
        Ok(())
    }

    async fn recv_transcript(&mut self) -> Result<Option<TranscriptEvent>, AppError> {
        let Some(ws) = self.ws.as_mut() else {
            // Closed: never resolve, so the caller keeps serving audio and abort.
            return std::future::pending().await;
        };
        match ws.next().await {
            Some(Ok(Message::Text(text))) => match parse_server_message(&text)? {
                ServerEvent::Sentence { id, text, ended } => {
                    if ended {
                        if !self.mark_finished(id) {
                            return Ok(None);
                        }
                        Ok(Some(TranscriptEvent::Final {
                            text,
                            confidence: 1.0,
                        }))
                    } else {
                        Ok(Some(TranscriptEvent::Partial { text }))
                    }
                }
                ServerEvent::Failed(message) => Ok(Some(TranscriptEvent::Error { message })),
                _ => Ok(None),
            },
            Some(Ok(Message::Close(frame))) => {
                tracing::warn!("Qwen-Audio server closed the session mid-recording: {frame:?}");
                self.ws = None;
                Ok(None)
            }
            None => {
                tracing::warn!("Qwen-Audio stream ended mid-recording");
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
        if !self.pending_audio.is_empty() {
            let audio = std::mem::take(&mut self.pending_audio);
            self.send_audio_frame(audio).await?;
        }
        let Some(mut ws) = self.ws.take() else {
            tracing::warn!("Qwen-Audio disconnect without a connection");
            return Ok(None);
        };
        let started = tokio::time::Instant::now();
        tracing::info!(
            "Qwen-Audio finishing: {:.1}s of audio sent, {} sentence(s) finished so far",
            self.sent_audio_bytes as f64 / 32_000.0,
            self.finished_sentence_ids.len()
        );
        Self::send_text(&mut ws, self.finish_task_message()).await?;

        let deadline = started + FINISH_RESPONSE_TIMEOUT;
        let mut final_texts = Vec::new();
        // The last unfinished sentence, kept when its final never arrives.
        let mut live: Option<String> = None;
        while tokio::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            match tokio::time::timeout(remaining, ws.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => match parse_server_message(&text)? {
                    ServerEvent::TaskFinished => break,
                    ServerEvent::Sentence { id, text, ended } => {
                        if ended {
                            live = None;
                            if self.mark_finished(id) {
                                final_texts.push(text);
                            }
                        } else {
                            live = Some(text);
                        }
                    }
                    ServerEvent::Failed(message) => return Err(AppError::Config(message)),
                    _ => {}
                },
                Ok(Some(Ok(Message::Close(frame)))) => {
                    tracing::warn!("Qwen-Audio closed before task-finished: {frame:?}");
                    break;
                }
                Ok(None) => {
                    tracing::warn!("Qwen-Audio stream ended before task-finished");
                    break;
                }
                Err(_) => {
                    tracing::warn!(
                        "Qwen-Audio gave no task-finished within {}s",
                        FINISH_RESPONSE_TIMEOUT.as_secs()
                    );
                    break;
                }
                Ok(Some(Ok(_))) => {}
                Ok(Some(Err(error))) => return Err(AppError::Network(error.to_string())),
            }
        }
        let _ = ws.close(None).await;
        if let Some(text) = live {
            tracing::warn!("Qwen-Audio kept the live text of a sentence that never got a final");
            final_texts.push(text);
        }
        tracing::info!(
            "Aliyun Qwen-Audio disconnected after {}ms with {} final segment(s)",
            started.elapsed().as_millis(),
            final_texts.len()
        );
        Ok((!final_texts.is_empty()).then(|| final_texts.join(" ")))
    }

    fn name(&self) -> &str {
        "Aliyun Qwen-Audio Streaming ASR"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    #[test]
    fn builds_the_workspace_endpoint_for_each_region() {
        assert_eq!(
            endpoint_url(Some(&region_setting("china-mainland", " ws-abc123 "))).unwrap(),
            "wss://ws-abc123.cn-beijing.maas.aliyuncs.com/api-ws/v1/inference"
        );
        assert_eq!(
            endpoint_url(Some(&region_setting("international", "ws-abc123"))).unwrap(),
            "wss://ws-abc123.ap-southeast-1.maas.aliyuncs.com/api-ws/v1/inference"
        );
    }

    #[test]
    fn extracts_the_id_from_a_pasted_host_or_url() {
        let expected = "wss://llm-abc.cn-beijing.maas.aliyuncs.com/api-ws/v1/inference";
        for pasted in [
            "llm-abc",
            "llm-abc.cn-beijing.maas.aliyuncs.com",
            "wss://llm-abc.cn-beijing.maas.aliyuncs.com/api-ws/v1/inference",
            "wss://<llm-abc>.cn-beijing.maas.aliyuncs.com/api-ws/v1/inference",
        ] {
            assert_eq!(
                endpoint_url(Some(&format!("china-mainland/{pasted}"))).unwrap(),
                expected,
                "{pasted}"
            );
        }
    }

    #[test]
    fn rejects_a_missing_or_unsafe_workspace_id() {
        assert!(endpoint_url(None).is_err());
        assert!(endpoint_url(Some("china-mainland/")).is_err());
        assert!(endpoint_url(Some("china-mainland/a b")).is_err());
        assert!(endpoint_url(Some("china-mainland/wss://")).is_err());
        assert!(endpoint_url(Some("mars/ws-1")).is_err());
    }

    #[test]
    fn parses_task_events() {
        assert_eq!(
            parse_server_message(r#"{"header":{"event":"task-started"},"payload":{}}"#).unwrap(),
            ServerEvent::TaskStarted
        );
        assert_eq!(
            parse_server_message(
                r#"{"header":{"event":"result-generated"},"payload":{"output":{"sentence":{"text":"你好","sentence_end":true,"sentence_id":2}}}}"#
            )
            .unwrap(),
            ServerEvent::Sentence {
                id: Some(2),
                text: "你好".to_string(),
                ended: true
            }
        );
        assert_eq!(
            parse_server_message(
                r#"{"header":{"event":"task-failed","error_code":"CLIENT_ERROR","error_message":"bad"}}"#
            )
            .unwrap(),
            ServerEvent::Failed("CLIENT_ERROR: bad".to_string())
        );
    }

    #[test]
    fn run_task_names_the_streaming_model() {
        let mut provider = AliyunQwenAudioProvider::new();
        provider.task_id = "task-1".to_string();
        let message = provider.run_task_message(16_000);
        assert_eq!(message["header"]["action"], "run-task");
        assert_eq!(message["payload"]["model"], ALIYUN_QWEN_AUDIO_MODEL);
        assert_eq!(message["payload"]["parameters"]["format"], "pcm");
        assert_eq!(message["payload"]["parameters"]["sample_rate"], 16_000);
    }

    #[tokio::test]
    async fn streams_audio_and_collects_final_sentences() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            let Some(Ok(Message::Text(run_task))) = ws.next().await else {
                panic!("expected run-task");
            };
            let run_task: serde_json::Value = serde_json::from_str(&run_task).unwrap();
            assert_eq!(run_task["header"]["action"], "run-task");
            ws.send(Message::Text(
                r#"{"header":{"event":"task-started"},"payload":{}}"#.to_string(),
            ))
            .await
            .unwrap();

            let Some(Ok(Message::Binary(audio))) = ws.next().await else {
                panic!("expected audio");
            };
            assert_eq!(audio.len(), AUDIO_CHUNK_BYTES);
            let Some(Ok(Message::Text(finish))) = ws.next().await else {
                panic!("expected finish-task");
            };
            assert!(finish.contains("finish-task"));
            for message in [
                r#"{"header":{"event":"result-generated"},"payload":{"output":{"sentence":{"text":"你好","sentence_end":true,"sentence_id":1}}}}"#,
                r#"{"header":{"event":"result-generated"},"payload":{"output":{"sentence":{"text":"世界","sentence_end":true,"sentence_id":2}}}}"#,
                r#"{"header":{"event":"task-finished"},"payload":{"output":{},"usage":null}}"#,
            ] {
                ws.send(Message::Text(message.to_string())).await.unwrap();
            }
        });

        let mut provider = AliyunQwenAudioProvider::new();
        let request = AliyunQwenAudioProvider::build_request(&format!("ws://{addr}"), "k").unwrap();
        let (ws, _) = connect_async(request).await.unwrap();
        provider.ws = Some(ws);
        provider.task_id = "t".to_string();
        let run_task = provider.run_task_message(16_000);
        AliyunQwenAudioProvider::send_text(provider.ws.as_mut().unwrap(), run_task)
            .await
            .unwrap();
        loop {
            let Some(Ok(Message::Text(text))) = provider.ws.as_mut().unwrap().next().await else {
                panic!("expected task-started");
            };
            if parse_server_message(&text).unwrap() == ServerEvent::TaskStarted {
                break;
            }
        }
        provider
            .send_audio(&vec![0u8; AUDIO_CHUNK_BYTES])
            .await
            .unwrap();
        let text = provider.disconnect().await.unwrap();
        assert_eq!(text.as_deref(), Some("你好 世界"));
        server.await.unwrap();
    }
}
