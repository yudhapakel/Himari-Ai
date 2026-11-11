use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MemoryData {
    pub user_name: String,
    pub facts: Vec<String>,
    #[serde(default)]
    pub routines: Vec<String>,
    #[serde(default)]
    pub preferences: Vec<String>,
    #[serde(default)]
    pub emotional_bonds: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct HimariResponse {
    #[serde(default)]
    pub jp: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub kaomoji: String,
}

#[derive(Serialize)]
pub struct OllamaRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    pub format: String,
    pub options: OllamaOptions,
}

#[derive(Serialize)]
pub struct OllamaOptions {
    pub temperature: f32,
    pub num_predict: i32,
    pub repeat_penalty: f32,
}

#[derive(Deserialize, Debug)]
pub struct OllamaResponse {
    pub message: ChatMessage,
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum AppState {
    Active,
    FirstWarning,
    SecondWarning,
    VoiceSleep,
}

#[derive(Serialize, Clone, Debug)]
pub struct ChatHistoryEntry {
    pub role: String,
    pub text: String,
    pub time: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct SharedGuiState {
    pub sub_jp: String,
    pub sub_id: String,
    pub is_speaking: bool,
    pub kaomoji: String,
    pub is_sleeping: bool,
    pub is_listening: bool,
    pub status_text: String,
    pub is_muted: bool,
    pub chat_history: Vec<ChatHistoryEntry>,
    pub live_transcript: String,
    pub active_mic_index: usize,
    pub active_mic_name: String,
    pub sleep_timer_remaining: Option<u64>,
    pub active_app_badge: String,
    pub gpu_health_badge: String,
    pub vram_health_badge: String,
}

impl Default for SharedGuiState {
    fn default() -> Self {
        Self {
            sub_jp: "...".to_string(),
            sub_id: "Himari siap mendengarkan...".to_string(),
            is_speaking: false,
            kaomoji: "(´｡• ᵕ •｡`)".to_string(),
            is_sleeping: false,
            is_listening: false,
            status_text: "READY".to_string(),
            is_muted: false,
            chat_history: Vec::new(),
            live_transcript: String::new(),
            active_mic_index: 0,
            active_mic_name: "Default Microphone".to_string(),
            sleep_timer_remaining: None,
            active_app_badge: "DESKTOP".to_string(),
            gpu_health_badge: "--".to_string(),
            vram_health_badge: "--".to_string(),
        }
    }
}

#[derive(Deserialize)]
pub struct TextChatPayload {
    pub text: String,
}

#[derive(Deserialize)]
pub struct NotifyPayload {
    pub text: String,
}

#[derive(Clone)]
pub struct AppContext {
    pub gui: Arc<Mutex<SharedGuiState>>,
    pub tx_notify: mpsc::Sender<String>,
    pub tx_user_intent: mpsc::Sender<String>,
}
