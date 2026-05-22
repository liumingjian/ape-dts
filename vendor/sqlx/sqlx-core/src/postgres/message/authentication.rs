use std::str::from_utf8;

use bytes::{Buf, Bytes};
use memchr::memchr;

use crate::error::Error;
use crate::io::Decode;

// On startup, the server sends an appropriate authentication request message,
// to which the frontend must reply with an appropriate authentication
// response message (such as a password).

// For all authentication methods except GSSAPI, SSPI and SASL, there is at
// most one request and one response. In some methods, no response at all is
// needed from the frontend, and so no authentication request occurs.

// For GSSAPI, SSPI and SASL, multiple exchanges of packets may
// be needed to complete the authentication.

// <https://www.postgresql.org/docs/devel/protocol-flow.html#id-1.10.5.7.3>
// <https://www.postgresql.org/docs/devel/protocol-message-formats.html>

#[derive(Debug)]
pub enum Authentication {
    /// The authentication exchange is successfully completed.
    Ok,

    /// The frontend must now send a [PasswordMessage] containing the
    /// password in clear-text form.
    CleartextPassword,

    /// The frontend must now send a [PasswordMessage] containing the
    /// password (with user name) encrypted via MD5, then encrypted
    /// again using the 4-byte random salt.
    Md5Password(AuthenticationMd5Password),

    /// The frontend must now initiate a SASL negotiation,
    /// using one of the SASL mechanisms listed in the message.
    ///
    /// The frontend will send a [SaslInitialResponse] with the name
    /// of the selected mechanism, and the first part of the SASL
    /// data stream in response to this.
    ///
    /// If further messages are needed, the server will
    /// respond with [Authentication::SaslContinue].
    Sasl(AuthenticationSasl),

    /// This message contains challenge data from the previous step of SASL negotiation.
    ///
    /// The frontend must respond with a [SaslResponse] message.
    SaslContinue(AuthenticationSaslContinue),

    /// SASL authentication has completed with additional mechanism-specific
    /// data for the client.
    ///
    /// The server will next send [Authentication::Ok] to
    /// indicate successful authentication.
    SaslFinal(AuthenticationSaslFinal),

    GaussDbSha256(AuthenticationGaussDbSha256),

    GaussDbMd5Sha256(AuthenticationGaussDbMd5Sha256),
}

impl Decode<'_> for Authentication {
    fn decode_with(mut buf: Bytes, _: ()) -> Result<Self, Error> {
        Ok(match buf.get_u32() {
            0 => Authentication::Ok,

            3 => Authentication::CleartextPassword,

            5 => {
                let mut salt = [0; 4];
                buf.copy_to_slice(&mut salt);

                Authentication::Md5Password(AuthenticationMd5Password { salt })
            }

            10 => {
                if buf.starts_with(b"SCRAM-SHA-256") {
                    Authentication::Sasl(AuthenticationSasl(buf))
                } else {
                    Authentication::GaussDbSha256(AuthenticationGaussDbSha256(buf))
                }
            }
            11 => {
                if buf.len() == 68 {
                    Authentication::GaussDbMd5Sha256(AuthenticationGaussDbMd5Sha256(buf))
                } else {
                    Authentication::SaslContinue(AuthenticationSaslContinue::decode(buf)?)
                }
            }
            12 => Authentication::SaslFinal(AuthenticationSaslFinal::decode(buf)?),

            ty => {
                return Err(err_protocol!("unknown authentication method: {}", ty));
            }
        })
    }
}

/// Body of [Authentication::Md5Password].
#[derive(Debug)]
pub struct AuthenticationMd5Password {
    pub salt: [u8; 4],
}

/// Body of [Authentication::Sasl].
#[derive(Debug)]
pub struct AuthenticationSasl(Bytes);

impl AuthenticationSasl {
    #[inline]
    pub fn mechanisms(&self) -> SaslMechanisms<'_> {
        SaslMechanisms(&self.0)
    }
}

/// An iterator over the SASL authentication mechanisms provided by the server.
pub struct SaslMechanisms<'a>(&'a [u8]);

