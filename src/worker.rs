use std::{collections::HashMap, sync::Arc};

use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

use crate::codec::IMessage;

#[derive(Debug, Clone)]
pub struct Worker {
    // The rx used for communicate
    // TODO: make fields private
    pub tx: mpsc::Sender<IMessage>,

    // number of processes running on this worker
    pub load: u64,
}

// TODO: use actor pattern
#[derive(Debug, Clone)]
pub struct Table {
    // ??: which Mutex to use, can I use std sync mutex??
    inner: Arc<Mutex<HashMap<Uuid, Worker>>>,
}

impl Table {
    #[allow(clippy::new_without_default)]
    #[must_use]
    pub fn new() -> Self {
        Table {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn create(&self, w: Worker) -> Uuid {
        let id = Uuid::new_v4();
        self.inner.lock().await.insert(id, w);

        id
    }

    pub async fn update(&self, id: &Uuid, w: Worker) -> anyhow::Result<()> {
        let mut table = self.inner.lock().await;
        if table.contains_key(id) {
            table.insert(*id, w);
            Ok(())
        } else {
            anyhow::bail!("Item {id} not found")
        }
    }

    pub async fn read(&self, id: &Uuid) -> Option<Worker> {
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

    pub async fn find_least_loaded_worker(&self) -> Option<Uuid> {
        self.inner
            .lock()
            .await
            .iter()
            .min_by_key(|&(_, w)| w.load)
            .map(|(&id, _)| id)
    }
}
