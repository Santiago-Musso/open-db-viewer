use core_db::ConnectionManager;

pub struct AppState {
    pub manager: ConnectionManager,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            manager: ConnectionManager::new(),
        }
    }
}
