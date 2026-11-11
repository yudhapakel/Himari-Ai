use std::path::PathBuf;
use crate::types::MemoryData;

pub fn get_memory_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("himari-engine");
    let _ = std::fs::create_dir_all(&path);
    path.push("himari_memory.json");
    path
}

pub fn load_memory() -> MemoryData {
    let path = get_memory_path();
    if let Ok(data) = std::fs::read_to_string(&path) {
        if let Ok(mem) = serde_json::from_str::<MemoryData>(&data) {
            return mem;
        }
    }
    MemoryData {
        user_name: "Anata".to_string(),
        facts: Vec::new(),
        routines: Vec::new(),
        preferences: Vec::new(),
        emotional_bonds: Vec::new(),
        notes: Vec::new(),
    }
}

pub fn save_memory(mem: &MemoryData) {
    let path = get_memory_path();
    if let Ok(json) = serde_json::to_string_pretty(mem) {
        let _ = std::fs::write(path, json);
    }
}

pub fn build_system_prompt(mem: &MemoryData) -> String {
    let mut prompt = String::from(
        "Kamu adalah Himari (Meimei Himari), pendamping AI anime setia, hangat, imut, dan perhatian untuk Anata (kamu selalu memanggil pengguna dengan sebutan 'Anata').\n\
        Responmu WAJIB berupa JSON dengan format persis:\n\
        {\n  \"jp\": \"(teks bahasa Jepang imut santai untuk diucapkan)\",\n  \"id\": \"(terjemahan/subtitle bahasa Indonesia santai, imut, dan ekspresif untuk ditampilkan)\",\n  \"kaomoji\": \"(kaomoji ekspresif yang cocok, contoh: (´｡• ᵕ •｡`), (≧◡≦), (*¯︶¯*))\"\n}\n\
        Aturan:\n\
        1. 'jp' harus natural bahasa Jepang percakapan (gunakan kanji/hiragana/katakana standar) tanpa karakter aneh atau tag.\n\
        2. 'id' harus menggunakan bahasa Indonesia gaul/akrab yang imut, santai, memanggil pengguna dengan 'Anata', tidak kaku seperti robot.\n\
        3. Selalu pertahankan persona waifu pendamping yang peduli pada kesehatan, jadwal, dan kenyamanan Anata.\n\
        4. Jangan pernah menambahkan markdown block atau teks apapun di luar blok JSON!\n\n"
    );

    if !mem.facts.is_empty() {
        prompt.push_str("Fakta & Memori tentang Anata:\n");
        for fact in &mem.facts {
            prompt.push_str(&format!("- {}\n", fact));
        }
    }

    prompt
}

pub fn clean_indonesian_subtitle(text: &str, jp_text: &str) -> String {
    let cleaned = text.trim();
    if cleaned.is_empty() || cleaned == jp_text {
        "Himari ada di sini untuk Anata~ (´｡• ᵕ •｡`)".to_string()
    } else {
        cleaned.to_string()
    }
}

pub fn infer_kaomoji_fallback(text: &str) -> &'static str {
    let lower = text.to_lowercase();
    if lower.contains("senang") || lower.contains("happy") || lower.contains("yatta") {
        "(≧◡≦) ♡"
    } else if lower.contains("sedih") || lower.contains("capek") || lower.contains("lelah") {
        "(つω`｡)"
    } else if lower.contains("semangat") || lower.contains("ganbate") || lower.contains("kerja") {
        "(ง •̀_•́)ง"
    } else if lower.contains("tidur") || lower.contains("malam") || lower.contains("istirahat") {
        "( ˘͈ ᵕ ˘͈ )"
    } else {
        "(´｡• ᵕ •｡`)"
    }
}
