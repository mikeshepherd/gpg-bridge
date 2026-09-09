pub mod certificates;
use certificates::*;

use crate::protocol::{Decode, DecodeBlob, ProtocolResult as Result};
use tokio_util::bytes::BytesMut;

#[derive(Clone, PartialEq, Debug)]
pub enum KeyData {
    EcdsaCert(ECDSACertificate),
    Other { algorithm: String, blob: BytesMut },
}

impl DecodeBlob for KeyData {
    type Item = KeyData;
    fn decode_blob(reader: &mut BytesMut) -> Result<Self::Item> {
        let algorithm = String::decode(reader)?;
        match algorithm {
            s if s.starts_with("ecdsa-sha2-") => {
                Ok(KeyData::EcdsaCert(ECDSACertificate::decode(reader)?))
            }
            s => Ok(KeyData::Other {
                algorithm: s,
                blob: reader.clone(),
            }),
        }
    }
}
