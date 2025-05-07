use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use queues_demo::api::QueueAddTask;
use rand::random_range;
use tokio::time::sleep;

#[derive(Debug, Parser)]
struct Cli {
    #[arg(long, short, default_value_t = 1.0)]
    interval: f64,
    #[arg(long, short, default_value_t = 1000)]
    max_id: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = reqwest::Client::new();
    loop {
        let s = sleep(Duration::from_secs_f64(cli.interval));
        let req = QueueAddTask {
            submission_id: format!("task{:x}", random_range(0..cli.max_id)),
        };
        println!("Submiting {}", req.submission_id);
        client
            .post("http://localhost:3000/queue/add_task")
            .json(&req)
            .send()
            .await?
            .error_for_status()?;
        s.await;
    }
}
