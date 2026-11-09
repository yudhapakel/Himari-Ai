use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use colored::*;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use rand::seq::SliceRandom;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Cursor};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

const OLLAMA_URL: &str = "http://127.0.0.1:11434/api/chat";
const MODEL_NAME: &str = "qwen2.5:14b";
const VOICEVOX_URL: &str = "http://127.0.0.1:50021";
const SPEAKER_ID: u32 = 14;

// --- Windows API FFI untuk Deteksi Gerak Mouse & Keyboard Global ---
#[repr(C)]
struct LASTINPUTINFO {
    cb_size: u32,
    dw_time: u32,
}

#[repr(C)]
struct SYSTEMTIME {
    w_year: u16,
    w_month: u16,
    w_day_of_week: u16,
    w_day: u16,
    w_hour: u16,
    w_minute: u16,
    w_second: u16,
    w_milliseconds: u16,
}

#[link(name = "user32")]
extern "system" {
    fn GetLastInputInfo(plii: *mut LASTINPUTINFO) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount() -> u32;
    fn GetLocalTime(lpSystemTime: *mut SYSTEMTIME);
}

fn get_windows_idle_millis() -> u64 {
    unsafe {
        let mut lii = LASTINPUTINFO {
            cb_size: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dw_time: 0,
        };
        if GetLastInputInfo(&mut lii) != 0 {
            let current_tick = GetTickCount();
            if current_tick >= lii.dw_time {
                (current_tick - lii.dw_time) as u64
            } else {
                0
            }
        } else {
            0
        }
    }
}

