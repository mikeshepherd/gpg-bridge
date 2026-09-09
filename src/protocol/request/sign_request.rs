use crate::protocol::{Base64, Decode, DecodeBlob, ProtocolResult as Result, key_data::KeyData};
use tokio_util::bytes::{Bytes, BytesMut};

#[derive(Clone, PartialEq, Debug)]
enum Flags {
    None,
}

impl Decode for Flags {
    type Item = Flags;

    fn decode_with_size(reader: &mut BytesMut) -> Result<(usize, Self::Item)> {
        let (size, flags) = u32::decode_with_size(reader)?;
        println!("FLAGS {}", flags);
        Ok((size, Flags::None))
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct RequestData {
    preamble: Result<Base64>,
    user_name: Result<String>,
    service_name: Result<String>,
    publickey: Result<String>,
    has_signature: Result<bool>,
    algorithm: Result<String>,
    key: Result<KeyData>,
    signature: Result<RequestSignature>,
}

impl DecodeBlob for RequestData {
    type Item = RequestData;

    fn decode_blob(reader: &mut BytesMut) -> Result<Self::Item> {
        let preamble = Base64::decode(reader);
        let _ = u8::decode(reader);
        let user_name = String::decode(reader);
        let service_name = String::decode(reader);
        let publickey = String::decode(reader);
        let has_signature = bool::decode(reader);
        let algorithm = String::decode(reader);
        let key = KeyData::decode(reader);
        let signature = RequestSignature::decode(reader);
        Ok(RequestData {
            preamble,
            user_name,
            service_name,
            publickey,
            has_signature,
            algorithm,
            key,
            signature,
        })
    }
}

// b"\0\0\0 &jQ~\\`<S#|zO<ar5Asp0U%m%eDGU&SO[\0\0\0 \xc2\xde\xd4q\xf7\xd0\x15\xdd\xf1&\xcd\xed7\r>\xecM\xd7\x9f\x80\xaaF0B\0\xea\xd7\xd3\x95\xefh\x0cG\xe0\x1aXk\xfd\x0cB\0\0\0\x02\0\0\0'codeServerVM.internal.mpssoftware.co.uk\0\0\0v\0\0\0\x0ccodeServerVM\0\0\0'codeServerVM.internal.mpssoftware.co.uk\0\0\0\x0ccodeservervm\0\0\0'codeservervm.internal.mpssoftware.co.uk\0\0\0\0i\xd4\xdf\x82\0\0\0\0i\xfcl\xbe\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0h\0\0\0\x13ecdsa-sha2-nistp256\0\0\0\x08nistp256\0\0\0A\x04\xef\xa0\xfd\xa5\x9f\x0bp\x8d\x14\xf1\xc1\xe4w\x9d\xa9\x11\xb7X\xda\xc3\xb8\x9f\xde\xady\xd8X\xbd+t\xdb\xee\xce\xd7\xc2\x7f$\xc5\xee\x0c\xf9\xab\xbb\xeb\xb12MJ\xb0\xdc\x123\xc5\\\xf2\xe0\xf2c\xd5A\xe3E\x19^\0\0\0d\0\0\0\x13ecdsa-sha2-nistp256\0\0\0I\0\0\0!\0\x9b\x90M\xa9\xe1\xd9W\xacP\xe5\xb0\xfa\x99\r\x91\x8f\x8e\x83\xd4\xd6\x06\xc1\xc4>\xe3,\x9f\x9f\x90Y\x06\x17\0\0\0 +:\\\xaf2?\x1frB\xf2R\x11\x8c=\xf5\x1aT\x15\x80Y\xfeiJF.s\x0c\xa8\xc0\xe1\xc5\xcd"

// b"G\xe0\x1aXk\xfd\x0cB\0\0\0\x02\0\0\0'codeServerVM.internal.mpssoftware.co.uk\0\0\0v\0\0\0\x0ccodeServerVM\0\0\0'codeServerVM.internal.mpssoftware.co.uk\0\0\0\x0ccodeservervm\0\0\0'codeservervm.internal.mpssoftware.co.uk\0\0\0\0i\xd4\xdf\x82\0\0\0\0i\xfcl\xbe\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0h\0\0\0\x13ecdsa-sha2-nistp256\0\0\0\x08nistp256\0\0\0A\x04\xef\xa0\xfd\xa5\x9f\x0bp\x8d\x14\xf1\xc1\xe4w\x9d\xa9\x11\xb7X\xda\xc3\xb8\x9f\xde\xady\xd8X\xbd+t\xdb\xee\xce\xd7\xc2\x7f$\xc5\xee\x0c\xf9\xab\xbb\xeb\xb12MJ\xb0\xdc\x123\xc5\\\xf2\xe0\xf2c\xd5A\xe3E\x19^\0\0\0d\0\0\0\x13ecdsa-sha2-nistp256\0\0\0I\0\0\0!\0\x9b\x90M\xa9\xe1\xd9W\xacP\xe5\xb0\xfa\x99\r\x91\x8f\x8e\x83\xd4\xd6\x06\xc1\xc4>\xe3,\x9f\x9f\x90Y\x06\x17\0\0\0 +:\\\xaf2?\x1frB\xf2R\x11\x8c=\xf5\x1aT\x15\x80Y\xfeiJF.s\x0c\xa8\xc0\xe1\xc5\xcd"

#[derive(Clone, PartialEq, Debug)]
pub struct RequestSignature {
    name: Result<String>,
    // blob1: Result<BytesMut>,
    // blob2: Result<BytesMut>,
    principals: Result<Vec<String>>,
}

impl DecodeBlob for RequestSignature {
    type Item = RequestSignature;

    fn decode_blob(reader: &mut BytesMut) -> Result<Self::Item> {
        // use base64::{Engine as _, engine::general_purpose};
        // let clone = reader.clone();
        // let b64 = general_purpose::STANDARD.encode(clone);
        // println!("{:?}", b64);
        let name = String::decode(reader);
        // let blob1 = BytesMut::decode(reader);
        // let blob2 = BytesMut::decode(reader);
        println!("remaining {:?}", reader.clone());
        let principals = Vec::<String>::decode(reader);
        Ok(RequestSignature {
            name,
            // blob1,
            // blob2,
            principals,
        })
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct SignatureRequest {
    key: Result<KeyData>,
    data: Result<RequestData>,
    flags: Result<Flags>,
}

impl Decode for SignatureRequest {
    type Item = SignatureRequest;

    fn decode_with_size(reader: &mut BytesMut) -> Result<(usize, Self::Item)> {
        let key = KeyData::decode(reader);
        let data = RequestData::decode(reader);
        let flags = Flags::decode(reader);

        Ok((0, SignatureRequest { key, data, flags }))
    }
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;
    use crate::protocol::error::ProtocolError;
    use crate::protocol::key_data::certificates::ECDSACertificate;
    use crate::protocol::key_data::certificates::Signature as CertSignature;
    use crate::protocol::key_data::certificates::SignatureKey;

    #[test]
    fn test_add() {
        use base64::{Engine as _, engine::general_purpose};
        let test_data = "AAAAIJ7COJ3uAvsv7FnAXsyvo+Cp05FSuB9IZOgEQa7k2DL4MgAAAARtaWtlAAAADnNzaC1jb25uZWN0aW9uAAAAI3B1YmxpY2tleS1ob3N0Ym91bmQtdjAwQG9wZW5zc2guY29tAQAAAChlY2RzYS1zaGEyLW5pc3RwMjU2LWNlcnQtdjAxQG9wZW5zc2guY29tAAACNAAAAChlY2RzYS1zaGEyLW5pc3RwMjU2LWNlcnQtdjAxQG9wZW5zc2guY29tAAAAIAB4tjKl3cwftuTui0COzI3+k9H5HP8JsIWmU20lu8ykAAAACG5pc3RwMjU2AAAAQQQGfbXK94Ec0+5Y4SvsvV7wsDTpOrI+8jCSdbiGVGUSPJInUmz/XVOeB8krNNNZGL064hamTmkIZE3DUiuCf5T+AAAAAAAAAAAAAAABAAAABG1pa2UAAAAIAAAABG1pa2UAAAAAaaRHNwAAAABrl++3AAAAAAAAAIIAAAAVcGVybWl0LVgxMS1mb3J3YXJkaW5nAAAAAAAAABdwZXJtaXQtYWdlbnQtZm9yd2FyZGluZwAAAAAAAAAWcGVybWl0LXBvcnQtZm9yd2FyZGluZwAAAAAAAAAKcGVybWl0LXB0eQAAAAAAAAAOcGVybWl0LXVzZXItcmMAAAAAAAAAAAAAAGgAAAATZWNkc2Etc2hhMi1uaXN0cDI1NgAAAAhuaXN0cDI1NgAAAEEEk12QHK9pkCfglqSZr/rUZHioaC41ZXxqMahe+ytawJz20dLauvMvOyfMMTrtDuDiC459MTxLX0HEtdW8+2MzpQAAAGUAAAATZWNkc2Etc2hhMi1uaXN0cDI1NgAAAEoAAAAhAMJd8DUxGB+x1I/vPG1smikSHhjCkH5AULJ80BYsZsgxAAAAIQCcB6wUiZ4s12g5z74I8FSobGETnkNf69qzZuKK1+YdRwAAAg0AAAAgc3NoLWVkMjU1MTktY2VydC12MDFAb3BlbnNzaC5jb20AAAAgJmpRflxgPFMjfHpPPGFyNUFzcDBVJW0lZURHVSZTT1sAAAAgwt7UcffQFd3xJs3tNw0+7E3Xn4CqRjBCAOrX05XvaAxH4BpYa/0MQgAAAAIAAAAnY29kZVNlcnZlclZNLmludGVybmFsLm1wc3NvZnR3YXJlLmNvLnVrAAAAdgAAAAxjb2RlU2VydmVyVk0AAAAnY29kZVNlcnZlclZNLmludGVybmFsLm1wc3NvZnR3YXJlLmNvLnVrAAAADGNvZGVzZXJ2ZXJ2bQAAACdjb2Rlc2VydmVydm0uaW50ZXJuYWwubXBzc29mdHdhcmUuY28udWsAAAAAadTfggAAAABp/Gy+AAAAAAAAAAAAAAAAAAAAaAAAABNlY2RzYS1zaGEyLW5pc3RwMjU2AAAACG5pc3RwMjU2AAAAQQTvoP2lnwtwjRTxweR3nakRt1jaw7if3q152Fi9K3Tb7s7Xwn8kxe4M+au767EyTUqw3BIzxVzy4PJj1UHjRRleAAAAZAAAABNlY2RzYS1zaGEyLW5pc3RwMjU2AAAASQAAACEAm5BNqeHZV6xQ5bD6mQ2Rj46D1NYGwcQ+4yyfn5BZBhcAAAAgKzpcrzI/H3JC8lIRjD31GlQVgFn+aUpGLnMMqMDhxc0=";

        let bytes = general_purpose::STANDARD.decode(test_data).unwrap();
        let mut bytes_mut = BytesMut::new();
        bytes_mut.extend_from_slice(bytes.as_slice());

        let decoded = RequestData::decode_blob(&mut bytes_mut).unwrap();

        assert_eq!(
            RequestData {
                preamble: Ok(Base64(
                    "nsI4ne4C+y/sWcBezK+j4KnTkVK4H0hk6ARBruTYMvg=".to_string()
                )),
                user_name: Ok("mike".to_string()),
                service_name: Ok("ssh-connection".to_string()),
                publickey: Ok("publickey-hostbound-v00@openssh.com".to_string()),
                has_signature: Ok(true),
                algorithm: Ok("ecdsa-sha2-nistp256-cert-v01@openssh.com".to_string()),
                key: Ok(KeyData::EcdsaCert(ECDSACertificate {
                    nonce: Base64("AHi2MqXdzB+25O6LQI7Mjf6T0fkc/wmwhaZTbSW7zKQ=".to_string()),
                    ecdsa_curve_name: "nistp256".to_string(),
                    q: Base64(
                        "BAZ9tcr3gRzT7ljhK+y9XvCwNOk6sj7yMJJ1uIZUZRI8kidSbP9dU54HySs001kYvTriFqZOaQhkTcNSK4J/lP4=".to_string()
                    ),
                    serial_number: 0,
                    certificate_role: 1,
                    identifier: "mike".to_string(),
                    principals: ["mike".to_string()].to_vec(),
                    valid_after: 1772373815,
                    valid_before: 1805119415,
                    critical_options: [].to_vec(),
                    extensions: [
                        "permit-X11-forwarding".to_string(),
                        "".to_string(),
                        "permit-agent-forwarding".to_string(),
                        "".to_string(),
                        "permit-port-forwarding".to_string(),
                        "".to_string(),
                        "permit-pty".to_string(),
                        "".to_string(),
                        "permit-user-rc".to_string(),
                        "".to_string()
                        ].to_vec(),
                    reserved: "".to_string(),
                    signature_key: SignatureKey {
                        name: "ecdsa-sha2-nistp256".to_string(),
                        identifier: "nistp256".to_string(),
                        blob: Base64(
                            "BJNdkByvaZAn4Jakma/61GR4qGguNWV8ajGoXvsrWsCc9tHS2rrzLzsnzDE67Q7g4guOfTE8S19BxLXVvPtjM6U=".to_string()
                        )
                    },
                    signature: CertSignature {
                        name: "ecdsa-sha2-nistp256".to_string(),
                        blob: Base64(
                            "AAAAIQDCXfA1MRgfsdSP7zxtbJopEh4YwpB+QFCyfNAWLGbIMQAAACEAnAesFImeLNdoOc++CPBUqGxhE55DX+vas2biitfmHUc=".to_string()
                        )
                    }
                })),
                signature: Err(ProtocolError::Length),
            },
            decoded
        );
    }
}
