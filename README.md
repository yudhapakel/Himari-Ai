# himari-engine

[![Rust](https://img.shields.io/badge/Rust-2021%20Edition-DEA584.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue.svg?style=flat-square)](LICENSE)
[![Tokio](https://img.shields.io/badge/Async-Tokio-000000.svg?style=flat-square)](https://tokio.rs/)
[![VOICEVOX](https://img.shields.io/badge/TTS-VOICEVOX-88DD88.svg?style=flat-square)](https://voicevox.hiroshiba.jp/)
[![Ollama](https://img.shields.io/badge/LLM-Qwen%202.5-412991.svg?style=flat-square)](https://ollama.com/)

High-performance, local voice companion and proactive desktop runtime built in Rust. Powered by local LLM inference (Qwen 2.5 via Ollama), in-memory low-latency speech synthesis (VOICEVOX), Win32 system sensory loop, and an interactive Cyberpunk HTML5 Canvas HUD.

---

## Overview

`himari-engine` unifies voice recognition, real-time speech synthesis, local language model cognition, Win32 hardware telemetry, and an always-on-top desktop overlay into a lightweight, memory-safe native binary. 

Engineered with a zero-cloud dependency model, it operates entirely offline without external API subscription costs, minimizing host memory consumption, zeroing SSD storage wear, and delivering sub-second acoustic responsiveness.

---

## Key Capabilities

- **Zero-Disk In-Memory Audio Streaming**: Direct audio buffer playback via `rodio` avoiding temporary `.wav` file writes on disk, maximizing SSD lifespan and reducing IO latency.
- **Hardware-Level Microphone Echo Isolation**: Automatic process-level microphone muting during speech synthesis output to eliminate acoustic echo feedback and audio feedback loops.
- **Dynamic Device Switching**: Hotkey-driven runtime audio device toggling between desktop studio microphones, wired headsets, and virtual audio inputs.
- **Hybrid Input Interface**: Seamless concurrent support for voice commands and keyboard text inputs via the Web HUD (`/` shortcut), maintaining full functionality in noise-restricted environments.
- **Always-on-Top Frameless Web HUD**: Hardware-accelerated Win32 overlay (`HWND_TOPMOST`) running an embedded Axum web server hosting a 60 FPS HTML5 Canvas HUD with glassmorphism design.
- **Proactive Context Awareness (Presence & App Sensing)**:
  - Background process detection inspecting active foreground applications (code editors, browsers, media players).
  - Idle state and sleep schedule guardian monitoring user presence and providing contextual notifications.
- **Native OS Automation & Media Controller**:
  - Direct Win32 key event injection for volume stepping and global media playback.
  - Automated YouTube video/music query resolution and browser playback launcher.
- **Local Memory & Sliding Context Compaction**:
  - Long-term profile persistence via JSON-based memory storage without VRAM overhead.
  - Sliding conversation window management maintaining token bounds within an 8K context limit.
- **Natural Voice Latency Concealment**: Pre-cached conversational voice fillers triggered instantly during asynchronous model inference to provide natural conversational cadence.

---

## Architecture

```text
[ Audio Input / Mic ] ---> [ Speech-to-Text Bridge ]
                                   |
                                   v
[ Web HUD Text Bar ]  ---> [  himari-engine  ] <---> [ Local Memory Store ]
                            (Rust / Tokio)
                            /                                       v              v
              [ Ollama: Qwen 2.5 ]    [ VOICEVOX Engine ]
                (Port 11434)            (Port 50021)
                           \              /
                            v            v
                    [ Rodio In-Memory Stream ]
                                 |
                                 v
                     [ HTML5 Canvas HUD (Axum) ]
```

---

## Tech Stack

| Domain | Technology | Details |
| :--- | :--- | :--- |
| **Core Runtime** | Rust 2021 (`tokio`, `axum`) | Multi-threaded async engine with low CPU/RAM overhead |
| **Audio Engine** | `rodio`, `symphonia` | In-memory audio decoding and output streaming |
| **Language Model** | Qwen 2.5 (14B / 7B Instruct) | Self-hosted locally on Ollama (`num_ctx: 8192`) |
| **Speech Synthesis** | VOICEVOX Engine | Neural Japanese TTS (Character: Meimei Himari, Speaker ID: 14) |
| **Speech Recognition** | SpeechRecognition Python Bridge | Low-latency audio transcription stream |
| **Desktop HUD** | HTML5 Canvas, Vanilla JS, CSS | Frameless browser application mode (`--app`) |
| **OS Integration** | Win32 API (`user32.dll`) | Process polling, window positioning, and media key injection |

---

## Prerequisites

- **Rust Toolchain**: 1.75+ (`cargo`, `rustc`)
- **Ollama**: Installed and listening on `http://127.0.0.1:11434`
  ```bash
  ollama pull qwen2.5:14b
  ```
- **VOICEVOX**: Installed and active on `http://127.0.0.1:50021`
- **Python Audio Bridge Dependencies**:
  ```bash
  pip install SpeechRecognition pyaudio
  ```

---

## Installation & Running

1. **Clone the repository:**
   ```bash
   git clone https://github.com/yudhapakel/Himari-Ai.git
   cd Himari-Ai
   ```

2. **Build the release executable:**
   ```bash
   cargo build --release
   ```

3. **Start the engine:**
   ```bash
   ./target/release/himari-engine
   ```

---

## Controls & Keybindings

| Key / Action | Trigger Mode | Function |
| :--- | :--- | :--- |
| `M` | Keyboard / UI Button | Toggle hardware microphone mute |
| `S` | Keyboard / UI Button | Cycle between primary and secondary audio devices |
| `/` or `Enter` | Keyboard | Focus into HUD text prompt input |
| `Esc` | Keyboard | Close chat overlay or exit application |
| `"Sleep mode"` | Voice / Text | Put assistant into low-power silent standby |
| `"Wake up"` | Voice / Text | Resume active listening from standby mode |
| `"Play [title]"` | Voice / Text | Automatically search and play media stream |
| `"Volume [0-100]%"` | Voice / Text | Adjust master system audio level to specified percentage |

---

## Project Structure

```text
himari-engine/
├── src/
│   ├── main.go / main.rs  # Core application entrypoint and runtime loops
│   ├── types.rs           # State models, configs, and message envelopes
│   ├── win32.rs           # Windows API bindings, process sensing, and media keys
│   ├── memory.rs          # Context management and persistent profile memory
│   ├── audio.rs           # Device selection, mic monitoring, and rodio playback
│   ├── ai.rs              # Ollama API client and prompt orchestration
│   ├── skills.rs          # Tool dispatching (media control, browser automation)
│   └── voice.rs           # VOICEVOX speech synthesis client and filler cache
├── web/
│   └── index.html         # HTML5 Canvas HUD with glassmorphism styling
├── Cargo.toml             # Rust package manifest
└── README.md              # Project documentation
```

---

## Roadmap

- [x] In-memory streaming audio engine (`rodio`) with zero disk wear
- [x] Process-level active window sensing (code editors, browsers)
- [x] Always-on-top glassmorphic HUD with interactive status visualizer
- [x] Win32 volume and media keyboard injection
- [ ] Direct WebSocket telemetry stream for HUD updates
- [ ] Offline local STT integration (Whisper.cpp / ONNX) to remove Python bridge
- [ ] Vision capabilities (desktop screenshot analysis via multi-modal models)

---

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

### Third-Party Acknowledgements
- Voice model by **VOICEVOX: 冥鳴ひまり** (Meimei Himari).
- LLM architecture powered by **Qwen 2.5** (Alibaba Cloud, Apache 2.0 License).
