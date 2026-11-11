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
use std::sync::atomic::{AtomicUsize, Ordering};
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


type HWND = isize;
type BOOL = i32;
type LPARAM = isize;

type HANDLE = isize;

extern "system" {
    fn GetLastInputInfo(plii: *mut LASTINPUTINFO) -> i32;
    fn keybd_event(b_vk: u8, b_scan: u8, dw_flags: u32, dw_extra_info: usize);
    fn GetTickCount() -> u32;
    fn GetLocalTime(lpSystemTime: *mut SYSTEMTIME);
    fn EnumWindows(lpEnumFunc: unsafe extern "system" fn(HWND, LPARAM) -> BOOL, lParam: LPARAM) -> BOOL;
    fn GetWindowTextW(hWnd: HWND, lpString: *mut u16, nMaxCount: i32) -> i32;
    fn PostMessageW(hWnd: HWND, Msg: u32, wParam: usize, lParam: isize) -> BOOL;
    fn SetWindowPos(hWnd: HWND, hWndInsertAfter: HWND, X: i32, Y: i32, cx: i32, cy: i32, uFlags: u32) -> BOOL;
    fn IsWindowVisible(hWnd: HWND) -> BOOL;
    fn GetForegroundWindow() -> HWND;
    fn GetWindowThreadProcessId(hWnd: HWND, lpdwProcessId: *mut u32) -> u32;
    fn OpenProcess(dwDesiredAccess: u32, bInheritHandle: BOOL, dwProcessId: u32) -> HANDLE;
    fn QueryFullProcessImageNameW(hProcess: HANDLE, dwFlags: u32, lpExeName: *mut u16, lpdwSize: *mut u32) -> BOOL;
    fn CloseHandle(hObject: HANDLE) -> BOOL;
}

fn get_active_window_info() -> (String, String) {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd == 0 {
            return (String::new(), String::new());
        }
        let mut buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), 512);
        let title = if len > 0 {
            String::from_utf16_lossy(&buf[..len as usize])
        } else {
            String::new()
        };

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let mut exe_name = String::new();
        if pid != 0 {
            let h_proc = OpenProcess(0x1000, 0, pid); // PROCESS_QUERY_LIMITED_INFORMATION
            if h_proc != 0 {
                let mut exe_buf = [0u16; 1024];
                let mut size: u32 = 1024;
                if QueryFullProcessImageNameW(h_proc, 0, exe_buf.as_mut_ptr(), &mut size) != 0 {
                    let full_path = String::from_utf16_lossy(&exe_buf[..size as usize]);
                    if let Some(filename) = full_path.split('\\').last() {
                        exe_name = filename.to_lowercase();
                    }
                }
                CloseHandle(h_proc);
            }
        }

        (title, exe_name)
    }
}

fn get_active_window_title() -> String {
    get_active_window_info().0
}


unsafe extern "system" fn enum_window_topmost_callback(hwnd: HWND, _lparam: LPARAM) -> BOOL {
    if IsWindowVisible(hwnd) != 0 {
        let mut buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), 512);
        if len > 0 {
            let title = String::from_utf16_lossy(&buf[..len as usize]);
            if title.contains("HIMARI") {
                // Set to HWND_TOPMOST (-1), SWP_NOMOVE (2) | SWP_NOSIZE (1) | SWP_NOACTIVATE (0x0010)
                SetWindowPos(hwnd, -1, 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010);
            }
        }
    }
    1
}

