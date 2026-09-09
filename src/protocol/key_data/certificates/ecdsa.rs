use crate::protocol::{Base64, Decode, DecodeBlob};
use decoder_macros::{BlobDecoder, Decoder};
use tokio_util::bytes::BytesMut;

#[derive(Clone, PartialEq, Debug, BlobDecoder)]
pub struct SignatureKey {
    pub name: String,
    pub identifier: String,
    pub blob: Base64,
}

#[derive(Clone, PartialEq, Debug, BlobDecoder)]
pub struct Signature {
    pub name: String,
    pub blob: Base64,
}

#[derive(Clone, PartialEq, Debug, Decoder)]
pub struct ECDSACertificate {
    pub nonce: Base64,
    pub ecdsa_curve_name: String,
    pub q: Base64,
    pub serial_number: u64,
    pub certificate_role: u32,
    pub identifier: String,
    pub principals: Vec<String>,
    pub valid_after: u64,
    pub valid_before: u64,
    pub critical_options: Vec<String>,
    pub extensions: Vec<String>,
    pub reserved: String,
    pub signature_key: SignatureKey,
    pub signature: Signature,
}
