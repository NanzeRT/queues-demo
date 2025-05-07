use std::{sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use tokio::time::sleep;

use crate::{
    AppState, CacheState,
    queue::{GenericTaskQueueWithBackup, TaskId},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/add_task", post(queue_add_task))
        .route("/get_task", get(queue_get_task))
        .route("/submit_completed", post(queue_submit_completed))
}

pub type MainQueue = GenericTaskQueueWithBackup<String, 30_000>;

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueTask {
    #[serde_as(as = "serde_with::hex::Hex")]
    pub id: TaskId<String>,
    pub submission_id: String,
    pub exploit: Arc<String>,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueCompletedTask {
    #[serde_as(as = "serde_with::hex::Hex")]
    pub id: TaskId<String>,
    pub info: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueTaskCompletion {
    pub submission_id: String,
    pub info: String,
}

#[derive(Debug)]
pub struct QueueState {
    pub queue: MainQueue,
    pub client: reqwest::Client,
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize)]
pub struct QueueAddTask {
    pub submission_id: String,
}

pub async fn queue_add_task(State(state): State<Arc<QueueState>>, task: Json<QueueAddTask>) {
    println!("Adding task {:?}", task);
    let QueueAddTask { submission_id } = task.0;
    state.queue.push(submission_id);
}

pub async fn queue_get_task(
    State(state): State<Arc<QueueState>>,
    State(cache): State<Arc<CacheState>>,
) -> Json<Option<QueueTask>> {
    let Some((addr, id)) = state.queue.pop_with_timeout(Duration::from_secs(10)).await else {
        return Json(None);
    };
    let task = QueueTask {
        id,
        exploit: cache.exploits.get(&addr).await,
        submission_id: addr,
    };
    Json(Some(task))
}

pub async fn queue_submit_completed(
    State(state): State<Arc<QueueState>>,
    Json(task): Json<QueueCompletedTask>,
) {
    state
        .queue
        .submit_completed_with_inspect(&task.id, async |entry| match entry {
            Some(submission_id) => {
                println!("Task {} completed: {}", submission_id, task.info);
                let req = QueueTaskCompletion {
                    submission_id,
                    info: task.info.clone(),
                };
                state
                    .client
                    .post("http://localhost:3002/submit")
                    .json(&req)
                    .send()
                    .await
                    .unwrap()
                    .error_for_status()
                    .unwrap();
            }
            None => {
                println!(
                    "Task not found: {}, id: {}",
                    task.info,
                    hex::encode(task.id.to_bytes())
                );
            }
        })
        .await;
}

pub async fn queue_collect_timeouts(state: Arc<QueueState>) {
    loop {
        sleep(Duration::from_secs(1)).await;
        state.queue.process_timeouts_with_inspect(|id, task| {
            println!(
                "Task timeout: {}, id: {}",
                &task,
                hex::encode(id.to_bytes())
            );
        });
        println!("Tasks left: {} pending, {} processing", state.queue.len_pending(), state.queue.len_processing());
    }
}
