use std::{io, marker::PhantomData};

use bytes::{Buf, BufMut};
use rmp_serde::{encode, Deserializer};
use serde::{Deserialize, Serialize};
use tokio_util::codec::{Decoder, Encoder};
use uuid::Uuid;

use crate::task::State;

#[derive(Serialize, Deserialize, Debug)]
pub enum Operation {
    Inspect,
    Submit,

    // CURD to the table (s)
    Create,
    Update,
    Read, // similar to Inspect
    Delete, 
}

// TODO: should look at rmp-rpc, see if use it or re-implement as same way
#[derive(Serialize, Deserialize, Debug)]
pub enum XMessage {
    // for fallback general unknown type messages
    Message {id: u32, content: String},

    // The Uuid is the task uuid
    TaskDispatch(Uuid), 

    // hand shake message when the msg content is a string
    HandShake(String),

    // Heartbeat with the port as identifier
    HeartBeat(u16),

    // Operations from actioner
    ActionerOp(Operation),

    // Operations from worker
    // XXX: may combine with the ActionerOp??
    WorkerOp{ op: Operation, id: Uuid, from: State, to: State},
}

// A custom CodeC that handles de/serialize messagepack data
pub struct Codec<T> {
    _marker: PhantomData<T>,
}

impl<T> Codec<T> {

    #[must_use]
    pub fn new() -> Self {
        Codec {
            _marker: PhantomData,
        }
    }
}

impl<T> Default for Codec<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Decoder for Codec<T>
where
    T: for<'de> Deserialize<'de>,
{
    type Item = T;
    type Error = io::Error;

    fn decode(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 4 {
            return Ok(None);
        }

        let len = src.get_u32() as usize;

        if src.len() < len {
            src.reserve(len - src.len());
            return Ok(None);
        }

        let buf = src.split_to(len);

        // Deserialize the MessagePackData
        let mut de = Deserializer::from_read_ref(&buf);
        let item = Deserialize::deserialize(&mut de)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Ok(Some(item))
    }
}

impl<T> Encoder<T> for Codec<T>
where
    T: Serialize,
{
    type Error = io::Error;

    fn encode(&mut self, item: T, dst: &mut bytes::BytesMut) -> Result<(), Self::Error> {
        // Serialize the item to MessagePack
        let mut buf = Vec::new();
        encode::write(&mut buf, &item).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // Write the length of the message to the header
        let len = buf.len() as u32;
        dst.put_u32(len);

        dst.extend_from_slice(&buf);

        Ok(())
    }
}
