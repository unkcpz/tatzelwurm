use std::{collections::HashMap, sync::Arc};

use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

use crate::codec::XMessage;

#[derive(Debug)]
pub struct Worker {
    // The rx used for communicate
    // TODO: make fields private
    pub tx: mpsc::Sender<XMessage>,

    // number of processes running on this worker
    pub load: u64,
}

// XXX: use tokio Mutex or sync Mutex?
pub type Table = Arc<Mutex<HashMap<Uuid, Worker>>>;
