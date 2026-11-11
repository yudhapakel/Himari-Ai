use crate::types::SharedGuiState;
use crate::win32::get_local_hour_minute;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

pub const VOICEVOX_URL: &str = "http://127.0.0.1:50021";
pub const SPEAKER_ID: u32 = 14;

pub async fn play_random_thinking_filler(client: &reqwest::Client, gui: Arc<Mutex<SharedGuiState>>) {
    let fillers = [
        "ええと…",
        "そうですね…",
        "うーん…",
        "ちょっと待ってね…",
    ];
    let chosen = fillers[rand::random::<usize>() % fillers.len()];
    play_voicevox(client, chosen, gui).await;
}

pub async fn play_voicevox(client: &reqwest::Client, text_jp: &str, gui: Arc<Mutex<SharedGuiState>>) {
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
