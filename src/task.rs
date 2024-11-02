use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tabled::builder::Builder as TableBuilder;
use tabled::settings::Style;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum State {
    Ready,
    Submit,
    Run,
    Complete,
    Except,
    Killed,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            State::Ready => write!(f, "ready"),
            State::Submit => write!(f, "submit"),
            State::Run => write!(f, "running"),
            State::Complete => write!(f, "complete"),
            State::Except => write!(f, "Execpt"),
            State::Killed => write!(f, "Killed"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Task {
    pub state: State,
    pub priority: u32,
    pub worker: Option<Uuid>,
}

impl Task {
    #[must_use]
    pub fn new(priority: u32) -> Self {
        Self {
            state: State::Ready,
            priority,
            worker: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Table {
    // ??: which Mutex to use, can I use std sync mutex??
    pub inner: Arc<Mutex<HashMap<Uuid, Task>>>,
}

impl Table {
    #[allow(clippy::new_without_default)]
    #[must_use]
    pub fn new() -> Self {
        Table {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn create(&self, w: Task) -> Uuid {
        let id = Uuid::new_v4();
        self.inner.lock().await.insert(id, w);

        id
    }

    pub async fn update(&self, id: &Uuid, w: Task) -> anyhow::Result<()> {
        let mut table = self.inner.lock().await;
        if table.contains_key(id) {
            table.insert(*id, w);
            Ok(())
        } else {
            anyhow::bail!("Item {id} not found")
        }
    }

    pub async fn read(&self, id: &Uuid) -> Option<Task> {
        self.inner.lock().await.get(id).cloned()
    }

    pub async fn delete(&self, id: &Uuid) -> anyhow::Result<()> {
        let mut table = self.inner.lock().await;
        if table.remove(id).is_some() {
            Ok(())
        } else {
            anyhow::bail!("Item {id} not found")
        }
    }

    /// Render a pretty printed table
    pub async fn render(&self) -> String {
        let mut builder = TableBuilder::default();
        for (id, task) in self.inner.lock().await.iter() {
            let worker_id = if let Some(worker) = task.worker {
                // XXX: ugly, show first 8 shorten chars
                worker.to_string()[..8].to_string()
            } else {
                "-".to_string()
            };

            let line = vec![
                id.to_string(),
                format!("{}", task.priority),
                format!("{}", task.state),
                format!("{}", worker_id),
            ];
            builder.push_record(line);
        }
        let header = vec![
            "Uuid".to_string(),
            "priority".to_string(),
            "state".to_string(),
            "worker".to_string(),
        ];
        builder.insert_record(0, header);

        let mut table = builder.build();
        table.with(Style::modern());
        table.to_string()
    }

    pub async fn filter_by_state(&self, state: State) -> HashMap<Uuid, Task> {
        self.inner
            .lock()
            .await
            .iter()
            .filter(|(_, t)| t.state == state)
            .map(|(&x, t)| (x, t.clone()))
            .collect()
    }
}
// lookup look at two table and construct a message send to worker.

// Mock tasks
// task 1: async sleep.
// task 2: sync sleep.