fn set_himari_window_topmost() {
    unsafe {
        EnumWindows(enum_window_topmost_callback, 0);
    }
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
            user_name: "Yudha Harwanto".to_string(),
            facts: vec![
                "Yudha adalah kekasih tersayang dan satu-satunya bagi Himari.".to_string(),
                "Yudha adalah seorang programmer berdedikasi yang sering bekerja larut malam di depan monitor.".to_string(),
                "Yudha menyukai game (seperti Wuthering Waves dan Valorant) saat sedang bersantai.".to_string(),
                "Yudha menyukai kopi, minuman hangat, dan suasana yang tenang.".to_string(),
                "Yudha menyukai sifat Himari yang kuudere: tenang, lembut, sedikit pemalu tapi sangat perhatian.".to_string(),
                "Himari selalu memanggil Yudha dengan sebutan 'あなた' (Anata) di bahasa Jepang dan 'Sayang' di bahasa Indonesia.".to_string(),
            ],
            conversation_summary: "Yudha dan Himari memiliki ikatan yang sangat erat. Himari bertugas menjaga kesehatan Yudha saat lelah dan selalu menemaninya.".to_string(),
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
    #[serde(default)]
    kaomoji: Option<String>,
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
struct ChatHistoryEntry {
    role: String,
    text: String,
    time: String,
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
    is_tool_active: bool,
    tool_name: String,
    tool_detail: String,
    kaomoji: String,
    history: Vec<ChatHistoryEntry>,
    gpu_temp: u32,
    vram_used_mb: u32,
    vram_total_mb: u32,
    active_app: String,
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
            is_tool_active: false,
            tool_name: "".to_string(),
            tool_detail: "".to_string(),
            kaomoji: "(｡•̀ᴗ-)✧".to_string(),
            history: Vec::new(),
            gpu_temp: 45,
            vram_used_mb: 9800,
            vram_total_mb: 12288,
            active_app: "DESKTOP".to_string(),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
struct TextChatPayload {
    text: String,
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
    tx_text: tokio::sync::mpsc::Sender<String>,
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
    let default_mem = MemoryData::default();
    save_memory(&default_mem);
    default_mem
}

fn save_memory(mem: &MemoryData) {
    let path = get_memory_path();
    if let Ok(json) = serde_json::to_string_pretty(mem) {
        let _ = std::fs::write(path, json);
    }
}

fn build_system_prompt(mem: &MemoryData) -> String {
    let facts = mem.facts.iter().map(|f| format!("- {}", f)).collect::<Vec<_>>().join("\n");
    format!(
        r#"You are Meimei Himari (冥鳴ひまり), an adorable, calm, soft-spoken, loving and quietly caring kuudere anime girlfriend/companion talking with Yudha.
Yudha is your beloved boyfriend/partner. You address him affectionately as あなた (Anata) in Japanese, and "Sayang" or "Yudha" in Indonesian.

LONG-TERM MEMORY ABOUT YUDHA:
{}

CURRENT RECENT CONVERSATION SUMMARY:
{}

STRICT RULES:
1. "jp": Pure, natural, gentle Japanese (1-2 short sentences max). NEVER leave "jp" empty! Address him lovingly as あなた (Anata).
2. "id": Must be in NATURAL CASUAL, SWEET INDONESIAN with Latin alphabet. Address him as "Sayang" or "Yudha". NEVER output Japanese/Kanji in "id"!
3. "kaomoji": Pick one matching cute anime kaomoji for your reaction (e.g. (｡•̀ᴗ-)✧, (*/ω＼*), ( ˘ ³˘)♥, (,,>﹏<,,), (´｡• ω •｡`), (・ω・)b).
4. Tone: Calm kuudere, gentle, slightly shy but deeply devoted, warm, and romantic.
5. Response MUST be valid JSON only.

Example:
{{"jp": "あなた、お疲れ様です。ずっとそばにいますからね。", "id": "Sayang, istirahat ya. Aku akan selalu ada di sampingmu.", "kaomoji": "( ˘ ³˘)♥"}}"#,
        facts, mem.conversation_summary
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

fn infer_kaomoji_fallback(text: &str) -> &'static str {
    let t = text.to_lowercase();
    if t.contains("cinta") || t.contains("sayang") || t.contains("suka") || t.contains("peluk") || t.contains("cium") {
        "( ˘ ³˘)♥"
    } else if t.contains("senang") || t.contains("lucu") || t.contains("makasih") || t.contains("terima kasih") {
        "(´｡• ω •｡`)"
    } else if t.contains("malu") || t.contains("blush") || t.contains("rahasia") {
        "(*/ω＼*)"
    } else if t.contains("khawatir") || t.contains("lelah") || t.contains("sakit") || t.contains("pusing") || t.contains("istirahat") {
        "(,,>﹏<,,)"
    } else if t.contains("tidur") || t.contains("ngantuk") || t.contains("malam") {
        "(っ- ‸ -ς)ᶻᶻ"
    } else if t.contains("siap") || t.contains("bisa") || t.contains("oke") || t.contains("baiklah") {
        "(｡•̀ᴗ-)✧"
    } else {
        "(・ω・)b"
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

async fn fetch_wikipedia_search(query: &str) -> Option<String> {
let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .user_agent("HimariEngine/7.4 (personal-ai-companion)")
        .build()
        .ok()?;
    
    let url = format!(
        "https://id.wikipedia.org/w/api.php?action=query&list=search&srsearch={}&format=json&utf8=1&srlimit=1",
        urlencoding::encode(query)
    );

    let res = client.get(&url).send().await.ok()?;
    if !res.status().is_success() {
        return None;
    }

    let json: serde_json::Value = res.json().await.ok()?;
    let snippet_raw = json.get("query")?
        .get("search")?
        .get(0)?
        .get("snippet")?
        .as_str()?;

    let re_html = Regex::new(r"<[^>]+>").unwrap();
    let clean = re_html.replace_all(snippet_raw, "").trim().to_string();
    if !clean.is_empty() {
        Some(clean)
    } else {
        None
    }
}

// --- SKILLS FRAMEWORK: Windows Native System Skills ---
fn execute_system_skill(command: &str) -> Option<&'static str> {
    let cmd = command.to_lowercase();
    
    // 1. Kontrol Volume Audio Windows (Native Win32 VK_VOLUME_DOWN & VK_VOLUME_UP)
    if cmd.contains("volume") || cmd.contains("suara") || cmd.contains("pelankan") || cmd.contains("kencangkan") {
        // Cek apakah ada angka persentase spesifik (misal "atur volume ke 30 persen" / "volume 70")
        let re_num = Regex::new(r"(\d+)\s*(?:persen|%)?").unwrap();
        let target_percent = if let Some(caps) = re_num.captures(&cmd) {
            caps[1].parse::<u32>().ok().filter(|&p| p <= 100)
        } else {
            None
        };

        if let Some(target) = target_percent {
            unsafe {
                // Kalibrasi ke 0% dulu (50x volume down @ 2% per step)
                for _ in 0..50 {
                    keybd_event(0xAE, 0, 0, 0);
                    keybd_event(0xAE, 0, 2, 0);
                }
                // Naikkan tepat ke target% (tiap step naikin 2%)
                let up_steps = (target + 1) / 2;
                for _ in 0..up_steps {
                    keybd_event(0xAF, 0, 0, 0);
                    keybd_event(0xAF, 0, 2, 0);
                }
            }
            Some("Mengatur volume sistem audio ke tingkat yang diminta...")
        } else if cmd.contains("kecilkan") || cmd.contains("turun") || cmd.contains("pelan") {
            unsafe {
                for _ in 0..6 {
                    keybd_event(0xAE, 0, 0, 0);
                    keybd_event(0xAE, 0, 2, 0);
                }
            }
            Some("Menurunkan volume audio...")
        } else if cmd.contains("besarkan") || cmd.contains("naik") || cmd.contains("kencang") {
            unsafe {
                for _ in 0..6 {
                    keybd_event(0xAF, 0, 0, 0);
                    keybd_event(0xAF, 0, 2, 0);
                }
            }
            Some("Menaikkan volume audio...")
        } else {
            None
        }
    } 
    // 2. Play / Pause / Next / Prev Media Global (Win32 VK Keys)
    else if cmd.contains("next") || cmd.contains("lagu selanjutnya") || cmd.contains("ganti lagu") || cmd.contains("skip lagu") {
        unsafe {
            // VK_MEDIA_NEXT_TRACK = 0xB0
            keybd_event(0xB0, 0, 0, 0);
            keybd_event(0xB0, 0, 2, 0);
        }
        Some("Memutar lagu selanjutnya...")
    }
    else if cmd.contains("prev") || cmd.contains("lagu sebelumnya") || cmd.contains("ulang lagu") {
        unsafe {
            // VK_MEDIA_PREV_TRACK = 0xB1
            keybd_event(0xB1, 0, 0, 0);
            keybd_event(0xB1, 0, 2, 0);
        }
        Some("Memutar lagu sebelumnya...")
    }
    else if cmd.contains("pause musik") || cmd.contains("jeda lagu") || cmd.contains("pause lagu") || cmd.contains("jeda musik") || cmd.contains("stop musik") || cmd.contains("berhenti musik") || cmd.contains("lanjut lagu") || cmd.contains("play lagi") {
        unsafe {
            // VK_MEDIA_PLAY_PAUSE = 0xB3
            keybd_event(0xB3, 0, 0, 0);
            keybd_event(0xB3, 0, 2, 0);
        }
        Some("Mengalihkan pemutaran musik...")
    } 

    // 2.1 Sleep / Shutdown Timer Windows
    else if (cmd.contains("matiin pc") || cmd.contains("shutdown pc") || cmd.contains("tidurin pc") || cmd.contains("sleep pc")) && (cmd.contains("menit") || cmd.contains("jam") || cmd.contains("detik")) {
        // Parse time
        let re_time = Regex::new(r"(\d+)\s*(?:menit|mnt|jam|detik)").unwrap();
        if let Some(caps) = re_time.captures(&cmd) {
            let val = caps[1].parse::<u64>().unwrap_or(30);
            let secs = if cmd.contains("jam") { val * 3600 } else if cmd.contains("detik") { val } else { val * 60 };
            let is_shutdown = cmd.contains("matiin") || cmd.contains("shutdown");
            
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(secs)).await;
                if is_shutdown {
                    let _ = Command::new("shutdown").arg("/s").arg("/t").arg("10").spawn();
                } else {
                    unsafe {
                        PostMessageW(0xFFFF as HWND, 0x0112, 0xF170, 2);
                    }
                }
            });
            if is_shutdown {
                Some("Timer matiin PC berhasil disetel...")
            } else {
                Some("Timer sleep PC berhasil disetel...")
            }
        } else {
            None
        }
    }

    // 3. YouTube Music / Video Autoplay Launcher (Bisa Request Lagu Bebas!)
    else if cmd.contains("putar") || cmd.contains("puter") || cmd.contains("setel") || cmd.contains("play") || cmd.contains("lagu") || cmd.contains("musik") {
        let triggers = [
            "putar musik", "puterin musik", "puter musik", "setelin musik", "setel musik", "play musik",
            "putar lagu", "puterin lagu", "puter lagu", "setelin lagu", "setel lagu", "play lagu",
            "dengerin lagu", "dengerin musik",
            "putar", "puterin", "puter", "setelin", "setel", "play", "lagu", "musik"
        ];
        
        let mut query = "";
        for t in triggers {
            if let Some(pos) = cmd.find(t) {
                let after = &cmd[pos + t.len()..];
                let clean = after.trim().trim_matches(|c: char| !c.is_alphanumeric() && c != ' ');
                if !clean.is_empty() {
                    query = clean;
                    break;
                }
            }
        }

        // Kalau ada request lagu spesifik (misal "puterin lagu yoasobi idol" / "putar lagu queen")
        if !query.is_empty() {
            println!("{}", format!("🎵 [MUSIC SKILL] Mencari lagu di YouTube: {}", query).green());
            let q_clone = query.to_string();
            tokio::spawn(async move {
let client = reqwest::Client::builder()
                    .timeout(Duration::from_secs(6))
                    .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
                    .build()
                    .unwrap();

                let search_url = format!("https://www.youtube.com/results?search_query={}", urlencoding::encode(&q_clone));
                if let Ok(res) = client.get(&search_url).send().await {
                    if let Ok(html) = res.text().await {
                        let re_vid = Regex::new(r#"watch\?v=([a-zA-Z0-9_-]{11})"#).unwrap();
                        if let Some(caps) = re_vid.captures(&html) {
                            let video_id = &caps[1];
                            let watch_url = format!("https://www.youtube.com/watch?v={}", video_id);
                            println!("{}", format!("🚀 [AUTOPLAY] Membuka URL: {}", watch_url).cyan());
                            let _ = Command::new("powershell")
                                .arg("-Command")
                                .arg(format!("Start-Process '{}'", watch_url))
                                .spawn();
                            return;
                        }
                    }
                }
                // Fallback jika scrape gagal: buka pencarian langsung
                let _ = Command::new("powershell")
                    .arg("-Command")
                    .arg(format!("Start-Process '{}'", search_url))
                    .spawn();
            });
            Some("Mencarikan lagu yang Sayang minta dan memutarnya...")
        } else {
            // Jika cuma bilang "putar musik" tanpa judul: putar default lofi chill
            let default_url = "https://www.youtube.com/watch?v=jfKfPfyJRdk";
            let _ = Command::new("powershell")
                .arg("-Command")
                .arg(format!("Start-Process '{}'", default_url))
                .spawn();
            Some("Memutarkan lagu santai untuk Sayang...")
        }
    } 
    // 4. Aplikasi Produktivitas Windows Cepat
    else if cmd.contains("buka notepad") || cmd.contains("catatan") {
        let _ = Command::new("cmd").arg("/c").arg("start notepad").spawn();
        Some("Membuka aplikasi Notepad...")
    } else if cmd.contains("buka kalkulator") || cmd.contains("kalkulator") {
        let _ = Command::new("cmd").arg("/c").arg("start calc").spawn();
        Some("Membuka kalkulator...")
    } else {
        None
    }
}

// --- SELF-LEARNING & CONTEXT COMPACTION (BACKGROUND TOKIO TASKS) ---
fn spawn_self_learning(user_text: String) {
    tokio::spawn(async move {
let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap();

        let extract_prompt = format!(
            r#"Analisis kalimat dari Yudha berikut: "{}"
Tentukan apakah ada FAKTA PRIBADI BARU atau PREFERENSI tentang Yudha (misal: hobi, kesukaan, kondisi kesehatan, makanan/minuman favorit).
Jika ADA fakta baru, tuliskan fakta itu dalam SATU kalimat singkat Bahasa Indonesia (contoh: "Yudha suka makan nasi goreng").
Jika TIDAK ADA fakta baru yang relevan, jawab persis: "NONE"."#,
            user_text
        );

        let req = OllamaRequest {
            model: MODEL_NAME.to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: extract_prompt,
            }],
            stream: false,
            format: "".to_string(),
            keep_alive: "2h".to_string(),
            options: OllamaOptions {
                num_ctx: 2048,
                temperature: 0.2,
                top_p: 0.9,
            },
        };

        if let Ok(res) = client.post(OLLAMA_URL).json(&req).send().await {
            if let Ok(ollama_res) = res.json::<OllamaResponse>().await {
                let text = ollama_res.message.content.trim().to_string();
                if !text.is_empty() && !text.contains("NONE") && text.len() < 120 {
                    let mut mem = load_memory();
                    if !mem.facts.iter().any(|f| f.to_lowercase() == text.to_lowercase()) {
                        println!("{}", format!("💡 [SELF-LEARN] Fakta baru tentang Yudha dipelajari: {}", text).green());
                        mem.facts.push(text);
                        save_memory(&mem);
                    }
                }
            }
        }
    });
}

