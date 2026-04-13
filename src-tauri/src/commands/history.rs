use tauri::State;
use crate::core::history::HistoryManager;
use tracing::{debug, info};

#[tauri::command]
pub async fn get_download_history(
    history: State<'_, HistoryManager>
) -> Result<String, String> {
    debug!(target: "commands::history", "Frontend requested history contents");
    history.get_content().await
}

#[tauri::command]
pub async fn save_download_history(
    history: State<'_, HistoryManager>,
    content: String
) -> Result<(), String> {
    info!(target: "commands::history", "Frontend saving new history contents ({} bytes)", content.len());
    history.save_content(content).await
}

#[tauri::command]
pub async fn clear_download_history(
    history: State<'_, HistoryManager>
) -> Result<(), String> {
    info!(target: "commands::history", "Frontend triggered full history clear");
    history.clear().await
}
