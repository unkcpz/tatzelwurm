use std::{io, marker::PhantomData};

use bytes::{Buf, BufMut};
use rmp_serde::{decode, encode, Deserializer};
use serde::{Deserialize, Serialize};
use tokio_util::codec::{Decoder, Encoder};

// TODO: should look at rmp-rpc, see if use it or re-implement
#[derive(Serialize, Deserialize, Debug)]
pub struct TMessage {
    pub id: u32,
    pub content: String,
}

// A custom CodeC that handles de/serialize messagepack data
pub struct Codec<T> {
    _marker: PhantomData<T>,
}

impl<T> Codec<T> {
    pub fn new() -> Self {
        Codec {
            _marker: PhantomData,
        }
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
