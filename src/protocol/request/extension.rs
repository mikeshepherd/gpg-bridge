use crate::protocol::Decode;
use decoder_macros::Decoder;
use tokio_util::bytes::BytesMut;

#[derive(Clone, PartialEq, Debug, Decoder)]
pub struct ExtensionRequest {
    blob: BytesMut,
}
