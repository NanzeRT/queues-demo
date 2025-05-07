use anyhow::Result;
use axum::{Json, Router, routing::post};
use queues_demo::api::QueueTaskCompletion;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    let app = Router::new().route(
        "/submit",
        post(async |Json(task): Json<QueueTaskCompletion>| {
            println!(
                "Task {} completed with info: {}",
                task.submission_id, task.info
            );
        }),
    );
    let listener = TcpListener::bind("[::]:3002").await?;
    let local_addr = listener.local_addr()?;
    println!("listening on {}", local_addr);
    axum::serve(listener, app).await?;
    Ok(())
}
