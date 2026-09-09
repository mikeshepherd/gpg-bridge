use crate::protocol::{Decode, DecodeBlob, ProtocolResult as Result, key_data::KeyData};
use decoder_macros::Decoder;
use tokio_util::bytes::{Buf, BytesMut};

#[derive(Clone, PartialEq, Debug)]
pub struct Identities(Vec<Identity>);

#[derive(Clone, PartialEq, Debug, Decoder)]
pub struct Identity {
    key_data: KeyData,
    comment: String,
}

impl Decode for Identities {
    type Item = Identities;
    fn decode_with_size(reader: &mut BytesMut) -> Result<(usize, Self::Item)> {
        let count = reader.get_u32() as usize;
        let result = vec![0; count];
        let identities = result
            .iter()
            .map(|_| Identity::decode(reader))
            .collect::<Result<Vec<Identity>>>()?;
        Ok((0, Identities(identities)))
    }
}
