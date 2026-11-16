# himari-engine

[![Rust](https://img.shields.io/badge/Rust-2021%20Edition-DEA584.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue.svg?style=flat-square)](LICENSE)
[![Tokio](https://img.shields.io/badge/Async-Tokio-000000.svg?style=flat-square)](https://tokio.rs/)
[![VOICEVOX](https://img.shields.io/badge/TTS-VOICEVOX-88DD88.svg?style=flat-square)](https://voicevox.hiroshiba.jp/)
[![Ollama](https://img.shields.io/badge/LLM-Qwen%202.5-412991.svg?style=flat-square)](https://ollama.com/)

Offline local voice assistant engine and desktop HUD runtime built in Rust. Powered by local LLM inference (Qwen 2.5 via Ollama), in-memory speech synthesis (VOICEVOX), Win32 system monitoring, and an embedded Axum Canvas HUD.

---

## Overview

`himari-engine` unifies local speech recognition, neural text-to-speech synthesis, self-hosted LLM inference, and desktop telemetry into a single native binary. It operates completely offline with zero external cloud dependencies, running local speech synthesis and inference pipelines entirely on local hardware.

---

## Key Capabilities

- **In-Memory Audio Streaming:** Streams raw audio buffers directly via `rodio` and `symphonia` without writing temporary `.wav` files to disk, eliminating unnecessary disk I/O.
- **Microphone Echo Isolation:** Automatically toggles process-level microphone muting while speech output is active to prevent acoustic feedback loops.
- **Audio Device Management:** Hotkey-driven runtime switching across input and output audio endpoints.
- **Hybrid Input:** Concurrent support for voice prompts and keyboard text input via the HUD shortcut (`/`).
- **Frameless Web HUD:** Desktop overlay running on Win32 (`HWND_TOPMOST`) backed by an embedded Axum web server hosting an HTML5 Canvas interface.
- **Desktop Process & Presence Polling:** Tracks foreground applications (editors, browsers, media players) and user idle states for contextual notifications.
- **OS & Media Controls:** Win32 virtual key event injection for system volume control and global media playback.
- **Sliding Context Window:** Local JSON memory persistence with rolling context bounds capped at an 8K token budget.

---

## Architecture

```text
[ Audio Input / Mic ] ---> [ Speech-to-Text Bridge ]
                                   |
                                   v
[ Web HUD Text Bar ]  ---> [  himari-engine  ] <---> [ Local Memory Store ]
                            (Rust / Tokio)
                            /              \
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
| **Core Runtime** | Rust 2021 (`tokio`, `axum`) | Multi-threaded async engine with low memory footprint |
| **Audio Engine** | `rodio`, `symphonia` | In-memory audio decoding and output streaming |
| **Language Model** | Qwen 2.5 (14B / 7B Instruct) | Self-hosted locally on Ollama (`num_ctx: 8192`) |
| **Speech Synthesis** | VOICEVOX Engine | Neural Japanese TTS (Character: Meimei Himari, Speaker ID: 14) |
| **Speech Recognition** | SpeechRecognition Python Bridge | Local audio transcription stream |
| **Desktop HUD** | HTML5 Canvas, Vanilla JS, CSS | Frameless browser application window (`--app`) |
| **OS Integration** | Win32 API (`user32.dll`) | Foreground window tracking, position anchoring, media keys |

---

## Prerequisites

- **Rust Toolchain**: 1.75+ (`cargo`, `rustc`)
- **Ollama**: Running locally on `http://127.0.0.1:11434`
  ```bash
  ollama pull qwen2.5:14b
  ```
- **VOICEVOX**: Running locally on `http://127.0.0.1:50021`
- **Python Audio Bridge Dependencies**:
  ```bash
  pip install SpeechRecognition pyaudio
  ```

---

## Installation & Running

1. Clone the repository:
   ```bash
   git clone https://github.com/yudhapakel/Himari-Ai.git
   cd Himari-Ai
   ```

2. Build release binary:
   ```bash
   cargo build --release
   ```

3. Run:
   ```bash
   ./target/release/himari-engine
   ```

---

## Controls & Keybindings

| Key / Action | Trigger Mode | Function |
| :--- | :--- | :--- |
| `M` | Keyboard / UI Button | Toggle hardware microphone mute |
| `S` | Keyboard / UI Button | Cycle primary and secondary audio endpoints |
| `/` or `Enter` | Keyboard | Focus HUD text input bar |
| `Esc` | Keyboard | Close chat overlay or exit application |
| `"Sleep mode"` | Voice / Text | Put assistant into low-power silent standby |
| `"Wake up"` | Voice / Text | Resume listening from standby |

---

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE) for details.
