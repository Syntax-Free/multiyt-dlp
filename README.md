<div align="center">

<img src="/.github/multiyt-dlp_logo.png" alt="Multiyt-dlp Logo" width="200"/>

# Multiyt-dlp RELOADED
### High-Velocity Concurrent Media Downloader

<a href="https://github.com/yt-dlp/yt-dlp">
    <img src="https://img.shields.io/badge/Powered%20By-yt--dlp-red?style=for-the-badge&logo=youtube" alt="Powered by yt-dlp" />
</a>
<img src="https://img.shields.io/badge/Backend-Rust-orange?style=for-the-badge&logo=rust" alt="Rust" />
<img src="https://img.shields.io/badge/Frontend-React-blue?style=for-the-badge&logo=react" alt="React" />
<img src="https://img.shields.io/badge/Style-Tailwind-38bdf8?style=for-the-badge&logo=tailwindcss" alt="Tailwind" />
<br/>
<img src="https://img.shields.io/badge/License-MIT-green?style=flat-square" alt="License" />
<img src="https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey?style=flat-square" alt="Cross Platform" />

<p align="center">
  <b>Multiyt-dlp</b> is a modern, cyberpunk-themed GUI for the legendary <a href="https://github.com/yt-dlp/yt-dlp">yt-dlp</a> command-line tool. 
  <br />
  Built with <b>Tauri</b>, it focuses on speed, massive concurrency, and a frictionless user experience.
</p>

</div>

---

## ⚡ Features

### 🚀 High-Performance Engine
*   **Parallel Downloads:** Download multiple videos or audio tracks simultaneously.
*   **Queue Management:** Smart queuing system manages bandwidth and CPU usage (configurable concurrency limits).
*   **Rust Backend:** Leveraging Tokio for asynchronous, non-blocking process management.

### 🎛️ Granular Control
*   **Format Presets:** One-click selection for Best Video (MP4/MKV/WebM) or Audiophile Audio (FLAC/MP3/M4A).
*   **Resolution Targeting:** Target specific resolutions from 240p up to 8K.
*   **Metadata & Art:** Automatically embed thumbnails (cover art) and ID3 tags/metadata into files.
*   **Playlist Expansion:** Paste a playlist URL to auto-expand and queue all videos instantly.

### 🎨 Visual Experience
*   **Neon Aesthetic:** A dark, high-contrast UI designed for long sessions.
*   **Live Progress:** Real-time progress bars, speed meters, and ETA calculations.
*   **State Awareness:** Visual indicators for Merging, Extracting, Fixing, and Metadata writing phases.

### 🛠️ Advanced Tooling
*   **Drag & Drop Template Editor:** Customize your filename output (e.g., `%(title)s - %(uploader)s.%(ext)s`) using a visual block editor.
*   **Dependency Check:** Built-in splash screen checks for `yt-dlp` and `ffmpeg` availability on launch.
*   **System Integration:** Native file dialogs and shell integration.

---

## 📦 Installation

### 📥 Pre-compiled Binaries (Windows Only)
A standalone executable (`.exe`) is available for Windows users.
👉 **[Download from Releases](../../releases)**

> **Note:** Compiling for macOS or Linux is not currently supported by the developer. Users on these platforms must **build from source** (see below).

### 🔧 Prerequisites
Regardless of whether you use the installer or build from source, **Multiyt-dlp requires these external tools** to function:

1.  **yt-dlp**: [Download Here](https://github.com/yt-dlp/yt-dlp/releases) (Must be in your system PATH)
2.  **FFmpeg**: [Download Here](https://ffmpeg.org/download.html) (Must be in your system PATH)
3.  *(Optional)* **Node/Deno/Bun**: Recommended for downloading from YouTube.

### 🏗️ Building from Source

1.  **Clone the repository**
    ```bash
    git clone https://github.com/zqily/multiyt-dlp.git
    cd multiyt-dlp
    ```

2.  **Install Dependencies**
    ```bash
    npm install
    ```

3.  **Run Development Mode**
    ```bash
    npm run tauri dev
    ```

4.  **Build Release**
    ```bash
    npm run tauri build
    ```

---

## 🖥️ Usage

1.  **Input URL:** Paste a link from YouTube (or any site supported by yt-dlp).
2.  **Select Mode:** Toggle between **VIDEO** or **AUDIO** mode.
3.  **Configure:** Choose your quality preset and toggle Metadata/Thumbnail options.
4.  **Download:** Hit the button. The job is added to the grid.
5.  **Manage:** Switch between **List View** (details) and **Grid View** (monitoring) using the layout toggle.

---

## ⚙️ Configuration

Settings are persisted automatically.

*   **Location:** `~/.multiyt-dlp/config.json`
*   **Logs:** `~/.multiyt-dlp/logs/` (Rotated daily)

You can configure **Concurrency limits** (how many downloads run at once) and **Total Instance limits** (downloads + post-processing) directly within the Settings UI.

---

## 🏗️ Tech Stack

*   **Core:** [Tauri](https://tauri.app/) (Rust)
*   **Frontend:** React, TypeScript, Vite
*   **Styling:** Tailwind CSS, clsx, tailwind-merge
*   **Icons:** Lucide React
*   **State Management:** React Context + Tauri Event System
*   **Process Handling:** Tokio (Rust Async Runtime)

---

<div align="center">
  <sub>Built with ❤️ by Syntax-Free & 🤖</sub>
</div>