use crate::win32::{keybd_event, PostMessageW, HWND};
use std::process::Command;

pub async fn fetch_weather_info() -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .ok()?;
    let url = "https://wttr.in/Bandung?format=%C+%t";
    let res = client.get(url).send().await.ok()?;
    if res.status().is_success() {
        let text = res.text().await.ok()?;
        return Some(text.trim().to_string());
    }
    None
}

pub async fn fetch_wikipedia_search(query: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;
    let url = format!(
        "https://id.wikipedia.org/api/rest_v1/page/summary/{}",
        urlencoding::encode(query)
    );
    let res = client.get(&url).header("User-Agent", "HimariEngine/1.0").send().await.ok()?;
    if res.status().is_success() {
        let json = res.json::<serde_json::Value>().await.ok()?;
        if let Some(extract) = json["extract"].as_str() {
            return Some(extract.to_string());
        }
    }
    None
}

pub fn execute_system_skill(command: &str) -> Option<&'static str> {
    let lower = command.to_lowercase();

    // A. Media Controls via Win32 keybd_event
    if lower.contains("pause") || lower.contains("jeda lagu") || lower.contains("play lagu") || lower.contains("lanjut lagu") || lower.contains("stop musik") {
        unsafe {
            keybd_event(0xB3, 0, 0, 0); // VK_MEDIA_PLAY_PAUSE
            keybd_event(0xB3, 0, 2, 0);
        }
        return Some("Media play/pause berhasil dijalankan!");
    }
    if lower.contains("next lagu") || lower.contains("lagu berikutnya") || lower.contains("skip lagu") {
        unsafe {
            keybd_event(0xB0, 0, 0, 0); // VK_MEDIA_NEXT_TRACK
            keybd_event(0xB0, 0, 2, 0);
        }
        return Some("Memutar lagu berikutnya!");
    }
    if lower.contains("prev lagu") || lower.contains("lagu sebelumnya") {
        unsafe {
            keybd_event(0xB1, 0, 0, 0); // VK_MEDIA_PREV_TRACK
            keybd_event(0xB1, 0, 2, 0);
        }
        return Some("Memutar lagu sebelumnya!");
    }
    if lower.contains("volume up") || lower.contains("besarin volume") || lower.contains("tambah volume") {
        unsafe {
            for _ in 0..5 {
                keybd_event(0xAF, 0, 0, 0); // VK_VOLUME_UP
                keybd_event(0xAF, 0, 2, 0);
            }
        }
        return Some("Volume dinaikkan!");
    }
    if lower.contains("volume down") || lower.contains("kecilin volume") || lower.contains("kurangi volume") {
        unsafe {
            for _ in 0..5 {
                keybd_event(0xAE, 0, 0, 0); // VK_VOLUME_DOWN
                keybd_event(0xAE, 0, 2, 0);
            }
        }
        return Some("Volume diturunkan!");
    }
    if lower.contains("mute suara") || lower.contains("unmute") || lower.contains("bisu") {
        unsafe {
            keybd_event(0xAD, 0, 0, 0); // VK_VOLUME_MUTE
            keybd_event(0xAD, 0, 2, 0);
        }
        return Some("Status mute diubah!");
    }

    // B. App Launchers & Quick Access
    if lower.contains("buka browser") || lower.contains("buka chrome") {
        let _ = Command::new("cmd").args(&["/c", "start chrome"]).spawn();
        return Some("Membuka Google Chrome!");
    }
    if lower.contains("buka zed") || lower.contains("buka editor") {
        let _ = Command::new("cmd").args(&["/c", "start zed"]).spawn();
        return Some("Membuka Zed Editor!");
    }
    if lower.contains("buka task manager") {
        let _ = Command::new("cmd").args(&["/c", "start taskmgr"]).spawn();
        return Some("Membuka Task Manager!");
    }
    if lower.contains("buka notepad") {
        let _ = Command::new("cmd").args(&["/c", "start notepad"]).spawn();
        return Some("Membuka Notepad!");
    }

    // C. Screen Display Standby (Anti-Freeze Win32 Safe PostMessage)
    if lower.contains("sleep monitor") || lower.contains("matikan layar") || lower.contains("standby layar") || lower.contains("matikan monitor") {
        unsafe {
            // SC_MONITORPOWER = 0xF170, 2 = Monitor Off / Standby
            PostMessageW(0xFFFF as HWND, 0x0112, 0xF170, 2);
        }
        return Some("Layar monitor telah dialihkan ke mode standby!");
    }

    None
}
