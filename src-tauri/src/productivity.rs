use crate::state::AppState;
use futures_util::StreamExt;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;
use tauri::State;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct HistoryEntry {
    pub timestamp: String,
    pub sql: String,
    pub duration_ms: u64,
    pub status: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct Snippet {
    pub name: String,
    pub sql: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct SessionTab {
    pub id: String,
    pub name: String,
    pub sql: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct SessionState {
    pub active_tab_id: Option<String>,
    pub tabs: Vec<SessionTab>,
}

fn get_file_path(app: &tauri::AppHandle, filename: &str) -> Result<std::path::PathBuf, String> {
    let mut path = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    path.push(filename);
    Ok(path)
}

fn escape_csv_field(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::Null => "".to_string(),
        serde_json::Value::String(s) => {
            if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.clone()
            }
        }
        other => other.to_string(),
    }
}

#[tauri::command]
pub async fn log_query(
    app: tauri::AppHandle,
    connection_id: String,
    sql: String,
    duration_ms: u64,
    status: String,
) -> Result<(), String> {
    let path = get_file_path(&app, &format!("history_{}.jsonl", connection_id))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());

    let entry = HistoryEntry {
        timestamp,
        sql,
        duration_ms,
        status,
    };
    let line = serde_json::to_string(&entry).map_err(|e| e.to_string())? + "\n";

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;

    file.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_query_history(
    app: tauri::AppHandle,
    connection_id: String,
) -> Result<Vec<HistoryEntry>, String> {
    let path = get_file_path(&app, &format!("history_{}.jsonl", connection_id))?;
    if !path.exists() {
        return Ok(vec![]);
    }

    let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let reader = std::io::BufReader::new(file);

    use std::io::BufRead;
    let mut entries = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        if let Ok(entry) = serde_json::from_str::<HistoryEntry>(&line) {
            entries.push(entry);
        }
    }
    entries.reverse();
    Ok(entries)
}

#[tauri::command]
pub async fn save_snippet(app: tauri::AppHandle, name: String, sql: String) -> Result<(), String> {
    let path = get_file_path(&app, "snippets.json")?;
    let mut snippets: Vec<Snippet> = if path.exists() {
        let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
        serde_json::from_reader(file).unwrap_or_else(|_| vec![])
    } else {
        vec![]
    };

    if let Some(pos) = snippets.iter().position(|s| s.name == name) {
        snippets[pos].sql = sql;
    } else {
        snippets.push(Snippet { name, sql });
    }

    let file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(file, &snippets).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn delete_snippet(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let path = get_file_path(&app, "snippets.json")?;
    if !path.exists() {
        return Ok(());
    }
    let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let mut snippets: Vec<Snippet> = serde_json::from_reader(file).unwrap_or_else(|_| vec![]);

    snippets.retain(|s| s.name != name);

    let file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(file, &snippets).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn load_snippets(app: tauri::AppHandle) -> Result<Vec<Snippet>, String> {
    let path = get_file_path(&app, "snippets.json")?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let snippets: Vec<Snippet> = serde_json::from_reader(file).unwrap_or_else(|_| vec![]);
    Ok(snippets)
}

#[tauri::command]
pub async fn save_session(app: tauri::AppHandle, state: SessionState) -> Result<(), String> {
    let path = get_file_path(&app, "session.json")?;
    let file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    serde_json::to_writer(file, &state).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn load_session(app: tauri::AppHandle) -> Result<Option<SessionState>, String> {
    let path = get_file_path(&app, "session.json")?;
    if !path.exists() {
        return Ok(None);
    }
    let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let state: SessionState = serde_json::from_reader(file).map_err(|e| e.to_string())?;
    Ok(Some(state))
}

#[tauri::command]
pub async fn export_query_results(
    state: State<'_, AppState>,
    connection_id: String,
    sql: String,
    file_path: String,
    format: String,
) -> Result<(), String> {
    let driver = state.manager.get_relational(&connection_id)?;
    let mut stream = driver.execute_query_stream("export", &sql, 500, None).await?;
    let mut file = std::fs::File::create(&file_path).map_err(|e| e.to_string())?;

    let mut is_first_batch = true;
    let mut has_written_any_rows = false;

    while let Some(batch_res) = stream.next().await {
        let batch = batch_res?;

        if is_first_batch {
            is_first_batch = false;
            if format == "csv" {
                let headers: Vec<String> = batch.columns.iter().map(|c| c.name.clone()).collect();
                let header_line = headers.join(",") + "\n";
                file.write_all(header_line.as_bytes())
                    .map_err(|e| e.to_string())?;
            } else if format == "json" {
                file.write_all(b"[\n").map_err(|e| e.to_string())?;
            }
        }

        if format == "csv" {
            for row in &batch.rows {
                let fields: Vec<String> = row.iter().map(escape_csv_field).collect();
                let line = fields.join(",") + "\n";
                file.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
            }
        } else if format == "json" {
            for row in &batch.rows {
                let mut map = serde_json::Map::new();
                for (col_idx, col) in batch.columns.iter().enumerate() {
                    map.insert(col.name.clone(), row[col_idx].clone());
                }
                let obj = serde_json::Value::Object(map);
                let json_str = serde_json::to_string(&obj).map_err(|e| e.to_string())?;

                if has_written_any_rows {
                    file.write_all(b",\n").map_err(|e| e.to_string())?;
                } else {
                    has_written_any_rows = true;
                }
                file.write_all(json_str.as_bytes())
                    .map_err(|e| e.to_string())?;
            }
        }
    }

    if format == "json" {
        if is_first_batch {
            file.write_all(b"[\n").map_err(|e| e.to_string())?;
        }
        file.write_all(b"\n]\n").map_err(|e| e.to_string())?;
    }

    Ok(())
}
