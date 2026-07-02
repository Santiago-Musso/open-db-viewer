mod keychain;
mod productivity;
mod state;

use driver_api::{
    ConnectionConfig, KeyValue, ScanResult, SchemaGraph, SchemaInfo, ServerInfo, TableInfo,
    TableSchema, DatabaseError,
};
use state::AppState;
use std::fs::File;
use std::io::{Read, Write};
use tauri::{Emitter, Manager, State, Window};

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ConnectionProfile {
    pub driver_id: String,
    pub name: String,
    pub config: ConnectionConfig,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ConnectionProfileResponse {
    pub driver_id: String,
    pub name: String,
    pub config: ConnectionConfig,
    pub has_password_saved: bool,
}

fn get_config_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let mut path = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    path.push("connections.json");
    Ok(path)
}

fn resolve_config_password(mut config: ConnectionConfig) -> ConnectionConfig {
    if config.password.is_none()
        || config
            .password
            .as_ref()
            .map(|s| s.is_empty())
            .unwrap_or(true)
    {
        if let Ok(pwd) = keychain::get_db_password(&config.id) {
            config.password = Some(pwd);
        }
    }
    config
}

#[tauri::command]
async fn connect_db(
    state: State<'_, AppState>,
    driver_id: String,
    config: ConnectionConfig,
) -> Result<(), String> {
    let resolved_config = resolve_config_password(config);
    state.manager.connect(&driver_id, &resolved_config).await
}

#[tauri::command]
fn disconnect_db(state: State<'_, AppState>, connection_id: String) -> Result<(), String> {
    state.manager.disconnect(&connection_id)
}

#[tauri::command]
async fn list_schemas(
    state: State<'_, AppState>,
    connection_id: String,
) -> Result<Vec<SchemaInfo>, DatabaseError> {
    println!(
        "DEBUG: list_schemas called for connection: {}",
        connection_id
    );
    let driver = state.manager.get_relational(&connection_id).map_err(DatabaseError::from)?;
    let res = driver.list_schemas().await;
    println!("DEBUG: list_schemas result: {:?}", res);
    res
}

#[tauri::command]
async fn list_tables(
    state: State<'_, AppState>,
    connection_id: String,
    schema: String,
) -> Result<Vec<TableInfo>, DatabaseError> {
    let driver = state.manager.get_relational(&connection_id).map_err(DatabaseError::from)?;
    driver.list_tables(&schema).await
}

#[tauri::command]
async fn describe_table(
    state: State<'_, AppState>,
    connection_id: String,
    schema: String,
    table: String,
) -> Result<TableSchema, DatabaseError> {
    let driver = state.manager.get_relational(&connection_id).map_err(DatabaseError::from)?;
    driver.describe_table(&schema, &table).await
}

#[tauri::command]
async fn get_table_ddl(
    state: State<'_, AppState>,
    connection_id: String,
    schema: String,
    table: String,
) -> Result<String, DatabaseError> {
    let driver = state.manager.get_relational(&connection_id).map_err(DatabaseError::from)?;
    driver.get_table_ddl(&schema, &table).await
}

#[tauri::command]
async fn get_schema_graph(
    state: State<'_, AppState>,
    connection_id: String,
    schema: String,
) -> Result<SchemaGraph, DatabaseError> {
    let driver = state.manager.get_relational(&connection_id).map_err(DatabaseError::from)?;
    driver.get_schema_graph(&schema).await
}

#[tauri::command]
async fn execute_query(
    window: Window,
    state: State<'_, AppState>,
    connection_id: String,
    query_id: String,
    sql: String,
    batch_size: usize,
    offset: Option<usize>,
) -> Result<(), DatabaseError> {
    let driver = state.manager.get_relational(&connection_id).map_err(DatabaseError::from)?;
    let mut stream = driver
        .execute_query_stream(&query_id, &sql, batch_size, offset)
        .await?;

    tokio::spawn(async move {
        use futures_util::StreamExt;
        let mut total_rows = 0;

        while let Some(batch_res) = stream.next().await {
            match batch_res {
                Ok(batch) => {
                    total_rows += batch.rows.len();
                    let payload = serde_json::json!({
                        "query_id": query_id,
                        "batch": batch,
                    });
                    if let Err(e) = window.emit("query:batch", payload) {
                        eprintln!("failed to emit query:batch: {}", e);
                        break;
                    }
                }
                Err(e) => {
                    let payload = serde_json::json!({
                        "query_id": query_id,
                        "error": e,
                    });
                    let _ = window.emit("query:error", payload);
                    return;
                }
            }
        }

        let payload = serde_json::json!({
            "query_id": query_id,
            "row_count": total_rows,
        });
        let _ = window.emit("query:done", payload);
    });

    Ok(())
}

#[tauri::command]
async fn cancel_query(
    state: State<'_, AppState>,
    connection_id: String,
    query_id: String,
) -> Result<(), DatabaseError> {
    let driver = state.manager.get_relational(&connection_id).map_err(DatabaseError::from)?;
    driver.cancel_query(&query_id).await
}

#[tauri::command]
async fn refresh_metadata_cache(
    state: State<'_, AppState>,
    connection_id: String,
    schema: Option<String>,
    table: Option<String>,
) -> Result<(), DatabaseError> {
    let driver = state.manager.get_relational(&connection_id).map_err(DatabaseError::from)?;
    if let Some(tbl) = table {
        if let Some(sch) = schema {
            driver.refresh_table(&sch, &tbl).await?;
        }
    } else if let Some(sch) = schema {
        driver.refresh_schema(&sch).await?;
    }
    Ok(())
}