fn get_local_hour_minute() -> (u16, u16) {
    unsafe {
        let mut st = SYSTEMTIME {
            w_year: 0,
            w_month: 0,
            w_day_of_week: 0,
            w_day: 0,
            w_hour: 0,
            w_minute: 0,
            w_second: 0,
            w_milliseconds: 0,
        };
        GetLocalTime(&mut st);
        (st.w_hour, st.w_minute)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct MemoryData {
    user_name: String,
    facts: Vec<String>,
    conversation_summary: String,
}

impl Default for MemoryData {
    fn default() -> Self {
        Self {
            user_name: "Yudha".to_string(),
            facts: vec![
                "Yudha adalah pasangan / kekasih tersayang bagi Himari.".to_string(),
                "Yudha suka karakter kuudere yang tenang, lembut, dan sangat perhatian.".to_string(),
                "Himari memanggil Yudha dengan 'あなた' (Anata) atau 'Sayang' / 'Yudha'.".to_string(),
            ],
            conversation_summary: "Yudha dan Himari memiliki hubungan yang sangat dekat dan penuh kasih sayang.".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct HimariResponse {
    #[serde(default)]
    jp: String,
    #[serde(default)]
    id: String,
}

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    format: String,
    keep_alive: String,
    options: OllamaOptions,
}

#[derive(Serialize)]
struct OllamaOptions {
    num_ctx: u32,
    temperature: f32,
    top_p: f32,
}

#[derive(Deserialize)]
struct OllamaResponse {
    message: ChatMessage,
}

#[derive(PartialEq, Debug, Clone, Copy)]
enum AppState {
    Active,
    FirstWarning,
    Standby,
    VoiceSleep,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct SharedGuiState {
    status_text: String,
    sub_jp: String,
    sub_id: String,
    mic_device: String,
    is_speaking: bool,
    is_thinking: bool,
    is_muted: bool,
    is_sleeping: bool,
}

impl Default for SharedGuiState {
    fn default() -> Self {
        Self {
            status_text: "STANDBY".to_string(),
            sub_jp: "".to_string(),
            sub_id: "Menghubungkan ke Qwen 14B...".to_string(),
            mic_device: "Mic IEM (USB Audio)".to_string(),
            is_speaking: false,
            is_thinking: false,
            is_muted: false,
            is_sleeping: false,
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
struct NotifyPayload {
    message: String,
    #[serde(default)]
    title: Option<String>,
}

struct AppContext {
    gui_state: Arc<Mutex<SharedGuiState>>,
    tx_action: tokio::sync::mpsc::Sender<char>,
    tx_notify: tokio::sync::mpsc::Sender<String>,
}

fn get_memory_path() -> PathBuf {
    dirs::download_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("himari_memory.json")
}

fn load_memory() -> MemoryData {
    let path = get_memory_path();
    if path.exists() {
        if let Ok(file) = std::fs::File::open(&path) {
            if let Ok(data) = serde_json::from_reader(file) {
                return data;
            }
        }
    }
    MemoryData::default()
}

fn build_system_prompt(mem: &MemoryData) -> String {
    let facts = mem.facts.iter().map(|f| format!("- {}", f)).collect::<Vec<_>>().join("\n");
    format!(
        r#"You are Meimei Himari (冥鳴ひまり), an adorable, calm, soft-spoken, loving and quietly caring kuudere anime girlfriend/companion talking with Yudha.
Yudha is your beloved boyfriend/partner. You address him affectionately as あなた (Anata) in Japanese, and "Sayang" or "Yudha" in Indonesian.

LONG-TERM MEMORY ABOUT YUDHA:
{}

STRICT RULES:
1. "jp": Pure, natural, gentle Japanese (1-2 short sentences max). NEVER leave "jp" empty! Address him lovingly as あなた (Anata).
2. "id": Must be in NATURAL CASUAL, SWEET INDONESIAN with Latin alphabet. Address him as "Sayang" or "Yudha". NEVER output Japanese/Kanji in "id"!
3. Tone: Calm kuudere, gentle, slightly shy but deeply devoted, warm, and romantic.
4. Response MUST be valid JSON only.

Example:
{{"jp": "あなた、お疲れ様です。ずっとそばにいますからね。", "id": "Sayang, istirahat ya. Aku akan selalu ada di sampingmu."}}"#,
        facts
    )
}

fn clean_indonesian_subtitle(text: &str, jp_text: &str) -> String {
    let japanese_regex = Regex::new(r"[\p{Hiragana}\p{Katakana}\p{Han}]").unwrap();
    let jp_chars = text.chars().filter(|c| japanese_regex.is_match(&c.to_string())).count();
    let total_chars = text.chars().count();
    
    if (total_chars > 0 && (jp_chars as f32 / total_chars as f32) > 0.3) || text.trim().is_empty() {
        if jp_text.contains("大丈夫") {
            "Sayang, apa kamu baik-baik saja?".to_string()
        } else if jp_text.contains("お疲れ") || jp_text.contains("休") {
            "Sayang, istirahat yang cukup ya...".to_string()
        } else {
            "Aku selalu ada di sampingmu kok, Sayang.".to_string()
        }
    } else {
        text.to_string()
    }
}

async fn fetch_weather_info() -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .ok()?;
    let res = client.get("https://wttr.in/?format=3").send().await.ok()?;
    if res.status().is_success() {
        let text = res.text().await.ok()?;
        Some(text.trim().to_string())
    } else {
        None
    }
}

async fn play_voicevox(client: &reqwest::Client, text_jp: &str, gui: Arc<Mutex<SharedGuiState>>) {
    if text_jp.trim().is_empty() {
        return;
    }
    
    let query_url = format!("{}/audio_query?text={}&speaker={}", VOICEVOX_URL, urlencoding::encode(text_jp), SPEAKER_ID);
    let query_res = match client.post(&query_url).send().await {
        Ok(res) if res.status().is_success() => res.json::<serde_json::Value>().await.ok(),
        _ => None,
    };

    let Some(query_data) = query_res else { return; };

    let synth_url = format!("{}/synthesis?speaker={}", VOICEVOX_URL, SPEAKER_ID);
    let synth_res = match client.post(&synth_url).json(&query_data).send().await {
        Ok(res) if res.status().is_success() => res.bytes().await.ok(),
        _ => None,
    };

    let Some(wav_bytes) = synth_res else { return; };

    {
        let mut g = gui.lock().unwrap();
        g.is_speaking = true;
    }

    tokio::task::spawn_blocking(move || {
        if let Ok((_stream, stream_handle)) = rodio::OutputStream::try_default() {
            if let Ok(sink) = rodio::Sink::try_new(&stream_handle) {
                let cursor = Cursor::new(wav_bytes);
                if let Ok(source) = rodio::Decoder::new(cursor) {
                    sink.append(source);
                    sink.sleep_until_end();
                }
            }
        }
    })
    .await
    .ok();

    {
        let mut g = gui.lock().unwrap();
        g.is_speaking = false;
    }
}

async fn query_ollama(
    client: &reqwest::Client,
    messages: &[ChatMessage],
    gui: Arc<Mutex<SharedGuiState>>,
) -> Option<(String, String)> {
    {
        let mut g = gui.lock().unwrap();
        g.is_thinking = true;
    }

    let req = OllamaRequest {
        model: MODEL_NAME.to_string(),
        messages: messages.to_vec(),
        stream: false,
        format: "json".to_string(),
        keep_alive: "2h".to_string(),
        options: OllamaOptions {
            num_ctx: 8192,
            temperature: 0.6,
            top_p: 0.9,
        },
    };

    let res = client
        .post(OLLAMA_URL)
        .json(&req)
        .timeout(Duration::from_secs(60))
        .send()
        .await;

    {
        let mut g = gui.lock().unwrap();
        g.is_thinking = false;
        g.status_text = "ONLINE".to_string();
    }

    let res = res.ok()?;
    if !res.status().is_success() {
        return None;
    }

    let ollama_res: OllamaResponse = res.json().await.ok()?;
    let content = ollama_res.message.content.trim();
    
    if let Ok(h) = serde_json::from_str::<HimariResponse>(content) {
        let jp = if h.jp.trim().is_empty() {
            "あなた、お疲れ様です。".to_string()
        } else {
            h.jp
        };
        let id_clean = clean_indonesian_subtitle(&h.id, &jp);
        Some((jp, id_clean))
    } else {
        Some(("あなた、どうしましたか？".to_string(), "Sayang, ada apa?".to_string()))
    }
}

struct MicController {
    child: Option<Child>,
    current_device_mode: usize,
}

impl MicController {
    fn new() -> Self {
        Self {
            child: None,
            current_device_mode: 0,
        }
    }

    fn device_name(&self) -> &'static str {
        if self.current_device_mode == 0 {
            "Mic IEM (Kabel USB Audio)"
        } else {
            "Mic HP Wireless (WO Mic)"
        }
    }

    fn toggle_device(&mut self) -> &'static str {
        self.stop();
        self.current_device_mode = if self.current_device_mode == 0 { 1 } else { 0 };
        self.device_name()
    }

    fn start(&mut self, tx: tokio::sync::mpsc::Sender<String>) {
        if self.child.is_some() {
            return;
        }

        let mode = self.current_device_mode;
        let py_script = format!(
            r#"
import speech_recognition as sr
import sys

mode = {mode}
target_idx = 1

mics = sr.Microphone.list_microphone_names()
if mode == 0:
    for idx, name in enumerate(mics):
        if 'usb audio' in name.lower() and 'micro' in name.lower():
            target_idx = idx
            break
else:
    found = False
    for idx, name in enumerate(mics):
        if any(k in name.lower() for k in ['wo mic', 'womic', 'relay', 'virtual']):
            target_idx = idx
            found = True
            break
    if not found:
        for idx, name in enumerate(mics):
            if 'camera' in name.lower():
                target_idx = idx
                found = True
                break
    if not found:
        target_idx = 2

r = sr.Recognizer()
r.pause_threshold = 1.2
r.non_speaking_duration = 0.7
r.energy_threshold = 400
r.dynamic_energy_threshold = False

try:
    mic = sr.Microphone(device_index=target_idx)
except Exception:
    mic = sr.Microphone()

with mic as source:
    r.adjust_for_ambient_noise(source, duration=0.6)
    while True:
        try:
            audio = r.listen(source, timeout=3, phrase_time_limit=12)
            text = r.recognize_google(audio, language="id-ID")
            if text and len(text.strip()) > 1:
                print(text.strip(), flush=True)
        except Exception:
            pass
"#
        );

        let mut child = Command::new("python")
            .arg("-c")
            .arg(py_script)
            .stdout(Stdio::piped())
            .spawn()
            .expect("Gagal menjalankan mic worker");

        let stdout = child.stdout.take().unwrap();
        self.child = Some(child);

        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                if let Ok(l) = line {
                    let cleaned = l.trim().to_string();
                    if !cleaned.is_empty() {
                        let _ = tx.send(cleaned).await;
                    }
                }
            }
        });
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn is_voice_sleep_command(text: &str) -> bool {
    let t = text.to_lowercase();
    let keywords = [
        "sleep",
        "slipknot",
        "mode sleep",
        "masuk sleep",
        "selamat tidur",
        "mau tidur",
        "tidur dulu",
        "mode tidur",
        "mute",
        "5000",
        "lima ribu",
        "diam dulu",
        "diam ya",
        "jangan bicara",
        "istirahat dulu",
        "nggak ngomong",
        "gak ngomong",
        "fokus dulu",
        "jangan berisik",
        "senyap",
    ];
    keywords.iter().any(|&k| t.contains(k))
}

fn is_wake_up_command(text: &str) -> bool {
    let t = text.to_lowercase();
    let keywords = [
        "himari",
        "hai himari",
        "halo himari",
        "bangun",
        "unmute",
        "bisa dengar",
        "aku kembali",
        "udah selesai",
        "dengerin aku",
        "sayang",
    ];
    keywords.iter().any(|&k| t.contains(k))
}

fn is_voice_switch_mic_command(text: &str) -> bool {
    let t = text.to_lowercase();
    let keywords = [
        "switch",
        "swit",
        "ganti mic",
        "switch mic",
        "switch mike",
        "pindah mic",
        "ganti ke komputer",
        "ganti ke pc",
        "ganti ke hp",
    ];
    keywords.iter().any(|&k| t.contains(k))
}

fn is_weather_inquiry(text: &str) -> bool {
    let t = text.to_lowercase();
    t.contains("cuaca") || t.contains("suhu") || t.contains("hujan") || t.contains("panas di luar") || t.contains("mendung") || t.contains("pagi")
}

fn parse_timer_seconds(text: &str) -> Option<(u64, String)> {
    let t = text.to_lowercase();
    let re_menit = Regex::new(r"(\d+)\s*(?:menit|mnt)").unwrap();
    let re_detik = Regex::new(r"(\d+)\s*(?:detik|dtk)").unwrap();

    if let Some(caps) = re_menit.captures(&t) {
        if let Ok(m) = caps[1].parse::<u64>() {
            return Some((m * 60, format!("{} menit", m)));
        }
    }
    if let Some(caps) = re_detik.captures(&t) {
        if let Ok(s) = caps[1].parse::<u64>() {
            return Some((s, format!("{} detik", s)));
        }
    }
    None
}

#[tokio::main]
async fn main() {
    println!("{}", "====================================================================".magenta());
    println!("{}", "🦀 HIMARI ENGINE v7.0: SMART HEALTH & CRON EDITION 🦀".bold().magenta());
    println!("{}", "   [Edge App Window • Win32 Input Hook • Scheduled Cron • Webhook]".cyan());
    println!("{}", "====================================================================".magenta());

    let gui_state = Arc::new(Mutex::new(SharedGuiState::default()));
    let (tx_action, mut rx_action) = tokio::sync::mpsc::channel::<char>(16);
    let (tx_notify, mut rx_notify) = tokio::sync::mpsc::channel::<String>(32);

    let app_context = Arc::new(AppContext {
        gui_state: Arc::clone(&gui_state),
        tx_action: tx_action.clone(),
        tx_notify: tx_notify.clone(),
    });

    // 1. Jalankan Axum Web Server di Background (Port 8765)
    let app_state_clone = Arc::clone(&app_context);
    let app = Router::new()
        .route("/api/state", get(|State(ctx): State<Arc<AppContext>>| async move {
            let data = ctx.gui_state.lock().unwrap().clone();
            Json(data)
        }))
        .route("/api/action/mute", post(|State(ctx): State<Arc<AppContext>>| async move {
            let _ = ctx.tx_action.send('m').await;
            StatusCode::OK
        }))
        .route("/api/action/switch", post(|State(ctx): State<Arc<AppContext>>| async move {
            let _ = ctx.tx_action.send('s').await;
            StatusCode::OK
        }))
        .route("/api/action/exit", post(|State(ctx): State<Arc<AppContext>>| async move {
            let _ = ctx.tx_action.send('q').await;
            StatusCode::OK
        }))
        .route("/api/notify", post(|State(ctx): State<Arc<AppContext>>, Json(payload): Json<NotifyPayload>| async move {
            println!("{}", format!("🔔 Webhook notification received: {}", payload.message).yellow());
            let _ = ctx.tx_notify.send(payload.message).await;
            StatusCode::OK
        }))
        .fallback_service(ServeDir::new("web"))
        .layer(CorsLayer::permissive())
        .with_state(app_state_clone);

    // Bind ke 0.0.0.0:8765 agar bisa menerima webhook dari Homeserver lewat Tailscale IP
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8765").await.unwrap();
    println!("{}", "🌐 HUD Web Server & Webhook aktif di: http://0.0.0.0:8765".green());

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // 2. Buka Jendela App Mode menggunakan Microsoft Edge (Frameless, Clean GUI)
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(600)).await;
        let _ = Command::new("cmd")
            .arg("/c")
            .arg("start msedge --app=http://127.0.0.1:8765 --window-size=780,600")
            .spawn();
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap();

    let memory = load_memory();
    let system_prompt = build_system_prompt(&memory);
    let mut chat_history: Vec<ChatMessage> = vec![ChatMessage {
        role: "system".to_string(),
        content: system_prompt,
    }];

    let (tx_speech, mut rx_speech) = tokio::sync::mpsc::channel::<String>(32);
    let mut mic_controller = MicController::new();
    mic_controller.start(tx_speech.clone());

    let mut is_hardware_muted = false;
    let mut app_state = AppState::Active;
    let mut last_interaction = Instant::now();
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    let gui = Arc::clone(&gui_state);

    {
        let mut g = gui.lock().unwrap();
        g.status_text = "ONLINE".to_string();
        g.sub_id = "Himari siap menemani Sayang...".to_string();
    }

    println!("{}", "✨ Himari Engine Siap! [M]: Mute | [S]: Switch Mic | [ESC]: Keluar".cyan());

    // --- State Tracker untuk Health Monitor & Internal Cronjob ---
    let mut last_activity_check = Instant::now();
    let mut continuous_active_seconds: u64 = 0;
    let mut last_eye_warning = Instant::now();

    let mut last_cron_morning = false;
    let mut last_cron_lunch = false;
    let mut last_cron_dinner = false;
    let mut current_day_marker: u16 = 999;

    loop {
        tokio::select! {
            // Menerima Webhook Notifikasi dari Luar (misal Homeserver Asus via Tailscale atau Internal Cron)
            Some(notify_text) = rx_notify.recv() => {
                println!("{}", format!("📢 Memproses Notifikasi: {}", notify_text).cyan());
                mic_controller.stop();
                while rx_speech.try_recv().is_ok() {}

                {
                    let mut g = gui.lock().unwrap();
                    g.sub_id = format!("Notifikasi: {}", notify_text);
                    g.status_text = "NOTIFICATION".to_string();
                }

                let notify_prompt = vec![
                    chat_history[0].clone(),
                    ChatMessage {
                        role: "user".to_string(),
                        content: format!(
                            "[SISTEM: Sampaikan notifikasi penting berikut kepada Yudha (Sayang) dengan lembut dan manis: '{}'. Panggil dia dengan あなた (Anata) atau Sayang.]",
                            notify_text
                        ),
                    },
                ];

                if let Some((jp, id)) = query_ollama(&client, &notify_prompt, Arc::clone(&gui)).await {
                    {
                        let mut g = gui.lock().unwrap();
                        g.sub_jp = jp.clone();
                        g.sub_id = id.clone();
                    }
                    play_voicevox(&client, &jp, Arc::clone(&gui)).await;
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                }

                mic_controller.start(tx_speech.clone());
                last_interaction = Instant::now();
                {
                    let mut g = gui.lock().unwrap();
                    g.status_text = "ONLINE".to_string();
                }
            }

            // Keyboard Hook
            res = tokio::task::spawn_blocking(|| {
                event::poll(Duration::from_millis(50)).unwrap_or(false)
            }) => {
                if res.unwrap_or(false) {
                    if let Ok(Event::Key(key)) = event::read() {
                        if key.kind == KeyEventKind::Press {
                            match key.code {
                                KeyCode::Char('m') | KeyCode::Char('M') => {
                                    is_hardware_muted = !is_hardware_muted;
                                    {
                                        let mut g = gui.lock().unwrap();
                                        g.is_muted = is_hardware_muted;
                                        g.status_text = if is_hardware_muted { "MIC MUTED".to_string() } else { "ONLINE".to_string() };
                                    }
                                    if is_hardware_muted {
                                        mic_controller.stop();
                                        while rx_speech.try_recv().is_ok() {}
                                        println!("{}", "🔴 HARDWARE MUTE: Mic dimatikan.".red().bold());
                                    } else {
                                        println!("{}", "🟢 HARDWARE UNMUTE: Mic diaktifkan.".green().bold());
                                        mic_controller.start(tx_speech.clone());
                                        last_interaction = Instant::now();
                                    }
                                }
                                KeyCode::Char('s') | KeyCode::Char('S') => {
                                    let new_name = mic_controller.toggle_device();
                                    {
                                        let mut g = gui.lock().unwrap();
                                        g.mic_device = new_name.to_string();
                                    }
                                    println!("{}", format!("🔄 Mic ditukar: {}", new_name).yellow().bold());
                                    if !is_hardware_muted {
                                        mic_controller.start(tx_speech.clone());
                                    }
                                }
                                KeyCode::Esc => {
                                    println!("{}", "👋 Keluar dari Himari Engine.".yellow());
                                    mic_controller.stop();
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            // Action dari Web UI Button
            Some(act) = rx_action.recv() => {
                match act {
                    'm' => {
                        is_hardware_muted = !is_hardware_muted;
                        {
                            let mut g = gui.lock().unwrap();
                            g.is_muted = is_hardware_muted;
                            g.status_text = if is_hardware_muted { "MIC MUTED".to_string() } else { "ONLINE".to_string() };
                        }
                        if is_hardware_muted {
                            mic_controller.stop();
                            while rx_speech.try_recv().is_ok() {}
                            println!("{}", "🔴 Web UI: Mic dimatikan.".red().bold());
                        } else {
                            println!("{}", "🟢 Web UI: Mic diaktifkan.".green().bold());
                            mic_controller.start(tx_speech.clone());
                            last_interaction = Instant::now();
                        }
                    }
                    's' => {
                        let new_name = mic_controller.toggle_device();
                        {
                            let mut g = gui.lock().unwrap();
                            g.mic_device = new_name.to_string();
                        }
                        println!("{}", format!("🔄 Web UI: Mic ditukar ke {}", new_name).yellow().bold());
                        if !is_hardware_muted {
                            mic_controller.start(tx_speech.clone());
                        }
                    }
                    'q' => {
                        println!("{}", "👋 Keluar via Web UI.".yellow());
                        mic_controller.stop();
                        break;
                    }
                    _ => {}
                }
            }

            // Input Suara Pengguna
            Some(user_text) = rx_speech.recv(), if !is_hardware_muted => {
                if app_state == AppState::VoiceSleep {
                    if is_wake_up_command(&user_text) {
                        mic_controller.stop();
                        while rx_speech.try_recv().is_ok() {}

                        {
                            let mut g = gui.lock().unwrap();
                            g.is_sleeping = false;
                            g.status_text = "ONLINE".to_string();
                        }

                        let wake_prompt = vec![
                            chat_history[0].clone(),
                            ChatMessage {
                                role: "user".to_string(),
                                content: "[SISTEM: Sayang (Yudha) memanggilmu untuk bangun. Ucapkan kalimat Jepang 1 kalimat lembut, manja, dan manis: はい、あなた！いつでもお話しできますよ。]".to_string(),
                            },
                        ];

                        if let Some((jp, id)) = query_ollama(&client, &wake_prompt, Arc::clone(&gui)).await {
                            {
                                let mut g = gui.lock().unwrap();
                                g.sub_jp = jp.clone();
                                g.sub_id = id.clone();
                            }
                            play_voicevox(&client, &jp, Arc::clone(&gui)).await;
                            tokio::time::sleep(Duration::from_millis(1000)).await;
                        }

                        app_state = AppState::Active;
                        mic_controller.start(tx_speech.clone());
                        last_interaction = Instant::now();

                        {
                            let mut g = gui.lock().unwrap();
                            g.status_text = "LISTENING".to_string();
                        }
                    }
                    continue;
                }

                mic_controller.stop();
                while rx_speech.try_recv().is_ok() {}

                {
                    let mut g = gui.lock().unwrap();
                    g.sub_id = format!("Sayang: \"{}\"", user_text);
                    g.status_text = "PROCESSING".to_string();
                }

                // 1. Fitur Perintah Ganti Mic
                if is_voice_switch_mic_command(&user_text) {
                    let new_name = mic_controller.toggle_device();
                    {
                        let mut g = gui.lock().unwrap();
                        g.mic_device = new_name.to_string();
                    }
                    
                    if let Some((jp, id)) = query_ollama(&client, &[
                        chat_history[0].clone(),
                        ChatMessage { role: "user".to_string(), content: "Ucapkan konfirmasi 1 kalimat manis: マイクを切り替えましたよ、あなた。".to_string() }
                    ], Arc::clone(&gui)).await {
                        {
                            let mut g = gui.lock().unwrap();
                            g.sub_jp = jp.clone();
                            g.sub_id = id.clone();
                        }
                        play_voicevox(&client, &jp, Arc::clone(&gui)).await;
                        tokio::time::sleep(Duration::from_millis(1000)).await;
                    }

                    mic_controller.start(tx_speech.clone());
                    last_interaction = Instant::now();
                    {
                        let mut g = gui.lock().unwrap();
                        g.status_text = "LISTENING".to_string();
                    }
                    continue;
                }

                // 2. Fitur Perintah Sleep Mode
                if is_voice_sleep_command(&user_text) {
                    app_state = AppState::VoiceSleep;

                    {
                        let mut g = gui.lock().unwrap();
                        g.is_sleeping = true;
                        g.status_text = "SLEEP MODE".to_string();
                    }

                    let sleep_prompt = vec![
                        chat_history[0].clone(),
                        ChatMessage {
                            role: "user".to_string(),
                            content: "[SISTEM: Sayang memintamu tidur/diam dulu. Ucapkan kalimat Jepang 1 kalimat sangat lembut: わかりました、静かに待っていますね、あなた。おやすみなさい。]".to_string(),
                        },
                    ];

                    if let Some((jp, id)) = query_ollama(&client, &sleep_prompt, Arc::clone(&gui)).await {
                        {
                            let mut g = gui.lock().unwrap();
                            g.sub_jp = jp.clone();
                            g.sub_id = id.clone();
                        }
                        play_voicevox(&client, &jp, Arc::clone(&gui)).await;
                        tokio::time::sleep(Duration::from_millis(1000)).await;
                    }

                    mic_controller.start(tx_speech.clone());
                    continue;
                }

                // 3. Fitur Pengingat Timer / Bangunkan
                if user_text.to_lowercase().contains("ingetin") || user_text.to_lowercase().contains("bangunin") || user_text.to_lowercase().contains("timer") {
                    if let Some((secs, label)) = parse_timer_seconds(&user_text) {
                        let notify_tx = tx_notify.clone();
                        let timer_label = label.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_secs(secs)).await;
                            let _ = notify_tx.send(format!("Waktunya bangun / waktu pengingat {} sudah habis!", timer_label)).await;
                        });

                        let confirm_prompt = vec![
                            chat_history[0].clone(),
                            ChatMessage {
                                role: "user".to_string(),
                                content: format!(
                                    "[SISTEM: Sayang meminta diingatkan dalam {}. Beri konfirmasi 1 kalimat sangat manis bahwa kamu akan mengingatkannya tepat waktu.]",
                                    label
                                ),
                            },
                        ];

                        if let Some((jp, id)) = query_ollama(&client, &confirm_prompt, Arc::clone(&gui)).await {
                            {
                                let mut g = gui.lock().unwrap();
                                g.sub_jp = jp.clone();
                                g.sub_id = id.clone();
                            }
                            play_voicevox(&client, &jp, Arc::clone(&gui)).await;
                            tokio::time::sleep(Duration::from_millis(1000)).await;
                        }

                        mic_controller.start(tx_speech.clone());
                        last_interaction = Instant::now();
                        {
                            let mut g = gui.lock().unwrap();
                            g.status_text = "LISTENING".to_string();
                        }
                        continue;
                    }
                }

                // 4. Fitur Cek Cuaca Fleksibel (Kapan Saja Tanpa Harus Sapaan Pagi)
                let mut actual_user_prompt = user_text.clone();
                if is_weather_inquiry(&user_text) {
                    if let Some(weather) = fetch_weather_info().await {
                        actual_user_prompt = format!(
                            "{} [INFO DATA CUACA REAL-TIME SAAT INI: {}. Sampaikan informasi cuaca ini kepada Sayang dengan ramah, hangat, dan manis.]",
                            user_text, weather
                        );
                    }
                }

                chat_history.push(ChatMessage {
                    role: "user".to_string(),
                    content: actual_user_prompt,
                });

                if chat_history.len() > 15 {
                    chat_history = [vec![chat_history[0].clone()], chat_history[chat_history.len()-14..].to_vec()].concat();
                }

                if let Some((jp, id)) = query_ollama(&client, &chat_history, Arc::clone(&gui)).await {
                    {
                        let mut g = gui.lock().unwrap();
                        g.sub_jp = jp.clone();
                        g.sub_id = id.clone();
                    }
                    
                    chat_history.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: format!(r#"{{"jp": "{}", "id": "{}"}}"#, jp, id),
                    });

                    play_voicevox(&client, &jp, Arc::clone(&gui)).await;
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                }

                mic_controller.start(tx_speech.clone());
                last_interaction = Instant::now();
                {
                    let mut g = gui.lock().unwrap();
                    g.status_text = "LISTENING".to_string();
                }
            }

            // Loop 1 Detik: Proactive Standby Timer + Win32 Mouse/Keyboard Screen Time Check + Cronjob Waktu
            _ = interval.tick() => {
                // A. Deteksi Gerak Mouse & Keyboard Windows Global via GetLastInputInfo
                let idle_ms = get_windows_idle_millis();
                if idle_ms < 60_000 {
                    // Ada aktivitas fisik mouse / keyboard dalam 60 detik terakhir
                    let delta = last_activity_check.elapsed().as_secs();
                    continuous_active_seconds += delta;
                } else if idle_ms > 300_000 {
                    // User meninggalkan meja / idle lebih dari 5 menit -> reset timer lelah mata
                    continuous_active_seconds = 0;
                }
                last_activity_check = Instant::now();

                // Jika user terus aktif di depan layar selama 90 menit (5400 detik) tanpa istirahat:
                if continuous_active_seconds >= 5400 && last_eye_warning.elapsed().as_secs() >= 3600 {
                    last_eye_warning = Instant::now();
                    let _ = tx_notify.send("Sayang, kamu sudah bekerja di depan layar cukup lama. Istirahatkan matamu sejenak dan minum air putih ya...".to_string()).await;
                }

                // B. Cronjob Waktu Otomatis (Jam 08:00 Pagi, Jam 12:00 Siang, Jam 19:00 Malam)
                let (hour, minute) = get_local_hour_minute();
                
                // Reset flag harian saat tengah malam
                if hour == 0 && minute == 0 && current_day_marker != 0 {
                    last_cron_morning = false;
                    last_cron_lunch = false;
                    last_cron_dinner = false;
                    current_day_marker = 0;
                } else if hour != 0 {
                    current_day_marker = hour;
                }

                // Jam 08:00 Pagi: Sapaan Selamat Pagi & Sarapan
                if hour == 8 && minute == 0 && !last_cron_morning {
                    last_cron_morning = true;
                    let weather_str = fetch_weather_info().await.unwrap_or_else(|| "Cuaca cerah berawan".to_string());
                    let _ = tx_notify.send(format!("Selamat pagi Sayang! Jangan lupa sarapan sebelum mulai aktivitas ya. Kondisi cuaca hari ini: {}.", weather_str)).await;
                }

                // Jam 12:00 Siang: Pengingat Makan Siang
                if hour == 12 && minute == 0 && !last_cron_lunch {
                    last_cron_lunch = true;
                    let _ = tx_notify.send("Sayang, sudah jam 12 siang lho. Waktunya istirahat dan makan siang dulu ya...".to_string()).await;
                }

                // Jam 19:00 Malam: Pengingat Makan Malam
                if hour == 19 && minute == 0 && !last_cron_dinner {
                    last_cron_dinner = true;
                    let _ = tx_notify.send("Sayang, hari sudah malam. Jangan lupa makan malam ya, jaga kesehatanmu...".to_string()).await;
                }

                // C. Proactive Idle Standby State Machine
                if !is_hardware_muted && app_state != AppState::VoiceSleep {
                    let elapsed = last_interaction.elapsed().as_secs();

                    match app_state {
                        AppState::Active => {
                            if elapsed >= 30 {
                                app_state = AppState::FirstWarning;

                                mic_controller.stop();
                                while rx_speech.try_recv().is_ok() {}

                                let idle_prompts = [
                                    "Sayang terdiam cukup lama. Tanyakan dengan lembut apakah Sayang sedang melamun atau lelah.",
                                    "Suasana hening. Tanyakan Sayang sedang memikirkan apa.",
                                    "Sayang diam saja. Ingatkan Sayang untuk minum air hangat secara manis.",
                                ];
                                let chosen = idle_prompts.choose(&mut rand::thread_rng()).unwrap();
                                let prompt = format!("[SISTEM: {} Panggil dengan あなた (Anata) atau Sayang. Maksimal 1 kalimat.]", chosen);

                                let mut temp_hist = chat_history.clone();
                                temp_hist.push(ChatMessage { role: "system".to_string(), content: prompt });

                                if let Some((jp, id)) = query_ollama(&client, &temp_hist, Arc::clone(&gui)).await {
                                    {
                                        let mut g = gui.lock().unwrap();
                                        g.sub_jp = jp.clone();
                                        g.sub_id = id.clone();
                                    }
                                    play_voicevox(&client, &jp, Arc::clone(&gui)).await;
                                    tokio::time::sleep(Duration::from_millis(1000)).await;
                                }

                                mic_controller.start(tx_speech.clone());
                                last_interaction = Instant::now();
                            }
                        }
                        AppState::FirstWarning => {
                            if elapsed >= 30 {
                                app_state = AppState::Standby;

                                mic_controller.stop();
                                while rx_speech.try_recv().is_ok() {}

                                let standby_prompt = "[SISTEM: Sayang tidak bersuara selama 1 menit penuh. Tinggalkan 1 pesan terakhir yang manis bahwa kamu akan menunggu dalam diam/standby di sini sampai Sayang memanggil lagi. Panggil あなた (Anata).]";

                                let mut temp_hist = chat_history.clone();
                                temp_hist.push(ChatMessage { role: "system".to_string(), content: standby_prompt.to_string() });

                                if let Some((jp, id)) = query_ollama(&client, &temp_hist, Arc::clone(&gui)).await {
                                    {
                                        let mut g = gui.lock().unwrap();
                                        g.sub_jp = jp.clone();
                                        g.sub_id = id.clone();
                                        g.status_text = "STANDBY".to_string();
                                    }
                                    play_voicevox(&client, &jp, Arc::clone(&gui)).await;
                                    tokio::time::sleep(Duration::from_millis(1000)).await;
                                }

                                mic_controller.start(tx_speech.clone());
                                last_interaction = Instant::now();
                                {
                                    let mut g = gui.lock().unwrap();
                                    g.status_text = "STANDBY".to_string();
                                }
                            }
                        }
                        AppState::Standby | AppState::VoiceSleep => {}
                    }
                }
            }
        }
    }
}
