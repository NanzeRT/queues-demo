use std::{collections::VecDeque, ops::Deref, sync::Mutex, time::Duration};

use dlv_list::VecList;
use serde::{Deserialize, Serialize, Serializer};
use serde_with::SerializeAs;
use tokio::{select, sync::Notify, time::sleep};

use crate::utils::Timed;

#[derive(Debug)]
pub struct GenericTaskQueueWithBackup<T, const EXECUTION_TIMEOUT_MILLIS: u128> {
    queue: GenericTaskQueue<T, EXECUTION_TIMEOUT_MILLIS>,
    db: sled::Db,
}

// TODO: Now it may fail on interaction with db
impl<T: Serialize + for<'de> Deserialize<'de> + Clone, const ET: u128>
    GenericTaskQueueWithBackup<T, ET>
{
    pub fn new(db: sled::Db) -> Self {
        let queue = GenericTaskQueue::default();
        let x = Self { queue, db };
        x.init_with_db();
        x
    }

    fn init_with_db(&self) {
        for item in self.db.iter() {
            let (item, _) = item.unwrap();
            let (task, _): (T, _) = bincode::serde::decode_from_slice(&item, bincode::config::standard()).unwrap();
            self.queue.push(task);
        }
    }

    pub fn push(&self, item: T) {
        self.queue.push(item.clone());
        self.db
            .insert(bincode::serde::encode_to_vec(&item, bincode::config::standard()).unwrap(), &[])
            .unwrap();
    }

    pub async fn pop_with_timeout(&self, timeout: Duration) -> Option<(T, TaskId<T>)> {
        self.queue.pop_with_timeout(timeout).await
    }

    pub fn submit_completed(&self, id: &TaskId<T>) -> Option<T> {
        let res = self.queue.submit_completed(id);
        if let Some(task) = &res {
            self.db.remove(bincode::serde::encode_to_vec(task, bincode::config::standard()).unwrap()).unwrap();
        }
        res
    }

    pub async fn submit_completed_with_inspect<R>(
        &self,
        id: &TaskId<T>,
        inspect: impl AsyncFnOnce(Option<T>) -> R,
    ) -> R {
        match self.queue.submit_completed(id) {
            Some(task) => {
                let key = bincode::serde::encode_to_vec(&task, bincode::config::standard()).unwrap();
                let res = inspect(Some(task)).await;
                self.db.remove(key).unwrap();
                res
            }
            None => inspect(None).await,
        }
    }

    pub fn process_timeouts(&self) {
        self.queue.process_timeouts();
    }

    pub fn process_timeouts_with_inspect(&self, inspect: impl Fn(TaskId<T>, &T)) {
        self.queue.process_timeouts_with_inspect(inspect);
    }

    pub fn len_pending(&self) -> usize {
        self.queue.len_pending()
    }

    pub fn len_processing(&self) -> usize {
        self.queue.len_processing()
    }
}

#[derive(Debug)]
pub struct GenericTaskQueue<T, const EXECUTION_TIMEOUT_MILLIS: u128> {
    notify_incoming: Notify,
    // NOTE: lock in order of definition
    pending: Mutex<VecDeque<T>>,
    processing: Mutex<VecList<Timed<T>>>,
}

impl<T, const ET: u128> Default for GenericTaskQueue<T, ET> {
    fn default() -> Self {
        Self {
            notify_incoming: Notify::new(),
            pending: Mutex::new(VecDeque::new()),
            processing: Mutex::new(VecList::new()),
        }
    }
}

impl<T: Clone, const EXECUTION_TIMEOUT_MILLIS: u128> GenericTaskQueue<T, EXECUTION_TIMEOUT_MILLIS> {
    pub fn push(&self, item: T) {
        self.pending.lock().expect("Mutex poisoned").push_back(item);
        self.notify_incoming.notify_one();
    }

    pub async fn pop_with_timeout(&self, timeout: Duration) -> Option<(T, TaskId<T>)> {
        let mut timeout = Box::pin(sleep(timeout));
        loop {
            if let Some(item) = self.pending.lock().expect("Mutex poisoned").pop_front() {
                let id = self
                    .processing
                    .lock()
                    .expect("Mutex poisoned")
                    .push_back(Timed::new(item.clone()));
                return Some((item, TaskId(id)));
            };
            select! {
                _ = self.notify_incoming.notified() => {},
                _ = &mut timeout => {
                    return None;
                },
            }
        }
    }

    pub fn submit_completed(&self, id: &TaskId<T>) -> Option<T> {
        self.processing
            .lock()
            .expect("Mutex poisoned")
            .remove(id.0)
            .map(|task| task.value)
    }

    pub fn process_timeouts(&self) {
        self.process_timeouts_with_inspect(|_, _| {})
    }

    pub fn process_timeouts_with_inspect(&self, inspect: impl Fn(TaskId<T>, &T)) {
        let mut pending = self.pending.lock().expect("Mutex poisoned");
        let mut processing = self.processing.lock().expect("Mutex poisoned");
        while let Some(task) = processing.front() {
            if task.timestamp.elapsed().as_millis() > EXECUTION_TIMEOUT_MILLIS {
                let id = processing.front_index().expect("Unreachable");
                let task = processing.pop_front().expect("Unreachable");
                inspect(TaskId(id), &task.value);
                pending.push_back(task.value);
                self.notify_incoming.notify_one();
            } else {
                break;
            }
        }
    }

    pub fn len_pending(&self) -> usize {
        let pending = self.pending.lock().expect("Mutex poisoned");
        pending.len()
    }

    pub fn len_processing(&self) -> usize {
        let processing = self.processing.lock().expect("Mutex poisoned");
        processing.len()
    }
}

#[derive(Debug, Copy, Serialize, Deserialize)]
#[serde(from = "[u8; 16]", into = "[u8; 16]")]
#[serde(bound(serialize = "", deserialize = ""))]
pub struct TaskId<T>(dlv_list::Index<Timed<T>>);

impl<T> Clone for TaskId<T> {
    fn clone(&self) -> Self {
        TaskId(self.0)
    }
}

impl<T> From<TaskId<T>> for [u8; 16] {
    fn from(id: TaskId<T>) -> Self {
        id.0.to_bytes()
    }
}

impl<T> From<[u8; 16]> for TaskId<T> {
    fn from(bytes: [u8; 16]) -> Self {
        Self(dlv_list::Index::from_bytes(bytes))
    }
}

impl<T> TryFrom<Vec<u8>> for TaskId<T> {
    type Error = <[u8; 16] as TryFrom<Vec<u8>>>::Error;
    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        let arr: [u8; 16] = bytes.try_into()?;
        Ok(TaskId::from(arr))
    }
}

impl<T> SerializeAs<TaskId<T>> for serde_with::hex::Hex {
    fn serialize_as<S>(source: &TaskId<T>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(source.0.to_bytes()))
    }
}

impl<T> Deref for TaskId<T> {
    type Target = dlv_list::Index<Timed<T>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