#[tauri::command]
async fn test_connection(
    state: State<'_, AppState>,
    driver_id: String,
    config: ConnectionConfig,
) -> Result<(), String> {
    let resolved_config = resolve_config_password(config);
    state
        .manager
        .test_connection(&driver_id, &resolved_config)
        .await
}

#[tauri::command]
async fn save_connection_profile(
    app: tauri::AppHandle,
    driver_id: String,
    name: String,
    mut config: ConnectionConfig,
) -> Result<(), String> {
    if let Some(password) = &config.password {
        if !password.is_empty() {
            keychain::set_db_password(&config.id, password)?;
        }
    }
    config.password = None;

    let path = get_config_path(&app)?;
    let mut profiles: Vec<ConnectionProfile> = if path.exists() {
        let mut file = File::open(&path).map_err(|e| e.to_string())?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .map_err(|e| e.to_string())?;
        serde_json::from_str(&contents).unwrap_or_else(|_| vec![])
    } else {
        vec![]
    };

    if let Some(pos) = profiles.iter().position(|p| p.config.id == config.id) {
        profiles[pos] = ConnectionProfile {
            driver_id,
            name,
            config,
        };
    } else {
        profiles.push(ConnectionProfile {
            driver_id,
            name,
            config,
        });
    }

    let mut file = File::create(&path).map_err(|e| e.to_string())?;
    let serialized = serde_json::to_string_pretty(&profiles).map_err(|e| e.to_string())?;
    file.write_all(serialized.as_bytes())
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
async fn load_connection_profiles(
    app: tauri::AppHandle,
) -> Result<Vec<ConnectionProfileResponse>, String> {
    let path = get_config_path(&app)?;
    if !path.exists() {
        return Ok(vec![]);
    }

    let mut file = File::open(&path).map_err(|e| e.to_string())?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| e.to_string())?;
    let profiles: Vec<ConnectionProfile> =
        serde_json::from_str(&contents).unwrap_or_else(|_| vec![]);

    let mut response = vec![];
    for p in profiles {
        let has_pwd = keychain::has_db_password(&p.config.id).unwrap_or(false);
        response.push(ConnectionProfileResponse {
            driver_id: p.driver_id,
            name: p.name,
            config: p.config,
            has_password_saved: has_pwd,
        });
    }

    Ok(response)
}

#[tauri::command]
async fn delete_connection_profile(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let _ = keychain::delete_db_password(&id);

    let path = get_config_path(&app)?;
    if !path.exists() {
        return Ok(());
    }

    let mut file = File::open(&path).map_err(|e| e.to_string())?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| e.to_string())?;
    let mut profiles: Vec<ConnectionProfile> =
        serde_json::from_str(&contents).unwrap_or_else(|_| vec![]);

    profiles.retain(|p| p.config.id != id);

    let mut file = File::create(&path).map_err(|e| e.to_string())?;
    let serialized = serde_json::to_string_pretty(&profiles).map_err(|e| e.to_string())?;
    file.write_all(serialized.as_bytes())
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
async fn redis_scan_keys(
    state: State<'_, AppState>,
    connection_id: String,
    pattern: String,
    cursor: u64,
    count: usize,
) -> Result<ScanResult, String> {
    let driver = state.manager.get_key_value(&connection_id)?;
    driver.scan_keys(&pattern, cursor, count).await
}

#[tauri::command]
async fn redis_get_key(
    state: State<'_, AppState>,
    connection_id: String,
    key: String,
) -> Result<KeyValue, String> {
    let driver = state.manager.get_key_value(&connection_id)?;
    driver.get_key(&key).await
}

#[tauri::command]
async fn redis_set_key(
    state: State<'_, AppState>,
    connection_id: String,
    key: String,
    value: String,
    ttl: Option<i64>,
) -> Result<(), String> {
    let driver = state.manager.get_key_value(&connection_id)?;
    driver.set_key(&key, &value, ttl).await
}

#[tauri::command]
async fn redis_delete_key(
    state: State<'_, AppState>,
    connection_id: String,
    key: String,
) -> Result<(), String> {
    let driver = state.manager.get_key_value(&connection_id)?;
    driver.delete_key(&key).await
}

#[tauri::command]
async fn redis_server_info(
    state: State<'_, AppState>,
    connection_id: String,
) -> Result<ServerInfo, String> {
    let driver = state.manager.get_key_value(&connection_id)?;
    driver.server_info().await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            connect_db,
            disconnect_db,
            list_schemas,
            list_tables,
            describe_table,
            get_table_ddl,
            get_schema_graph,
            execute_query,
            cancel_query,
            refresh_metadata_cache,
            test_connection,
            save_connection_profile,
            load_connection_profiles,
            delete_connection_profile,
            redis_scan_keys,
            redis_get_key,
            redis_set_key,
            redis_delete_key,
            redis_server_info,
            productivity::log_query,
            productivity::get_query_history,
            productivity::save_snippet,
            productivity::delete_snippet,
            productivity::load_snippets,
            productivity::save_session,
            productivity::load_session,
            productivity::export_query_results
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
