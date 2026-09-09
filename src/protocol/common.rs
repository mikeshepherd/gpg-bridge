use crate::protocol::{Decode, ProtocolError, ProtocolResult as Result};
use tokio_util::bytes::{Buf, BytesMut};

impl Decode for bool {
    type Item = bool;
    fn decode_with_size(reader: &mut BytesMut) -> Result<(usize, Self::Item)> {
        reader
            .try_get_u8()
            .map(|v| (1, v != 0))
            .map_err(|e| e.into())
    }
}

impl Decode for u8 {
    type Item = u8;
    fn decode_with_size(reader: &mut BytesMut) -> Result<(usize, Self::Item)> {
        reader.try_get_u8().map(|v| (1, v)).map_err(|e| e.into())
    }
}

impl Decode for u32 {
    type Item = u32;
    fn decode_with_size(reader: &mut BytesMut) -> Result<(usize, Self::Item)> {
        reader.try_get_u32().map(|v| (4, v)).map_err(|e| e.into())
    }
}

impl Decode for u64 {
    type Item = u64;
    fn decode_with_size(reader: &mut BytesMut) -> Result<(usize, Self::Item)> {
        reader.try_get_u64().map(|v| (8, v)).map_err(|e| e.into())
    }
}

impl Decode for String {
    type Item = String;
    fn decode_with_size(reader: &mut BytesMut) -> Result<(usize, Self::Item)> {
        let (length, bytes) = BytesMut::decode_with_size(reader)?;
        Ok((length, String::from_utf8(bytes.to_vec())?))
    }
}

impl Decode for BytesMut {
    type Item = BytesMut;
    fn decode_with_size(reader: &mut BytesMut) -> Result<(usize, Self::Item)> {
        let length = reader.try_get_u32()? as usize;
        if reader.len() < length {
            return Err(ProtocolError::Length);
        }
        let result = reader.split_to(length);
        Ok((4 + length, result))
    }
}

impl<T> Decode for Vec<T>
where
    T: Decode<Item = T> + std::fmt::Debug,
{
    type Item = Vec<T>;
    fn decode_with_size(reader: &mut BytesMut) -> Result<(usize, Self::Item)> {
        let (length, mut bytes) = BytesMut::decode_with_size(reader)?;
        let mut vec = Vec::new();
        let mut result = T::decode(&mut bytes);
        let values: Result<()> = loop {
            match result {
                Ok(value) => vec.push(value),
                Err(ProtocolError::NotEnoughBytes(_)) => break Ok(()),
                Err(err) => break Err(err),
            }
            result = T::decode(&mut bytes);
        };
        values.map(|_| (length, vec))
    }
}
