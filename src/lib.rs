use api::QueueState;
use axum::extract::FromRef;
use cache::DataGetter;
use std::sync::Arc;

pub mod api;
pub mod cache;
pub mod queue;
pub mod utils;

#[derive(Debug, Default)]
pub struct GetterStub {
    client: reqwest::Client,
}

impl DataGetter for GetterStub {
    type Key = String;
    type BorrowedKey = str;
    type Value = Arc<String>;
    async fn get(&self, key: &str) -> Arc<String> {
        // TODO: this shouldn't panic silently
        Arc::new(
            self.client
                .get(format!("http://localhost:3001/get_exploit/{key}"))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .text()
                .await
                .unwrap(),
        )
    }
}

impl FromRef<AppState> for Arc<CacheState> {
    fn from_ref(state: &AppState) -> Self {
        state.cache.clone()
    }
}

impl FromRef<AppState> for Arc<QueueState> {
    fn from_ref(state: &AppState) -> Self {
        state.api.clone()
    }
}

#[derive(Debug, Default)]
pub struct CacheState {
    pub exploits: cache::Cache<GetterStub, 30_000, 600_000>,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub api: Arc<QueueState>,
    pub cache: Arc<CacheState>,
}
