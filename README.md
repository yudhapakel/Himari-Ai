# himari-engine

High-performance, local anime voice assistant runtime built in Rust, powered by local LLMs (Qwen 2.5 via Ollama), in-memory Japanese speech synthesis (VOICEVOX), and an interactive Cyberpunk HUD web interface.

---

## Overview

`himari-engine` orchestrates speech recognition, local language model inference, and low-latency audio synthesis into a unified native binary. Designed for minimal resource footprint, zero disk wear, and responsive real-time conversation.

### Key Capabilities

- **Zero-Disk Streaming Audio**: Audio synthesis streams directly through memory buffers (`rodio`), avoiding temporary `.wav` read/write cycles on SSD storage.
- **Hardware-Level Mic Isolation**: Process-level microphone termination during assistant speech output to eliminate acoustic echo feedback and self-hearing loops.
- **Dynamic Input Switching**: Runtime hotkey switching between desktop wired headset (IEM) and wireless mobile microphones (WO Mic / virtual audio).
- **Multi-State Standby Machine**: Autonomous conversational initiative after idle thresholds, direct speech intent standby, and wake-word voice recovery.
- **Interactive Cyberpunk HUD**: Embedded lightweight Axum web server hosting a 60 FPS HTML5 Canvas HUD displaying live audio status, state machine transitions, and dual Japanese/Indonesian subtitles.

---

## Architecture

```text
[ Microphone Input ] ---> [ Speech-to-Text ]
                                  |
                                  v
                        [ himari-engine ] <---> [ himari_memory.json ]
                         (Rust / Tokio)
                         /            \
                        v              v
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

- **Runtime**: Rust 2021 Edition (`tokio`, `axum`, `reqwest`, `rodio`, `serde`)
- **Language Model**: Qwen 2.5 14B / 7B Instruct hosted via local Ollama
- **Speech Synthesis**: VOICEVOX Engine (Speaker: Meimei Himari / 冥鳴ひまり, ID: 14)
- **Speech Recognition**: Google Speech Recognition API via local Python audio bridge
- **User Interface**: HTML5 Canvas, Vanilla JavaScript, CSS Glassmorphism, Microsoft Edge Application Mode

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

---

## Controls

| Trigger | Input | Description |
| :--- | :--- | :--- |
| `M` Key / UI Click | Keyboard / Mouse | Toggle hardware microphone mute |
| `S` Key / UI Click | Keyboard / Mouse | Switch active microphone device (IEM / Mobile) |
| Voice Command | "Selamat tidur" / "Sleep mode" | Trigger silent standby sleep mode |
| Voice Command | "Himari" / "Bangun" | Wake assistant from sleep mode |
| `ESC` Key | Keyboard | Terminate application and runtime processes |

---

## License & Attribution

- Source code licensed under the **MIT License**.
- Voice model provided by **VOICEVOX: 冥鳴ひまり** (Meimei Himari).
- LLM powered by **Qwen 2.5** (Alibaba Cloud, Apache 2.0 License).
