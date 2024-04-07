use std::{fmt::Display, str::FromStr};

use tpex::PlayerId;
use base64::prelude::*;

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
#[derive(num_derive::FromPrimitive)]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum TokenLevel {
    /// The client can only get general pricing data
    ReadOnly = 0,
    /// The client can act on behalf of a user, but not for banker commands
    ProxyOne = 1,
    /// The client can act on behalf of any user, and perform admin commands
    ProxyAll = 2,
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Token(pub [u8;16]);
impl Token {
    #[cfg(feature = "bin")]
    pub fn generate() -> Token {
        let mut ret = Token(Default::default());
        getrandom::getrandom(&mut ret.0).expect("Could not generate token");
        ret
    }
}

impl FromStr for Token {
    type Err = base64::DecodeSliceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut ret = Token(Default::default());
        let len = BASE64_STANDARD_NO_PAD.decode_slice(s, &mut ret.0)?;
        if len != ret.0.len() {
            // FIXME: better error here
            Err(base64::DecodeSliceError::OutputSliceTooSmall)
        }
        else {
            Ok(ret)
        }
    }
}
impl Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", BASE64_STANDARD_NO_PAD.encode(self.0))
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TokenPostArgs {
    pub level: TokenLevel,
    pub user: PlayerId
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct TokenDeleteArgs {
    pub token: Option<Token>
}
