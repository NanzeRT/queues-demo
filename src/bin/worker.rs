use std::time::Duration;

use anyhow::Result;
use futures::future::try_join_all;
use queues_demo::api::{QueueCompletedTask, QueueTask};
use rand::random;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<()> {
    let workers = (0..10).map(work);
    try_join_all(workers).await?;
    Ok(())
}

async fn work(i: u32) -> Result<()> {
    let client = reqwest::Client::new();
    loop {
        let res: Option<QueueTask> = {
            client
                .get("http://localhost:3000/queue/get_task")
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?
        };
        let Some(task) = res else {
            println!("Worker {i} has no tasks to do");
            continue;
        };
        println!(
            "Worker {i} got task {:x?} {} {}",
            task.id, task.submission_id, task.exploit
        );
        // work
        sleep(Duration::from_secs_f64(random()) * 10).await;
        println!("Worker {i} done task {:x?}", task.id);
        // Uncomment to simulate task dropping
        // if random() {
        //     continue;
        // }
        let resp = QueueCompletedTask {
            id: task.id,
            info: task.exploit.to_string(),
        };
        client
            .post("http://localhost:3000/queue/submit_completed")
            .json(&resp)
            .send()
            .await?
            .error_for_status()?;
    }
}
