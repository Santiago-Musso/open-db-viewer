#[cfg(not(debug_assertions))]
mod implementation {
    use keyring::Entry;

    const SERVICE_NAME: &str = "com.santiagomusso.tauri-app";

    pub fn set_db_password(conn_id: &str, password: &str) -> Result<(), String> {
        let entry = Entry::new(SERVICE_NAME, conn_id).map_err(|e| e.to_string())?;
        entry.set_password(password).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_db_password(conn_id: &str) -> Result<String, String> {
        let entry = Entry::new(SERVICE_NAME, conn_id).map_err(|e| e.to_string())?;
        entry.get_password().map_err(|e| e.to_string())
    }

    pub fn delete_db_password(conn_id: &str) -> Result<(), String> {
        let entry = Entry::new(SERVICE_NAME, conn_id).map_err(|e| e.to_string())?;
        match entry.delete_password() {
            Ok(_) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn has_db_password(conn_id: &str) -> Result<bool, String> {
        let entry = Entry::new(SERVICE_NAME, conn_id).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(_) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(debug_assertions)]
mod implementation {
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::{Read, Write};
    use std::path::PathBuf;

    fn get_file_path() -> PathBuf {
        std::env::temp_dir().join("tauri-db-viewer-dev-passwords.json")
    }

    fn read_store() -> HashMap<String, String> {
        if let Ok(mut file) = File::open(get_file_path()) {
            let mut contents = String::new();
            if file.read_to_string(&mut contents).is_ok() {
                if let Ok(store) = serde_json::from_str(&contents) {
                    return store;
                }
            }
        }
        HashMap::new()
    }

    fn write_store(store: &HashMap<String, String>) -> Result<(), String> {
        let serialized = serde_json::to_string(store).map_err(|e| e.to_string())?;
        let mut file = File::create(get_file_path()).map_err(|e| e.to_string())?;
        file.write_all(serialized.as_bytes()).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn set_db_password(conn_id: &str, password: &str) -> Result<(), String> {
        let mut store = read_store();
        store.insert(conn_id.to_string(), password.to_string());
        write_store(&store)
    }

    pub fn get_db_password(conn_id: &str) -> Result<String, String> {
        let store = read_store();
        store.get(conn_id).cloned().ok_or_else(|| "No entry".to_string())
    }

    pub fn delete_db_password(conn_id: &str) -> Result<(), String> {
        let mut store = read_store();
        store.remove(conn_id);
        write_store(&store)
    }

    pub fn has_db_password(conn_id: &str) -> Result<bool, String> {
        let store = read_store();
        Ok(store.contains_key(conn_id))
    }
}

pub use implementation::*;
