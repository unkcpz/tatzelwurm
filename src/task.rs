use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tabled::builder::Builder as TableBuilder;
use tabled::settings::Style;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum State {
    Created,
    Ready,
    Submit,
    Pause,
    Run,
    // XXX: distinguish kill and complete as dedicate states
    Terminated(i8),
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            State::Created => write!(f, "created"),
            State::Ready => write!(f, "ready"),
            State::Submit => write!(f, "submit"),
            State::Pause => write!(f, "pause"),
            State::Run => write!(f, "running"),
            State::Terminated(exit_code) if *exit_code == 0 => {
                write!(f, "complete")
            }
            State::Terminated(exit_code) if *exit_code == -1 => {
                write!(f, "killed")
            }
            State::Terminated(exit_code) => {
                write!(f, "terminated(exit_code={exit_code})")
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Task {
    // pub task_pool: Uuid, // XXX: ??

    // The id in task pool to se/de the task to/from
    pub id: String,

    // state of task in coordinator table
    pub state: State,

    // task priority
    pub priority: u32,

    // worker that runs this task
    pub worker: Option<Uuid>,
}

impl Task {
    #[must_use]
    pub fn new(priority: u32, id: &str) -> Self {
        Self {
            id: id.to_string(),
            state: State::Created,
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

    #[allow(clippy::must_use_candidate)]
    pub fn from_mapping(mapping: HashMap<Uuid, Task>) -> Self {
        Table {
            inner: Arc::new(Mutex::new(mapping)),
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

    pub async fn filter_by_states(&self, states: Vec<State>) -> HashMap<Uuid, Task> {
        self.inner
            .lock()
            .await
            .iter()
            .filter(|(_, t)| states.contains(&t.state))
            .map(|(&x, t)| (x, t.clone()))
            .collect()
    }

    pub async fn count(&self) -> HashMap<State, u64> {
        let mut state_count = HashMap::<State, u64>::new();
        for (_, task) in self.inner.lock().await.iter() {
            match task.state {
                State::Created => state_count
                    .entry(State::Created)
                    .and_modify(|e| *e += 1)
                    .or_insert(1),
                State::Ready => state_count
                    .entry(State::Ready)
                    .and_modify(|e| *e += 1)
                    .or_insert(1),
                State::Submit => state_count
                    .entry(State::Submit)
                    .and_modify(|e| *e += 1)
                    .or_insert(1),
                State::Pause => state_count
                    .entry(State::Pause)
                    .and_modify(|e| *e += 1)
                    .or_insert(1),
                State::Run => state_count
                    .entry(State::Run)
                    .and_modify(|e| *e += 1)
                    .or_insert(1),
                State::Terminated(x) => state_count
                    .entry(State::Terminated(x))
                    .and_modify(|e| *e += 1)
                    .or_insert(1),
            };
        }

        state_count
    }
}
// lookup look at two table and construct a message send to worker.

// Mock tasks
// task 1: async sleep.
// task 2: sync sleep.
