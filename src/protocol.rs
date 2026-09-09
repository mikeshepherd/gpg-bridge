mod common;
mod error;
mod key_data;
mod request;
mod response;
use base64::{Engine as _, engine::general_purpose};
use error::*;
use response::{Identities, Signature};
use std::marker::PhantomData;
use tokio_util::bytes::{Buf, BytesMut};

use crate::protocol::request::{ExtensionRequest, SignatureRequest};

pub trait Decode: Sized {
    type Item;
    fn decode(reader: &mut BytesMut) -> ProtocolResult<Self::Item> {
        Self::decode_with_size(reader).map(|v| v.1)
    }
    fn decode_with_size(reader: &mut BytesMut) -> ProtocolResult<(usize, Self::Item)>;
}

pub trait DecodeBlob: Sized {
    type Item;
    fn decode(reader: &mut BytesMut) -> ProtocolResult<Self::Item> {
        let mut bytes = BytesMut::decode(reader)?;
        Self::decode_blob(&mut bytes)
    }
    fn decode_blob(reader: &mut BytesMut) -> ProtocolResult<Self::Item>;
}

#[derive(Debug)]
pub struct Decoder<Output>(PhantomData<Output>)
where
    Output: Decode;

impl<Output> Default for Decoder<Output>
where
    Output: Decode,
{
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<Output> Decode for Decoder<Output>
where
    Output: Decode<Item = Output>,
{
    type Item = Output::Item;
    fn decode_with_size(reader: &mut BytesMut) -> ProtocolResult<(usize, Self::Item)> {
        Self::Item::decode_with_size(reader)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Base64(String);

/// SSH agent protocol response messages.
///
/// These message types are sent to a client *from* an agent (in response to a [`Request`](super::Request) message).
///
/// Described in [draft-miller-ssh-agent-14 § 3](https://www.ietf.org/archive/id/draft-miller-ssh-agent-14.html#section-3).
#[derive(Clone, PartialEq, Debug)]
pub enum Response {
    Failure,
    Success,
    IdentitiesAnswer(Identities),
    SignResponse(Signature),
    Other(BytesMut),
}

impl Decode for Response {
    type Item = Response;
    fn decode_with_size(reader: &mut BytesMut) -> ProtocolResult<(usize, Self::Item)> {
        if reader.len() < size_of::<u32>() {
            return Err(ProtocolError::Length);
        }

        let length = reader.get_u32() as usize;

        if reader.len() < length {
            return Err(ProtocolError::Length);
        }
        let message_type = reader.get_u8();

        match message_type {
            5 => Ok((0, Self::Failure)),
            6 => Ok((0, Self::Success)),
            12 => Identities::decode_with_size(reader)
                .map(|(size, answer)| (size, Self::IdentitiesAnswer(answer))),
            14 => Signature::decode(reader).map(|answer| (0, Self::SignResponse(answer))),
            _ => Ok((0, Self::Other(reader.clone()))),
        }
    }
}

/// SSH agent protocol response messages.
///
/// These message types are sent to a client *from* an agent (in response to a [`Request`](super::Request) message).
///
/// Described in [draft-miller-ssh-agent-14 § 3](https://www.ietf.org/archive/id/draft-miller-ssh-agent-14.html#section-3).
#[derive(Clone, PartialEq, Debug)]
pub enum Request {
    SignRequest(SignatureRequest),
    RequestIdentities(BytesMut),
    Extension(ExtensionRequest),
    Other(BytesMut),
}

impl Decode for Request {
    type Item = Request;
    fn decode_with_size(reader: &mut BytesMut) -> ProtocolResult<(usize, Self::Item)> {
        if reader.len() < size_of::<u32>() {
            return Err(ProtocolError::Length);
        }

        let length = reader.get_u32() as usize;

        if reader.len() < length {
            return Err(ProtocolError::Length);
        }
        let message_type = reader.get_u8();

        match message_type {
            11 => Ok((0, Self::RequestIdentities(reader.clone()))),

            13 => SignatureRequest::decode_with_size(reader)
                .map(|(size, answer)| (size, Self::SignRequest(answer))),
            27 => ExtensionRequest::decode_with_size(reader)
                .map(|(size, answer)| (size, Self::Extension(answer))),
            _ => Ok((0, Self::Other(reader.clone()))),
        }
    }
}

impl Decode for Base64 {
    type Item = Base64;
    fn decode_with_size(reader: &mut BytesMut) -> ProtocolResult<(usize, Self::Item)> {
        let (length, bytes) = BytesMut::decode_with_size(reader)?;
        let b64 = general_purpose::STANDARD.encode(bytes);
        Ok((length, Base64(b64)))
    }
}
