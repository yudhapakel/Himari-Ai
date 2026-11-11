use crate::types::{ChatMessage, HimariResponse, OllamaOptions, OllamaRequest, OllamaResponse};

pub async fn query_ollama(
    client: &reqwest::Client,
    messages: &[ChatMessage],
    model: &str,
) -> Result<HimariResponse, Box<dyn std::error::Error + Send + Sync>> {
    let req = OllamaRequest {
        model: model.to_string(),
        messages: messages.to_vec(),
        stream: false,
        format: "json".to_string(),
        options: OllamaOptions {
            temperature: 0.7,
            num_predict: 256,
            repeat_penalty: 1.1,
        },
    };

    let res = client
        .post("http://127.0.0.1:11434/api/chat")
        .json(&req)
        .send()
        .await?;

    if !res.status().is_success() {
        return Err(format!("Ollama status error: {}", res.status()).into());
    }

    let data: OllamaResponse = res.json().await?;
    let content = data.message.content.trim();

    // Parse JSON
    if let Ok(parsed) = serde_json::from_str::<HimariResponse>(content) {
        return Ok(parsed);
    }

    // Fallback regex jika model membungkus dengan markdown
    let re = regex::Regex::new(r"\{.*?\}").unwrap();
    if let Some(mat) = re.find(content) {
        if let Ok(parsed) = serde_json::from_str::<HimariResponse>(mat.as_str()) {
            return Ok(parsed);
        }
    }

    Ok(HimariResponse {
        jp: "わかりました、あなた。".to_string(),
        id: content.to_string(),
        kaomoji: "(´｡• ᵕ •｡`)".to_string(),
    })
}

pub fn spawn_self_learning(user_text: String) {
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let prompt = format!(
            "Analisis kalimat pengguna berikut: '{}'.\n\
            Apakah terdapat fakta penting, preferensi, kebiasaan, atau instruksi pribadi baru tentang pengguna?\n\
            Jika YA, ekstrak fakta tersebut menjadi 1 kalimat padat dan jelas dalam bahasa Indonesia.\n\
            Jika TIDAK ada fakta baru, jawab persis: NONE.",
            user_text
        );
        let req = serde_json::json!({
            "model": "qwen2.5:14b",
            "messages": [{"role": "user", "content": prompt}],
            "stream": false
        });
        if let Ok(res) = client.post("http://127.0.0.1:11434/api/chat").json(&req).send().await {
            if let Ok(data) = res.json::<serde_json::Value>().await {
                if let Some(reply) = data["message"]["content"].as_str() {
                    let cleaned = reply.trim();
                    if !cleaned.is_empty() && !cleaned.starts_with("NONE") {
                        let mut mem = crate::memory::load_memory();
                        if !mem.facts.iter().any(|f| f.contains(cleaned)) {
                            mem.facts.push(cleaned.to_string());
                            crate::memory::save_memory(&mem);
                            println!("[SELF-LEARNING] Memory updated with: {}", cleaned);
                        }
                    }
                }
            }
        }
    });
}

pub fn spawn_context_compaction(turns_to_compact: Vec<ChatMessage>) {
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let mut conv_text = String::new();
        for msg in &turns_to_compact {
            conv_text.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }

        let prompt = format!(
            "Rangkum intisari penting dari percakapan berikut dalam 2-3 poin ringkas:\n{}\n\
            Simpan konteks penting agar tidak lupa.",
            conv_text
        );

        let req = serde_json::json!({
            "model": "qwen2.5:14b",
            "messages": [{"role": "user", "content": prompt}],
            "stream": false
        });

        if let Ok(res) = client.post("http://127.0.0.1:11434/api/chat").json(&req).send().await {
            if let Ok(data) = res.json::<serde_json::Value>().await {
                if let Some(reply) = data["message"]["content"].as_str() {
                    let mut mem = crate::memory::load_memory();
                    mem.notes.push(reply.trim().to_string());
                    if mem.notes.len() > 10 {
                        mem.notes.remove(0);
                    }
                    crate::memory::save_memory(&mem);
                }
            }
        }
    });
}
