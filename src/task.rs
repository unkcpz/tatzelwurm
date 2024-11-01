use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum State {
    Ready,
    Submit,
    Run,
    Complete,
    Except,
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
}
// lookup look at two table and construct a message send to worker.

// Mock tasks
// task 1: async sleep.
// task 2: sync sleep.
