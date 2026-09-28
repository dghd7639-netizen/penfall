use crate::storage;

#[tauri::command]
pub async fn get_history(
    state: tauri::State<'_, storage::HistoryStore>,
    config_state: tauri::State<'_, storage::ConfigManager>,
    limit: u32,
    offset: u32,
) -> Result<Vec<storage::HistoryEntry>, String> {
    let config = config_state.load().await.map_err(|e| e.to_string())?;
    if config.history_enabled {
        state
            .prune_with_policy(
                &config.history_retention_policy(),
                &chrono::Utc::now()
                    .naive_utc()
                    .format("%Y-%m-%dT%H:%M:%S")
                    .to_string(),
            )
            .await
            .map_err(|e| e.to_string())?;
    }
    state.list(limit, offset).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_history(state: tauri::State<'_, storage::HistoryStore>) -> Result<(), String> {
    state.clear().await.map_err(|e| e.to_string())
}