impl<'a> Iterator for SaslMechanisms<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.0.is_empty() && self.0[0] == b'\0' {
            return None;
        }

        let mechanism = memchr(b'\0', self.0).and_then(|nul| from_utf8(&self.0[..nul]).ok())?;

        self.0 = &self.0[(mechanism.len() + 1)..];

        Some(mechanism)
    }
}

#[derive(Debug)]
pub struct AuthenticationSaslContinue {
    pub salt: Vec<u8>,
    pub iterations: u32,
    pub nonce: String,
    pub message: String,
}

impl Decode<'_> for AuthenticationSaslContinue {
    fn decode_with(buf: Bytes, _: ()) -> Result<Self, Error> {
        let mut iterations: u32 = 4096;
        let mut salt = Vec::new();
        let mut nonce = Bytes::new();

        // [Example]
        // r=/z+giZiTxAH7r8sNAeHr7cvpqV3uo7G/bJBIJO3pjVM7t3ng,s=4UV68bIkC8f9/X8xH7aPhg==,i=4096

        for item in buf.split(|b| *b == b',') {
            let key = item[0];
            let value = &item[2..];

            match key {
                b'r' => {
                    nonce = buf.slice_ref(value);
                }

                b'i' => {
                    iterations = atoi::atoi(value).unwrap_or(4096);
                }

                b's' => {
                    salt = base64::decode(value).map_err(Error::protocol)?;
                }

                _ => {}
            }
        }

        Ok(Self {
            iterations,
            salt,
            nonce: from_utf8(&*nonce).map_err(Error::protocol)?.to_owned(),
            message: from_utf8(&*buf).map_err(Error::protocol)?.to_owned(),
        })
    }
}

#[derive(Debug)]
pub struct AuthenticationSaslFinal {
    pub verifier: Vec<u8>,
}

impl Decode<'_> for AuthenticationSaslFinal {
    fn decode_with(buf: Bytes, _: ()) -> Result<Self, Error> {
        let mut verifier = Vec::new();

        for item in buf.split(|b| *b == b',') {
            let key = item[0];
            let value = &item[2..];

            if let b'v' = key {
                verifier = base64::decode(value).map_err(Error::protocol)?;
            }
        }

        Ok(Self { verifier })
    }
}

#[derive(Debug)]
pub struct AuthenticationGaussDbSha256(Bytes);

impl AuthenticationGaussDbSha256 {
    pub fn storage_method(&self) -> Result<u32, Error> {
        if self.0.len() < 4 {
            return Err(err_protocol!("invalid gaussdb sha256 auth body length"));
        }
        Ok((&self.0[..4]).get_u32())
    }

    pub fn random64(&self) -> Result<&str, Error> {
        read_fixed_str(&self.0, 4, 64)
    }

    pub fn token8(&self) -> Result<&str, Error> {
        read_fixed_str(&self.0, 68, 8)
    }

    pub fn iterations(&self) -> Option<u32> {
        if self.0.len() < 80 {
            return None;
        }
        Some((&self.0[76..80]).get_u32())
    }
}

#[derive(Debug)]
pub struct AuthenticationGaussDbMd5Sha256(Bytes);

impl AuthenticationGaussDbMd5Sha256 {
    pub fn random64(&self) -> Result<&str, Error> {
        read_fixed_str(&self.0, 0, 64)
    }

    pub fn salt(&self) -> Result<[u8; 4], Error> {
        if self.0.len() < 68 {
            return Err(err_protocol!("invalid gaussdb md5-sha256 auth body length"));
        }
        let mut salt = [0_u8; 4];
        salt.copy_from_slice(&self.0[64..68]);
        Ok(salt)
    }
}

fn read_fixed_str(storage: &[u8], offset: usize, len: usize) -> Result<&str, Error> {
    if storage.len() < offset + len {
        return Err(err_protocol!("invalid gaussdb auth body length"));
    }
    let end = storage[offset..offset + len]
        .iter()
        .position(|b| *b == 0)
        .map(|pos| offset + pos)
        .unwrap_or(offset + len);
    from_utf8(&storage[offset..end]).map_err(Error::protocol)
}