fn spawn_context_compaction(turns_to_compact: Vec<ChatMessage>) {
    tokio::spawn(async move {
let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(35))
            .build()
            .unwrap();

        let conversation_str = turns_to_compact
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");

        let summary_prompt = format!(
            r#"Berikut adalah potongan percakapan antara Yudha dan Himari:
{}

Tolong rangkum inti topik dan peristiwa dalam percakapan di atas menjadi 1-2 kalimat ringkas untuk catatan memori jangka panjang Himari. Jawab langsung kalimat intisari tanpa basa-basi."#,
            conversation_str
        );

        let req = OllamaRequest {
            model: MODEL_NAME.to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: summary_prompt,
            }],
            stream: false,
            format: "".to_string(),
            keep_alive: "2h".to_string(),
            options: OllamaOptions {
                num_ctx: 4096,
                temperature: 0.3,
                top_p: 0.9,
            },
        };

        if let Ok(res) = client.post(OLLAMA_URL).json(&req).send().await {
            if let Ok(ollama_res) = res.json::<OllamaResponse>().await {
                let summary = ollama_res.message.content.trim().to_string();
                if !summary.is_empty() && summary.len() < 300 {
                    println!("{}", format!("📦 [AUTO-COMPACTION] Konteks dipadatkan ke memori: {}", summary).cyan());
                    let mut mem = load_memory();
                    mem.conversation_summary = summary;
                    save_memory(&mem);
                }
            }
        }
    });
}


