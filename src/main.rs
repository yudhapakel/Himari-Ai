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

#[derive(Serialize, Deserialize, Clone, Debug)]
struct MemoryData {
    user_name: String,
    facts: Vec<String>,
    conversation_summary: String,
}

impl Default for MemoryData {
    fn default() -> Self {
        Self {
            user_name: "Yudha (Master)".to_string(),
            facts: vec![
                "Yudha adalah programmer dan Master bagi Himari.".to_string(),
                "Yudha suka karakter kuudere yang tenang dan perhatian.".to_string(),
                "Yudha sedang tidak enak badan / istirahat hari ini.".to_string(),
            ],
            conversation_summary: "Yudha dan Himari sudah saling akrab.".to_string(),
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

struct AppContext {
    gui_state: Arc<Mutex<SharedGuiState>>,
    tx_action: tokio::sync::mpsc::Sender<char>,
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
        r#"You are Meimei Himari (冥鳴ひまり), an adorable, calm, soft-spoken, and quietly caring kuudere anime companion talking with Yudha.
Yudha is your Master. He speaks in casual Indonesian or English.

LONG-TERM MEMORY ABOUT MASTER YUDHA:
{}

STRICT RULES:
1. "jp": Pure, natural, gentle Japanese (1-2 short sentences max). NEVER leave "jp" empty! Address him as マスター (Master).
2. "id": Must be in NATURAL CASUAL INDONESIAN with Latin alphabet. NEVER output Japanese/Kanji in "id"!
3. Response MUST be valid JSON only.

Example:
{{"jp": "マスター、お疲れ様です。そばにいますね。", "id": "Master, istirahat ya. Aku akan tetap di sampingmu."}}"#,
        facts
    )
}

fn clean_indonesian_subtitle(text: &str, jp_text: &str) -> String {
    let japanese_regex = Regex::new(r"[\p{Hiragana}\p{Katakana}\p{Han}]").unwrap();
    let jp_chars = text.chars().filter(|c| japanese_regex.is_match(&c.to_string())).count();
    let total_chars = text.chars().count();
    
    if (total_chars > 0 && (jp_chars as f32 / total_chars as f32) > 0.3) || text.trim().is_empty() {
        if jp_text.contains("大丈夫") {
            "Master, apa kamu baik-baik saja?".to_string()
        } else if jp_text.contains("お疲れ") || jp_text.contains("休") {
            "Master, istirahat yang cukup ya...".to_string()
        } else {
            "Aku selalu ada di samping Master kok.".to_string()
        }
    } else {
        text.to_string()
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

async fn query_ollama(client: &reqwest::Client, messages: &[ChatMessage], gui: Arc<Mutex<SharedGuiState>>) -> Option<(String, String)> {
    {
        let mut g = gui.lock().unwrap();
        g.is_thinking = true;
        g.status_text = "THINKING...".to_string();
    }

    let req_body = OllamaRequest {
        model: MODEL_NAME.to_string(),
        messages: messages.to_vec(),
        stream: false,
        format: "json".to_string(),
        keep_alive: "2h".to_string(),
        options: OllamaOptions {
            num_ctx: 8192,
            temperature: 0.7,
            top_p: 0.9,
        },
    };

    let res = client.post(OLLAMA_URL).json(&req_body).send().await.ok();

    {
        let mut g = gui.lock().unwrap();
        g.is_thinking = false;
        g.status_text = "ONLINE".to_string();
    }

    let res = res?;
    if !res.status().is_success() {
        return None;
    }

    let ollama_res: OllamaResponse = res.json().await.ok()?;
    let content = ollama_res.message.content.trim();
    
    if let Ok(h) = serde_json::from_str::<HimariResponse>(content) {
        let jp = if h.jp.trim().is_empty() {
            "マスター、お疲れ様です。".to_string()
        } else {
            h.jp
        };
        let id_clean = clean_indonesian_subtitle(&h.id, &jp);
        Some((jp, id_clean))
    } else {
        Some(("マスター、どうしましたか？".to_string(), "Master, ada apa?".to_string()))
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

        tokio::task::spawn_blocking(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                if tx.blocking_send(line).is_err() {
                    break;
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

#[tokio::main]
async fn main() {
    println!("{}", "====================================================================".magenta());
    println!("{}", "🦀 HIMARI ENGINE v5.0: CYBERPUNK HUD (JS / WEB UI EDITION) 🦀".bold().magenta());
    println!("{}", "   [Edge App Window • Smooth Canvas 60FPS • UTF-8 Kanji Support]".cyan());
    println!("{}", "====================================================================".magenta());

    let gui_state = Arc::new(Mutex::new(SharedGuiState::default()));
    let (tx_action, rx_action) = tokio::sync::mpsc::channel::<char>(16);

    let app_context = Arc::new(AppContext {
        gui_state: Arc::clone(&gui_state),
        tx_action: tx_action.clone(),
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
        .fallback_service(ServeDir::new("web"))
        .layer(CorsLayer::permissive())
        .with_state(app_state_clone);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8765").await.unwrap();
    println!("{}", "🌐 HUD Web Server aktif di: http://127.0.0.1:8765".green());

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

    // 3. Listener Keyboard Console / Terminal (M: Mute, S: Switch, ESC / Q: Exit)
    let tx_key_cli = tx_action.clone();
    tokio::task::spawn_blocking(move || {
        loop {
            if let Ok(true) = event::poll(Duration::from_millis(100)) {
                if let Ok(Event::Key(key_event)) = event::read() {
                    if key_event.kind == KeyEventKind::Press {
                        match key_event.code {
                            KeyCode::Char('m') | KeyCode::Char('M') => {
                                let _ = tx_key_cli.blocking_send('m');
                            }
                            KeyCode::Char('s') | KeyCode::Char('S') => {
                                let _ = tx_key_cli.blocking_send('s');
                            }
                            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                                let _ = tx_key_cli.blocking_send('q');
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    });

    // 4. Jalankan AI Pipeline Runtime
    run_himari_backend(gui_state, rx_action).await;
}

async fn run_himari_backend(gui: Arc<Mutex<SharedGuiState>>, mut rx_action: tokio::sync::mpsc::Receiver<char>) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .build()
        .unwrap();

    let memory = load_memory();
    let sys_prompt = build_system_prompt(&memory);

    let mut chat_history: Vec<ChatMessage> = vec![ChatMessage {
        role: "system".to_string(),
        content: sys_prompt,
    }];

    // Sapaan awal
    let init_prompt = vec![
        chat_history[0].clone(),
        ChatMessage {
            role: "user".to_string(),
            content: "[SISTEM: Sapa Master Yudha saat sistem aktif. Panggil Master. Ucapkan kalimat Jepang yang manis dan santai.]".to_string(),
        },
    ];

    if let Some((jp, id)) = query_ollama(&client, &init_prompt, Arc::clone(&gui)).await {
        {
            let mut g = gui.lock().unwrap();
            g.sub_jp = jp.clone();
            g.sub_id = id.clone();
            g.status_text = "ONLINE".to_string();
        }
        play_voicevox(&client, &jp, Arc::clone(&gui)).await;
        chat_history.push(ChatMessage {
            role: "assistant".to_string(),
            content: format!(r#"{{"jp": "{}", "id": "{}"}}"#, jp, id),
        });
    }

    tokio::time::sleep(Duration::from_millis(1000)).await;

    let (tx_speech, mut rx_speech) = tokio::sync::mpsc::channel::<String>(32);
    let mut mic_controller = MicController::new();
    mic_controller.start(tx_speech.clone());

    {
        let mut g = gui.lock().unwrap();
        g.mic_device = mic_controller.device_name().to_string();
        g.status_text = "LISTENING".to_string();
    }

    let mut last_interaction = Instant::now();
    let mut app_state = AppState::Active;
    let mut is_hardware_muted = false;

    let mut interval = tokio::time::interval(Duration::from_millis(500));

    loop {
        tokio::select! {
            Some(action) = rx_action.recv() => {
                match action {
                    'q' => {
                        println!("\n{}", "👋 Master menutup aplikasi. Membersihkan proses...".cyan());
                        mic_controller.stop();
                        std::process::exit(0);
                    }
                    'm' => {
                        is_hardware_muted = !is_hardware_muted;

                        if is_hardware_muted {
                            mic_controller.stop();
                            while rx_speech.try_recv().is_ok() {}

                            {
                                let mut g = gui.lock().unwrap();
                                g.is_muted = true;
                                g.status_text = "MIC MUTED".to_string();
                                g.sub_id = "Mic dimatikan via Hotkey 'M'. Tekan 'M' lagi untuk unmute.".to_string();
                            }

                            let mute_prompt = vec![
                                chat_history[0].clone(),
                                ChatMessage {
                                    role: "user".to_string(),
                                    content: "[SISTEM: Master menekan tombol mute hardware. Ucapkan kalimat Jepang 1 kalimat lembut bahwa kamu akan standby sampai tombol unmute ditekan.]".to_string(),
                                },
                            ];

                            if let Some((jp, id)) = query_ollama(&client, &mute_prompt, Arc::clone(&gui)).await {
                                {
                                    let mut g = gui.lock().unwrap();
                                    g.sub_jp = jp.clone();
                                    g.sub_id = id.clone();
                                }
                                play_voicevox(&client, &jp, Arc::clone(&gui)).await;
                                tokio::time::sleep(Duration::from_millis(1000)).await;
                            }

                            app_state = AppState::Standby;
                        } else {
                            {
                                let mut g = gui.lock().unwrap();
                                g.is_muted = false;
                                g.status_text = "ONLINE".to_string();
                            }

                            let unmute_prompt = vec![
                                chat_history[0].clone(),
                                ChatMessage {
                                    role: "user".to_string(),
                                    content: "[SISTEM: Master un-mute mic. Ucapkan kalimat Jepang ceria: selamat datang kembali Master, aku siap mendengarkan.]".to_string(),
                                },
                            ];

                            if let Some((jp, id)) = query_ollama(&client, &unmute_prompt, Arc::clone(&gui)).await {
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
                            app_state = AppState::Active;

                            {
                                let mut g = gui.lock().unwrap();
                                g.status_text = "LISTENING".to_string();
                            }
                        }
                    }
                    's' => {
                        let new_name = mic_controller.toggle_device();

                        {
                            let mut g = gui.lock().unwrap();
                            g.mic_device = new_name.to_string();
                            g.status_text = "SWITCHING MIC".to_string();
                        }

                        let switch_prompt = vec![
                            chat_history[0].clone(),
                            ChatMessage {
                                role: "user".to_string(),
                                content: "[SISTEM: Mikrofon input baru saja diganti. Ucapkan kalimat Jepang 1 kalimat singkat:マイクを切り替えました、マスター。]".to_string(),
                            },
                        ];

                        if let Some((jp, id)) = query_ollama(&client, &switch_prompt, Arc::clone(&gui)).await {
                            {
                                let mut g = gui.lock().unwrap();
                                g.sub_jp = jp.clone();
                                g.sub_id = id.clone();
                            }
                            play_voicevox(&client, &jp, Arc::clone(&gui)).await;
                            tokio::time::sleep(Duration::from_millis(1000)).await;
                        }

                        if !is_hardware_muted {
                            mic_controller.start(tx_speech.clone());
                            last_interaction = Instant::now();
                        }

                        {
                            let mut g = gui.lock().unwrap();
                            g.status_text = "LISTENING".to_string();
                        }
                    }
                    _ => {}
                }
            }

            Some(user_text) = rx_speech.recv(), if !is_hardware_muted => {
                if app_state == AppState::VoiceSleep {
                    if is_wake_up_command(&user_text) {
                        mic_controller.stop();
                        while rx_speech.try_recv().is_ok() {}

                        {
                            let mut g = gui.lock().unwrap();
                            g.is_sleeping = false;
                            g.status_text = "WAKE UP DETECTED".to_string();
                        }

                        let wake_prompt = vec![
                            chat_history[0].clone(),
                            ChatMessage {
                                role: "user".to_string(),
                                content: "[SISTEM: Master memanggil namamu untuk bangun. Ucapkan kalimat Jepang 1 kalimat lembut dan manis: はい、マスター！いつでもお話しできますよ。]".to_string(),
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
                    g.sub_id = format!("Master: \"{}\"", user_text);
                    g.status_text = "PROCESSING".to_string();
                }

                if is_voice_switch_mic_command(&user_text) {
                    let new_name = mic_controller.toggle_device();
                    {
                        let mut g = gui.lock().unwrap();
                        g.mic_device = new_name.to_string();
                    }
                    
                    if let Some((jp, id)) = query_ollama(&client, &[
                        chat_history[0].clone(),
                        ChatMessage { role: "user".to_string(), content: "Ucapkan konfirmasi 1 kalimat: マイクを切り替えました、マスター。".to_string() }
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

                if is_voice_sleep_command(&user_text) {
                    app_state = AppState::VoiceSleep;

                    {
                        let mut g = gui.lock().unwrap();
                        g.is_sleeping = true;
                        g.status_text = "STANDBY SLEEP".to_string();
                    }

                    let sleep_prompt = vec![
                        chat_history[0].clone(),
                        ChatMessage {
                            role: "user".to_string(),
                            content: "[SISTEM: Master memintamu mute/diam dulu untuk fokus. Ucapkan kalimat Jepang 1 kalimat sangat lembut: わかりました、静かに待っていますね。呼んでください。]".to_string(),
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

                chat_history.push(ChatMessage {
                    role: "user".to_string(),
                    content: user_text.clone(),
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

            _ = interval.tick(), if !is_hardware_muted && app_state != AppState::VoiceSleep => {
                let elapsed = last_interaction.elapsed().as_secs();

                match app_state {
                    AppState::Active => {
                        if elapsed >= 30 {
                            app_state = AppState::FirstWarning;

                            mic_controller.stop();
                            while rx_speech.try_recv().is_ok() {}

                            let idle_prompts = [
                                "Master terdiam cukup lama. Tanyakan dengan lembut apakah beliau sedang melamun atau pusing.",
                                "Suasana hening. Tanyakan Master sedang memikirkan apa.",
                                "Master diam saja. Ingatkan Master untuk minum air hangat secara manis.",
                            ];
                            let chosen = idle_prompts.choose(&mut rand::thread_rng()).unwrap();
                            let prompt = format!("[SISTEM: {} Panggil Master. Maksimal 1 kalimat.]", chosen);

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

                            let standby_prompt = "[SISTEM: Master tidak bersuara selama 1 menit penuh. Tinggalkan 1 pesan terakhir yang manis bahwa kamu akan menunggu dalam diam/standby di sini sampai Master memanggil lagi.]";

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
