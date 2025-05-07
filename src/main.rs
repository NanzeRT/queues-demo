use std::{sync::Arc, time::Duration};

use queues_demo::{
    AppState, CacheState,
    api::{MainQueue, QueueState},
};
use tokio::{select, time::sleep};

async fn cache_collect_expires(state: Arc<CacheState>) -> ! {
    loop {
        sleep(Duration::from_secs(10)).await;
        let expires = state.exploits.evict_expired();
        for expire in expires {
            eprintln!(
                "Cache \"bytecodes\": key {} expired while having {} usages",
                expire.key, expire.usages
            );
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = sled::open("queue.db")?;
    let state = AppState {
        api: Arc::new(QueueState {
            queue: MainQueue::new(db),
            client: reqwest::Client::new(),
        }),
        cache: Arc::new(CacheState::default()),
    };
    let state_queue = state.api.clone();
    let state_cache = state.cache.clone();

    let app = axum::Router::new()
        .nest("/queue", queues_demo::api::routes())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("[::]:3000").await?;
    let local_addr = listener.local_addr()?;
    println!("listening on {}", local_addr);
    select! {
        res = axum::serve(listener, app) => {
            res?;
        },
        _ = queues_demo::api::queue_collect_timeouts(state_queue) => {
            unreachable!();
        },
        _ = cache_collect_expires(state_cache) => {
            unreachable!();
        },
    }
    Ok(())
}
