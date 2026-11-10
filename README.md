# himari-engine

High-performance, local anime voice & desktop companion runtime built in Rust, powered by local LLMs (Qwen 2.5 via Ollama), in-memory Japanese speech synthesis (VOICEVOX), and an interactive Cyberpunk HUD web interface.

---

## Overview

`himari-engine` orchestrates speech recognition, text input, local language model inference, and low-latency audio synthesis into a unified native binary. Designed for zero monthly API cost, minimal resource footprint, zero disk wear, and responsive real-time companion interaction on Windows 11.

### Key Capabilities

- **Zero-Disk Streaming Audio**: Audio synthesis streams directly through memory buffers (`rodio`), eliminating temporary `.wav` read/write cycles on SSD storage.
- **Hardware-Level Mic Isolation**: Process-level microphone termination during assistant speech output to eliminate acoustic echo feedback and self-hearing loops.
- **Dynamic Input Switching**: Runtime hotkey switching between desktop wired headset (IEM) and wireless mobile microphones (WO Mic / virtual audio).
- **Hybrid Input System (Voice & Keyboard Text)**: Full support for text chat directly from the Web HUD (`/` shortcut), functioning seamlessly even when the microphone is hardware-muted.
- **Always-on-Top Frameless HUD**: Native Win32 API (`HWND_TOPMOST`) window loop keeping the companion HUD visible above all workspaces, games, and browser tabs without stealing keyboard focus.
- **Natural Voice Thinking Fillers**: Instant non-repeating anime Japanese voice cues (*"Chotto matte ne"*, *"Etto ne..."*, *"Ryoukai da yo"*, etc.) fired concurrently while the LLM or tool pipeline evaluates commands.
- **Native Windows OS Skills & Media Controller**:
  - Precise Win32 volume calibration (`VK_VOLUME_DOWN` & `VK_VOLUME_UP`).
  - Global media play/pause (`VK_MEDIA_PLAY_PAUSE`).
  - Automated YouTube search scraper and universal music/video playback launcher.
- **Autonomous Self-Learning & Memory Compaction**:
  - Background Tokio daemon extracting durable personal preferences into `himari_memory.json` (0 MB VRAM overhead).
  - Context compaction sliding window keeping conversations bounded within 8K tokens.
- **Interactive Cyberpunk HUD**: Embedded lightweight Axum web server hosting a 60 FPS HTML5 Canvas HUD displaying live audio status, state machine transitions, dual Japanese/Indonesian subtitles, and quick chat history drawer.

---

## Architecture

```text
[ Microphone Input ] ---> [ Speech-to-Text ]
                                  |
                                  v
[ Web HUD Text Bar ] ---> [ himari-engine ] <---> [ himari_memory.json ]
                           (Rust / Tokio)
                           /                                      v              v
             [ Ollama: Qwen 2.5 ]   [ VOICEVOX Engine ]
               (Port 11434)           (Port 50021)
                          \              /
                           v            v
                   [ Rodio In-Memory Playback ]
                                |
                                v
                    [ HTML5 Canvas HUD (Axum) ]
```

---

## Tech Stack

- **Runtime**: Rust 2021 Edition (`tokio`, `axum`, `reqwest`, `rodio`, `serde`, `regex`)
- **Language Model**: Qwen 2.5 14B / 7B Instruct hosted via local Ollama (`num_ctx: 8192`)
- **Speech Synthesis**: VOICEVOX Engine (Speaker: Meimei Himari / 冥鳴ひまり, ID: 14)
- **Speech Recognition**: Google Speech Recognition API via local Python audio bridge
- **User Interface**: HTML5 Canvas, Vanilla JavaScript, CSS Glassmorphism, Microsoft Edge Application Mode (`--app`)
- **OS Integration**: Windows 32 FFI (`user32.dll` for `keybd_event`, `SetWindowPos`, `GetLastInputInfo`)

---

## Prerequisites

1. **Rust Toolchain**: `rustc` and `cargo` installed.
2. **Ollama**: Installed and serving locally on `http://127.0.0.1:11434`.
   ```bash
   ollama pull qwen2.5:14b
   ```
3. **VOICEVOX**: Installed and active on `http://127.0.0.1:50021`.
4. **Python Dependencies**:
   ```bash
   pip install SpeechRecognition pyaudio
   ```

---

## Installation & Usage

1. Clone repository:
   ```bash
   git clone https://github.com/yudhapakel/himari-engine.git
   cd himari-engine
   ```

2. Build release binary:
   ```bash
   cargo build --release
   ```

3. Run executable:
   ```bash
   ./target/release/himari-engine
   ```
   *(Or launch via the Windows Start Menu shortcut `Himari AI`)*.

---

## Controls & Shortcuts

| Trigger | Input | Description |
| :--- | :--- | :--- |
| `M` Key / UI Click | Keyboard / Mouse | Toggle hardware microphone mute |
| `S` Key / UI Click | Keyboard / Mouse | Switch active microphone device (IEM / Mobile) |
| `/` or `Enter` Key | Keyboard | Focus into the Cyberpunk Text Chat input bar |
| Voice Command | "Selamat tidur" / "Sleep mode" | Trigger silent standby sleep mode |
| Voice Command | "Himari" / "Bangun" | Wake assistant from sleep mode |
| Voice / Text Command | "Puter lagu [judul lagu]" | Search YouTube and autoplay video/music |
| Voice / Text Command | "Atur volume [persen]%" | Calibrate system volume to exact percentage |
| `ESC` Key | Keyboard | Terminate application and runtime processes |

---

## License & Attribution

- Source code licensed under the **MIT License**.
- Voice model provided by **VOICEVOX: 冥鳴ひまり** (Meimei Himari).
- LLM powered by **Qwen 2.5** (Alibaba Cloud, Apache 2.0 License).
