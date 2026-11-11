mod ai;
mod audio;
mod memory;
mod skills;
mod types;
mod voice;
mod win32;

use ai::*;
use audio::*;
use memory::*;
use skills::*;
use types::*;
use voice::*;
use win32::*;

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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

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