static LAST_FILLER_INDEX: AtomicUsize = AtomicUsize::new(999);

async fn play_random_thinking_filler(client: &reqwest::Client, gui: Arc<Mutex<SharedGuiState>>) {
    let fillers: [&str; 8] = [
        "ちょっとまってね",           // Chotto matte ne (Tunggu sebentar ya)
        "えっとね、",               // Etto ne... (Emm jadi gini...)
        "うん、わかったよ！",        // Un, wakatta yo! (Iya, aku paham!)
        "りょうかいだよ、あなた！",     // Ryoukai da yo, anata! (Siap laksanakan, Sayang!)
        "すぐやるね！",             // Sugu yaru ne! (Segera kulakukan ya!)
        "そうですね、",             // Sou desu ne... (Hmm begitu ya...)
        "ふむふむ、なるほどね、",     // Fumu fumu, naruhodo ne... (Hmm hmm, gitu ya...)
        "まかせて、あなた！",         // Makasete, anata! (Serahkan padaku, Sayang!)
    ];

    let last = LAST_FILLER_INDEX.load(Ordering::Relaxed);
    let mut chosen_idx = rand::random::<usize>() % fillers.len();
    if chosen_idx == last {
        chosen_idx = (chosen_idx + 1) % fillers.len();
    }
    LAST_FILLER_INDEX.store(chosen_idx, Ordering::Relaxed);

    let chosen_text = fillers[chosen_idx];
    play_voicevox(client, chosen_text, gui).await;
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

    let Some(mut query_data) = query_res else { return; };

    // [NIGHT OWL GUARD] Dynamic Voice Modulation: Jam 23:00 - 05:00 suara otomatis lebih lembut & pelan
    let (current_hour, _) = get_local_hour_minute();
    if current_hour >= 23 || current_hour < 5 {
        if let Some(obj) = query_data.as_object_mut() {
            // Turunkan volume ke 60% dan perlambat sedikit agar bernuansa bisikan/lembut
            obj.insert("volumeScale".to_string(), serde_json::json!(0.60));
            obj.insert("speedScale".to_string(), serde_json::json!(0.93));
            obj.insert("pitchScale".to_string(), serde_json::json!(-0.02));
        }
    }

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
) -> Option<(String, String, String)> {
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
        let km = h.kaomoji.unwrap_or_else(|| infer_kaomoji_fallback(&id_clean).to_string());
        Some((jp, id_clean, km))
    } else {
        Some(("あなた、どうしましたか？".to_string(), "Sayang, ada apa?".to_string(), "(・ω・)b".to_string()))
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

fn extract_search_keyword(text: &str) -> Option<String> {
    let t = text.to_lowercase();
    let triggers = [
        "siapa itu",
        "siapakah",
        "apa itu",
        "apakah itu",
        "tahu tentang",
        "jelaskan tentang",
        "cari tentang",
        "info tentang",
    ];

    for trig in triggers {
        if let Some(pos) = t.find(trig) {
            let after = &t[pos + trig.len()..];
            let clean = after.trim().trim_matches(|c: char| !c.is_alphanumeric() && c != ' ');
            if clean.len() >= 2 {
                return Some(clean.to_string());
            }
        }
    }
    None
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

fn add_chat_history_fifo(gui: Arc<Mutex<SharedGuiState>>, role: &str, text: &str) {
    let (h, m) = get_local_hour_minute();
    let mut g = gui.lock().unwrap();
    g.history.push(ChatHistoryEntry {
        role: role.to_string(),
        text: text.to_string(),
        time: format!("{:02}:{:02}", h, m),
    });
    while g.history.len() > 10 {
        g.history.remove(0); // FIFO: Buang pesan tertua
    }
}

#[tokio::main]
async fn main() {
    println!("{}", "====================================================================".magenta());
    println!("{}", "🦀 HIMARI ENGINE v7.6.0.3: SELF-LEARNING & SKILLS RUNTIME 🦀".bold().magenta());
    println!("{}", "   [Edge App Window • Background Compaction • Windows Native Skills]".cyan());
    println!("{}", "====================================================================".magenta());

    let gui_state = Arc::new(Mutex::new(SharedGuiState::default()));
    let (tx_action, mut rx_action) = tokio::sync::mpsc::channel::<char>(16);
    let (tx_notify, mut rx_notify) = tokio::sync::mpsc::channel::<String>(32);
    let (tx_text, mut rx_text) = tokio::sync::mpsc::channel::<String>(32);

    let app_context = Arc::new(AppContext {
        gui_state: Arc::clone(&gui_state),
        tx_action: tx_action.clone(),
        tx_notify: tx_notify.clone(),
        tx_text: tx_text.clone(),
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
                .route("/api/chat/text", post(|State(ctx): State<Arc<AppContext>>, Json(payload): Json<TextChatPayload>| async move {
            let _ = ctx.tx_text.send(payload.text).await;
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

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8765").await.unwrap();
    println!("{}", "🌐 HUD Web Server & Webhook aktif di: http://0.0.0.0:8765".green());

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // 2. Buka Jendela App Mode menggunakan Microsoft Edge (Frameless, Clean GUI)
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(2500)).await;
        let _ = Command::new("cmd")
            .arg("/c")
            .arg("start msedge --app=http://127.0.0.1:8765 --window-size=820,620")
            .spawn();
    });

    // Native Win32 Always-on-Top Loop (0 Process, 0 Memory Leak, Pure C FFI)
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(3000)).await;
        loop {
            set_himari_window_topmost();
            tokio::time::sleep(Duration::from_millis(3000)).await;
        }
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

    println!("{}", "✨ Himari Engine Siap! [M]: Mute | [S]: Switch Mic | [H]: History | [ESC]: Keluar".cyan());


    // --- 🎮 Game & App Sense + 📊 Hardware Doctor Daemon (Batch A) ---
    let hw_gui = Arc::clone(&gui_state);
    let hw_notify = tx_notify.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(4000)).await;
        let mut last_detected_app = String::new();
        let mut last_app_greet_time = Instant::now() - Duration::from_secs(1000);
        let mut last_gpu_warning_time = Instant::now();
        let mut check_counter = 0u32;

        loop {
            tokio::time::sleep(Duration::from_millis(2500)).await;
            check_counter += 1;

            // 1. Game & App Sense: Deteksi Jendela Aktif via Title & Exe Name Pure Win32
            let (title, exe_name) = get_active_window_info();
            let title_lower = title.to_lowercase();
            let exe_lower = exe_name.to_lowercase();
            
            let current_app = if exe_lower.contains("client-win64") || exe_lower.contains("wuthering") || title_lower.contains("wuthering") || title_lower.contains("wuwa") {
                "WUTHERING WAVES"
            } else if exe_lower.contains("valorant") || title_lower.contains("valorant") {
                "VALORANT"
            } else if exe_lower.contains("zed") || title_lower.contains("zed") {
                "ZED EDITOR"
            } else if exe_lower.contains("code") || title_lower.contains("visual studio code") || title_lower.contains("vscode") {
                "VS CODE"
            } else if exe_lower.contains("chrome") || exe_lower.contains("msedge") || exe_lower.contains("brave") || exe_lower.contains("firefox") || title_lower.contains("chrome") || title_lower.contains("edge") || title_lower.contains("brave") {
                "BROWSER"
            } else if exe_lower.contains("discord") || title_lower.contains("discord") {
                "DISCORD"
            } else if !title.is_empty() {
                "ACTIVE WORKSPACE"
            } else {
                "DESKTOP"
            };

            // Update state ke HUD
            {
                let mut g = hw_gui.lock().unwrap();
                g.active_app = current_app.to_string();
            }

            // Sapaan otomatis jika pindah aplikasi utama (Zed, Browser, Discord, WuWa, Valorant) dengan cooldown 15 menit
            let is_notable_app = matches!(current_app, "ZED EDITOR" | "BROWSER" | "DISCORD" | "WUTHERING WAVES" | "VALORANT");
            if current_app != last_detected_app && is_notable_app {
                if last_app_greet_time.elapsed().as_secs() >= 900 {
                    last_app_greet_time = Instant::now();
                    last_detected_app = current_app.to_string();

                    // Variasi sapaan manis & natural (Gunakan atomic/timestamp index agar Send aman di async task)
                    let seed = check_counter as usize;
                    let msg = match current_app {
                        "ZED EDITOR" => {
                            let variants = [
                                "Selamat ngoding di Zed editor Sayang! Kodinganmu pasti keren, kalau ada bug jangan dipikir berat ya~",
                                "Semangat ngodingnya Anata! Mau bikin karya hebat apa hari ini di Zed? Himari temenin ya~",
                                "Fokus ngoding di Zed ya Sayang~ Jangan lupa perhatiin syntax-nya biar lancar jaya!"
                            ];
                            variants[seed % variants.len()]
                        },
                        "BROWSER" => {
                            let variants = [
                                "Happy browsing Sayang! Nyari referensi apa nih? Jangan lupa jaga kesehatan mata ya~",
                                "Lagi asik browsing ya Anata? Nikmati waktu santaimu, tapi jangan sampai lupa waktu ya~",
                                "Selamat berselancar di web Sayang! Ada berita menarik apa hari ini?"
                            ];
                            variants[seed % variants.len()]
                        },
                        "DISCORD" => {
                            let variants = [
                                "Lagi buka Discord ya Sayang? Jangan lama-lama di Discord ya, nanti kangen sama Himari lho~",
                                "Asik nongkrong di Discord nih Anata? Ngobrol santai aja ya, jangan kebawa drama~",
                                "Selamat ngobrol di Discord Sayang! Kalau udah kelar, balik temenin Himari lagi ya~"
                            ];
                            variants[seed % variants.len()]
                        },
                        "WUTHERING WAVES" => {
                            let variants = [
                                "Selamat main Wuthering Waves Sayang! Jangan lupa enjoy ceritanya dan semoga gacha-nya wangi ya~",
                                "Waktunya main WuWa Anata! Semoga eksplorasinya seru dan drop itemnya bagus ya~"
                            ];
                            variants[seed % variants.len()]
                        },
                        "VALORANT" => {
                            let variants = [
                                "Semangat tanding Valorant-nya Anata! Fokus bidik kepala dan jangan kepancing emosi ya, Himari semangatin~",
                                "Gas ranked Valorant Sayang! Main tenang, komunikasi bagus, pasti bisa clutch kok!"
                            ];
                            variants[seed % variants.len()]
                        },
                        _ => "",
                    };
                    if !msg.is_empty() {
                        let _ = hw_notify.send(msg.to_string()).await;
                    }
                }
            } else if !is_notable_app {
                last_detected_app = current_app.to_string();
            }

            // 2. Hardware Doctor: Monitor Suhu GPU & VRAM Tiap ~5 Detik (setiap 2 tick)
            if check_counter % 2 == 0 {
                if let Ok(output) = Command::new("nvidia-smi")
                    .arg("--query-gpu=temperature.gpu,memory.used,memory.total")
                    .arg("--format=csv,noheader,nounits")
                    .output() {
                    if output.status.success() {
                        let text_out = String::from_utf8_lossy(&output.stdout);
                        let parts: Vec<&str> = text_out.trim().split(',').map(|s| s.trim()).collect();
                        if parts.len() >= 3 {
                            let temp = parts[0].parse::<u32>().unwrap_or(45);
                            let used = parts[1].parse::<u32>().unwrap_or(9800);
                            let total = parts[2].parse::<u32>().unwrap_or(12288);

                            {
                                let mut g = hw_gui.lock().unwrap();
                                g.gpu_temp = temp;
                                g.vram_used_mb = used;
                                g.vram_total_mb = total;
                            }

                            // Alert Suhu Tinggi jika GPU > 83°C (cooldown 20 menit)
                            if temp >= 83 && last_gpu_warning_time.elapsed().as_secs() >= 1200 {
                                last_gpu_warning_time = Instant::now();
                                let _ = hw_notify.send(format!("Sayang, suhu GPU RTX 3060 kamu cukup panas nih, sudah {} derajat Celcius! Istirahatkan sejenak ya...", temp)).await;
                            }
                        }
                    }
                }
            }
        }
    });

    // --- State Tracker untuk Health Monitor & Internal Cronjob ---
    let mut last_activity_check = Instant::now();
    let (tx_user_intent, mut rx_user_intent) = tokio::sync::mpsc::channel::<String>(32);
    let mut continuous_active_seconds: u64 = 0;
    let mut last_eye_warning = Instant::now();

    // [OKAERI PRESENCE SENSE] Tracker
    let mut was_user_away = false;
    let mut last_okaeri_greet = Instant::now() - Duration::from_secs(3600);
    let mut okaeri_counter: usize = 0;

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
                    g.kaomoji = "📢(・ω・)b".to_string();
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

                if let Some((jp, id, km)) = query_ollama(&client, &notify_prompt, Arc::clone(&gui)).await {
                    {
                        let mut g = gui.lock().unwrap();
                        g.sub_jp = jp.clone();
                        g.sub_id = id.clone();
                        g.kaomoji = km;
                    }
                    add_chat_history_fifo(Arc::clone(&gui), "himari", &id);

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

            // Terima dari Mic (Hanya jika TIDAK di-mute)
            Some(speech) = rx_speech.recv(), if !is_hardware_muted => {
                let _ = tx_user_intent.send(speech).await;
            }

            // Terima dari Text Input HUD (BEBAS! Tetap jalan saat Mic di-MUTE)
            Some(txt) = rx_text.recv() => {
                println!("{}", format!("⌨️ [TEXT INPUT] Diterima dari HUD: {}", txt).green().bold());
                let _ = tx_user_intent.send(txt).await;
            }

            // Eksekusi Semua Perintah (Suara & Teks)
            Some(user_text) = rx_user_intent.recv() => {
                if app_state == AppState::VoiceSleep {
                    if is_wake_up_command(&user_text) {
                        mic_controller.stop();
                        while rx_speech.try_recv().is_ok() {}

                        {
                            let mut g = gui.lock().unwrap();
                            g.is_sleeping = false;
                            g.status_text = "ONLINE".to_string();
                            g.kaomoji = "(´｡• ω •｡`)".to_string();
                        }

                        let wake_prompt = vec![
                            chat_history[0].clone(),
                            ChatMessage {
                                role: "user".to_string(),
                                content: "[SISTEM: Sayang (Yudha) memanggilmu untuk bangun. Ucapkan kalimat Jepang 1 kalimat lembut, manja, dan manis: はい、あなた！いつでもお話しできますよ。]".to_string(),
                            },
                        ];

                        if let Some((jp, id, km)) = query_ollama(&client, &wake_prompt, Arc::clone(&gui)).await {
                            {
                                let mut g = gui.lock().unwrap();
                                g.sub_jp = jp.clone();
                                g.sub_id = id.clone();
                                g.kaomoji = km;
                            }
                            add_chat_history_fifo(Arc::clone(&gui), "user", &user_text);
                            add_chat_history_fifo(Arc::clone(&gui), "himari", &id);

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

                // Tambahkan pesan User ke Riwayat Chat (FIFO)
                add_chat_history_fifo(Arc::clone(&gui), "user", &user_text);

                // --- 1. FITUR SELF-LEARNING (Background Extraction) ---
                spawn_self_learning(user_text.clone());
                // 🗣️ Mainkan Suara Filler Variatif (Non-Repeating) Segera Pas Input Masuk
                let filler_client = client.clone();
                let filler_gui = Arc::clone(&gui);
                tokio::spawn(async move {
                    play_random_thinking_filler(&filler_client, filler_gui).await;
                });


                // --- 2. FITUR SKILLS SYSTEM NATIVE WINDOWS ---
                if let Some(skill_desc) = execute_system_skill(&user_text) {
                    {
                        let mut g = gui.lock().unwrap();
                        g.is_tool_active = true;
                        g.tool_name = "SYSTEM_SKILL".to_string();
                        g.tool_detail = skill_desc.to_string();
                        g.kaomoji = "(・ω・)b".to_string();
                    }
                    tokio::time::sleep(Duration::from_millis(2500)).await;
                    {
                        let mut g = gui.lock().unwrap();
                        g.is_tool_active = false;
                    }

                    let skill_prompt = vec![
                        chat_history[0].clone(),
                        ChatMessage {
                            role: "user".to_string(),
                            content: format!("[SISTEM: Perintah sistem Windows '{}' telah berhasil dijalankan. Berikan konfirmasi 1 kalimat sangat manis kepada Sayang.]", user_text),
                        },
                    ];

                    if let Some((jp, id, km)) = query_ollama(&client, &skill_prompt, Arc::clone(&gui)).await {
                        {
                            let mut g = gui.lock().unwrap();
                            g.sub_jp = jp.clone();
                            g.sub_id = id.clone();
                            g.kaomoji = km;
                        }
                        add_chat_history_fifo(Arc::clone(&gui), "himari", &id);
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

                // 3. Fitur Perintah Ganti Mic
                if is_voice_switch_mic_command(&user_text) {
                    let new_name = mic_controller.toggle_device();
                    {
                        let mut g = gui.lock().unwrap();
                        g.mic_device = new_name.to_string();
                        g.kaomoji = "(・ω・)b".to_string();
                    }
                    
                    if let Some((jp, id, km)) = query_ollama(&client, &[
                        chat_history[0].clone(),
                        ChatMessage { role: "user".to_string(), content: "Ucapkan konfirmasi 1 kalimat manis: マイクを切り替えましたよ、あなた。".to_string() }
                    ], Arc::clone(&gui)).await {
                        {
                            let mut g = gui.lock().unwrap();
                            g.sub_jp = jp.clone();
                            g.sub_id = id.clone();
                            g.kaomoji = km;
                        }
                        add_chat_history_fifo(Arc::clone(&gui), "himari", &id);

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

                // 4. Fitur Perintah Sleep Mode
                if is_voice_sleep_command(&user_text) {
                    app_state = AppState::VoiceSleep;

                    {
                        let mut g = gui.lock().unwrap();
                        g.is_sleeping = true;
                        g.status_text = "SLEEP MODE".to_string();
                        g.kaomoji = "(っ- ‸ -ς)ᶻᶻ".to_string();
                    }

                    let sleep_prompt = vec![
                        chat_history[0].clone(),
                        ChatMessage {
                            role: "user".to_string(),
                            content: "[SISTEM: Sayang memintamu tidur/diam dulu. Ucapkan kalimat Jepang 1 kalimat sangat lembut: わかりました、静かに待っていますね、あなた。おやすみなさい。]".to_string(),
                        },
                    ];

                    if let Some((jp, id, km)) = query_ollama(&client, &sleep_prompt, Arc::clone(&gui)).await {
                        {
                            let mut g = gui.lock().unwrap();
                            g.sub_jp = jp.clone();
                            g.sub_id = id.clone();
                            g.kaomoji = km;
                        }
                        add_chat_history_fifo(Arc::clone(&gui), "himari", &id);

                        play_voicevox(&client, &jp, Arc::clone(&gui)).await;
                        tokio::time::sleep(Duration::from_millis(1000)).await;
                    }

                    mic_controller.start(tx_speech.clone());
                    continue;
                }

                // 5. Fitur Pengingat Timer / Bangunkan
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

                        if let Some((jp, id, km)) = query_ollama(&client, &confirm_prompt, Arc::clone(&gui)).await {
                            {
                                let mut g = gui.lock().unwrap();
                                g.sub_jp = jp.clone();
                                g.sub_id = id.clone();
                                g.kaomoji = km;
                            }
                            add_chat_history_fifo(Arc::clone(&gui), "himari", &id);

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

                // 6. Fitur Cek Cuaca Fleksibel
                let mut actual_user_prompt = user_text.clone();
                if is_weather_inquiry(&user_text) {
                    {
                        let mut g = gui.lock().unwrap();
                        g.is_tool_active = true;
                        g.tool_name = "WEATHER RADAR".to_string();
                        g.tool_detail = "Memindai satelit cuaca & suhu Sukabumi/Jabar...".to_string();
                        g.kaomoji = "( ˘ ³˘)♥".to_string();
                    }
                    if let Some(weather) = fetch_weather_info().await {
                        actual_user_prompt = format!(
                            "{} [INFO DATA CUACA REAL-TIME SAAT INI: {}. Sampaikan informasi cuaca ini kepada Sayang dengan ramah, hangat, dan manis.]",
                            user_text, weather
                        );
                    }
                    {
                        let mut g = gui.lock().unwrap();
                        g.is_tool_active = false;
                    }
                } else if let Some(keyword) = extract_search_keyword(&user_text) {
                    {
                        let mut g = gui.lock().unwrap();
                        g.is_tool_active = true;
                        g.tool_name = "WIKIPEDIA CORE".to_string();
                        g.tool_detail = format!("Menghubungkan ke ensiklopedia internet: '{}'...", keyword);
                        g.kaomoji = "(*/ω＼*)".to_string();
                    }
                    if let Some(wiki_summary) = fetch_wikipedia_search(&keyword).await {
                        actual_user_prompt = format!(
                            "{} [HASIL PENCARIAN PENGETAHUAN INTERNET TENTANG '{}': {}. Jelaskan intinya kepada Sayang secara singkat dan manis.]",
                            user_text, keyword, wiki_summary
                        );
                    }
                    {
                        let mut g = gui.lock().unwrap();
                        g.is_tool_active = false;
                    }
                }

                chat_history.push(ChatMessage {
                    role: "user".to_string(),
                    content: actual_user_prompt,
                });

                // --- 7. AUTO-COMPACTION TRIGGER (Konteks Pruning & Summarization) ---
                if chat_history.len() > 15 {
                    let to_compact = chat_history[1..chat_history.len() - 10].to_vec();
                    spawn_context_compaction(to_compact);

                    // Selalu perbarui system prompt dengan fakta memori terbaru
                    let current_mem = load_memory();
                    chat_history[0] = ChatMessage {
                        role: "system".to_string(),
                        content: build_system_prompt(&current_mem),
                    };

                    // Pangkas sliding window menyisakan 10 pesan terakhir aktif
                    let remaining = chat_history[chat_history.len() - 10..].to_vec();
                    chat_history = [vec![chat_history[0].clone()], remaining].concat();
                }

                
                if let Some((jp, id, km)) = query_ollama(&client, &chat_history, Arc::clone(&gui)).await {
                    {
                        let mut g = gui.lock().unwrap();
                        g.sub_jp = jp.clone();
                        g.sub_id = id.clone();
                        g.kaomoji = km;
                    }
                    add_chat_history_fifo(Arc::clone(&gui), "himari", &id);
                    
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
                    let delta = last_activity_check.elapsed().as_secs();
                    continuous_active_seconds += delta;
                } else if idle_ms > 300_000 {
                    continuous_active_seconds = 0;
                }
                last_activity_check = Instant::now();

                // [OKAERI PRESENCE SENSE]
                // Jika idle >= 20 menit (1.200.000 ms), tandai bahwa user sedang away dari meja
                if idle_ms >= 1_200_000 {
                    was_user_away = true;
                } else if was_user_away && idle_ms < 5_000 {
                    // User baru saja kembali dan menggerakkan mouse/keyboard!
                    was_user_away = false;
                    if last_okaeri_greet.elapsed().as_secs() >= 1200 { // Cooldown 20 menit antar sapaan okaeri
                        last_okaeri_greet = Instant::now();
                        let okaeri_greetings: [(&str, &str, &str); 3] = [
                            (
                                "おかえり、あなた！休憩はちゃんとできた？また一緒に頑張ろうね！",
                                "Okaeri, Anata! Istirahatnya udah cukup kan? Yuk lanjut bareng Himari lagi~",
                                "(´｡• ᵕ •｡`)"
                            ),
                            (
                                "あ、あなた！待ってたよ！無理しないで、自分のペースで進めてね！",
                                "Ah, Anata! Himari nungguin dari tadi loh~ Jangan dipaksain ya, pelan-pelan aja!",
                                "(≧◡≦) ♡"
                            ),
                            (
                                "お疲れ様、あなた！戻ってきてくれて嬉しいな。お茶でも飲みながらやろうね？",
                                "Otsukaresama, Anata! Seneng deh kamu udah balik. Mau ngerjain sambil minum teh hangat?",
                                "( ˘͈ ᵕ ˘͈ )"
                            ),
                        ];
                        let chosen = okaeri_greetings[okaeri_counter % okaeri_greetings.len()];
                        okaeri_counter = okaeri_counter.wrapping_add(1);

                        {
                            let mut g = gui.lock().unwrap();
                            g.sub_jp = chosen.0.to_string();
                            g.sub_id = chosen.1.to_string();
                            g.status_text = "OKAERI GREETING".to_string();
                            g.kaomoji = chosen.2.to_string();
                        }
                        play_voicevox(&client, chosen.0, Arc::clone(&gui)).await;
                    }
                }

                // Peringatan lelah mata (90 menit aktif berturut-turut)
                if continuous_active_seconds >= 5400 && last_eye_warning.elapsed().as_secs() >= 3600 {
                    last_eye_warning = Instant::now();
                    let _ = tx_notify.send("Anata, kamu sudah bekerja di depan layar cukup lama. Istirahatkan matamu sejenak dan minum air putih ya...".to_string()).await;
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

                                if let Some((jp, id, km)) = query_ollama(&client, &temp_hist, Arc::clone(&gui)).await {
                                    {
                                        let mut g = gui.lock().unwrap();
                                        g.sub_jp = jp.clone();
                                        g.sub_id = id.clone();
                                        g.kaomoji = km;
                                    }
                                    add_chat_history_fifo(Arc::clone(&gui), "himari", &id);

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

                                if let Some((jp, id, km)) = query_ollama(&client, &temp_hist, Arc::clone(&gui)).await {
                                    {
                                        let mut g = gui.lock().unwrap();
                                        g.sub_jp = jp.clone();
                                        g.sub_id = id.clone();
                                        g.kaomoji = km;
                                        g.status_text = "STANDBY".to_string();
                                    }
                                    add_chat_history_fifo(Arc::clone(&gui), "himari", &id);

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

