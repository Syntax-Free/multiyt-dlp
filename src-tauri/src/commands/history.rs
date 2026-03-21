use crate::core::history::HistoryManager;
use tauri::State;

#[tauri::command]
pub async fn get_download_history(history: State<'_, HistoryManager>) -> Result<String, String> {
    history.get_content().await
}

#[tauri::command]
pub async fn save_download_history(
    history: State<'_, HistoryManager>,
    content: String,
) -> Result<(), String> {
    history.save_content(content).await
}

#[tauri::command]
pub async fn clear_download_history(history: State<'_, HistoryManager>) -> Result<(), String> {
    history.clear().await
}
