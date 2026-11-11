use colored::*;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

pub struct MicController {
    child: Option<Child>,
    mic_index: Arc<AtomicUsize>,
}

impl MicController {
    pub fn new() -> Self {
        Self {
            child: None,
            mic_index: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn start(&mut self, tx: tokio::sync::mpsc::Sender<String>) {
        self.stop();

        let python_cmd = "python";
        let script_path = "mic_stream.py";

        let current_mic = self.mic_index.load(Ordering::SeqCst);
        let mut child = match Command::new(python_cmd)
            .args(&[script_path, &current_mic.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{}", format!("Gagal menjalankan mic_stream.py: {}", e).red());
                return;
            }
        };

        if let Some(stdout) = child.stdout.take() {
            std::thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines().flatten() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        let _ = tx.blocking_send(trimmed.to_string());
                    }
                }
            });
        }

        self.child = Some(child);
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn switch_microphone(&mut self, new_index: usize, tx: tokio::sync::mpsc::Sender<String>) {
        self.mic_index.store(new_index, Ordering::SeqCst);
        self.start(tx);
    }
}

pub fn is_voice_sleep_command(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("tidur")
        || lower.contains("istirahat dulu")
        || lower.contains("diam dulu")
        || lower.contains("sleep")
        || lower.contains("oyasumi")
        || lower.contains("berhenti mendengarkan")
}

pub fn is_wake_up_command(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("himari bangun")
        || lower.contains("bangun himari")
        || lower.contains("bangun")
        || lower.contains("halo himari")
        || lower.contains("ohayou")
        || lower.contains("wake up")
}

pub fn is_voice_switch_mic_command(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("ganti mic") || lower.contains("switch mic") || lower.contains("tukar mic")
}

pub fn is_weather_inquiry(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("cuaca") || lower.contains("hujan") || lower.contains("panas") || lower.contains("suhu")
}

pub fn extract_search_keyword(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    if lower.contains("siapa itu") || lower.contains("apa itu") || lower.contains("cari tahu tentang") {
        let cleaned = lower
            .replace("siapa itu", "")
            .replace("apa itu", "")
            .replace("cari tahu tentang", "")
            .replace("himari", "")
            .trim()
            .to_string();
        if !cleaned.is_empty() {
            return Some(cleaned);
        }
    }
    None
}

pub fn parse_timer_seconds(text: &str) -> Option<(u64, String)> {
    let lower = text.to_lowercase();
    let re = regex::Regex::new(r"(\d+)\s*(menit|detik|jam)").unwrap();
    if let Some(cap) = re.captures(&lower) {
        let num: u64 = cap.get(1)?.as_str().parse().ok()?;
        let unit = cap.get(2)?.as_str();
        let seconds = match unit {
            "detik" => num,
            "menit" => num * 60,
            "jam" => num * 3600,
            _ => return None,
        };
        let label = format!("{} {}", num, unit);
        return Some((seconds, label));
    }
    None
}
