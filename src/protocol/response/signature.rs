use crate::protocol::{Base64, Decode, DecodeBlob};
use decoder_macros::BlobDecoder;
use tokio_util::bytes::BytesMut;

#[derive(Clone, PartialEq, Debug, BlobDecoder)]
pub struct Signature {
    name: String,
    blob: Base64,
}
