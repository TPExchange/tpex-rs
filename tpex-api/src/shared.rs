use std::{fmt::Display, str::FromStr};

use num_traits::FromPrimitive;
use serde::{de::Visitor, Deserialize, Serialize};
use tpex::PlayerId;
use base64::prelude::*;

#[repr(u8)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
#[derive(num_derive::FromPrimitive)]
pub enum TokenLevel  {
    /// The client can only get general pricing data
    ReadOnly = 0,
    /// The client can act on behalf of a user, but not for banker commands
    ProxyOne = 1,
    /// The client can act on behalf of any user, and perform admin commands
    ProxyAll = 2,
}
impl Serialize for TokenLevel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        serializer.serialize_u64(*self as u64)
    }
}
impl<'de> Deserialize<'de> for TokenLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: serde::Deserializer<'de> {
        struct Inner;
        impl Visitor<'_> for Inner {
            type Value = TokenLevel;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(formatter, "an integer TokenLevel")
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where E: serde::de::Error, {
                TokenLevel::from_u64(v).ok_or(E::invalid_value(serde::de::Unexpected::Unsigned(v), &Self))
            }
        }
        deserializer.deserialize_u64(Inner)
    }
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub struct Token(pub [u8;16]);
impl Token {
    #[cfg(feature = "server")]
    pub fn generate() -> Token {
        let mut ret = Token(Default::default());
        getrandom::fill(&mut ret.0).expect("Could not generate token");
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
impl Serialize for Token {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer {
        serializer.serialize_str(&self.to_string())
    }
}
impl<'de> Deserialize<'de> for Token {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: serde::Deserializer<'de> {
        struct Inner;
        impl Visitor<'_> for Inner {
            type Value = Token;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(formatter, "a base64-encoded token")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: serde::de::Error, {
                v.parse().map_err(E::custom)
            }
        }
        deserializer.deserialize_str(Inner)
    }
}

#[derive(PartialEq, Eq, Debug, Clone)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TokenInfo {
    pub token: Token,
    pub user: PlayerId,
    pub level: TokenLevel
}

#[derive(Debug, Clone)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TokenPostArgs {
    pub level: TokenLevel,
    pub user: PlayerId
}

#[derive(Default, Debug, Clone)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TokenDeleteArgs {
    pub token: Option<Token>
}

#[derive(Default, Debug, Clone)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct StateGetArgs {
    pub from: Option<u64>
}

#[derive(Default, Debug, Clone)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct StatePatchArgs {
    pub id: Option<u64>
}
#[derive(Default, Debug, Clone)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ErrorInfo {
    pub error: String
}
#[derive(Default, Debug, Clone)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct PollGetArgs {
    pub id: u64
}
#[derive(Default, Debug, Clone)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct InspectBalanceGetArgs {
    pub player: PlayerId
}
#[derive(Default, Debug, Clone)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct InspectAssetsGetArgs {
    pub player: PlayerId
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PriceChangeCause {
    Buy,
    Sell,
    Cancel
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PriceChange {
    pub time: chrono::DateTime<chrono::Utc>,
    pub best_buy: Option<tpex::Coins>,
    pub n_buy: u64,
    pub best_sell: Option<tpex::Coins>,
    pub n_sell: u64,
    pub cause: PriceChangeCause
}
impl PriceChange {
    pub const fn mid_market(&self) -> Option<tpex::Coins> {
        match (self.best_buy, self.best_sell) {
            (Some(best_buy), Some(best_sell)) => Some(tpex::Coins::from_millicoins(best_buy.millicoins().saturating_add(best_sell.millicoins()) / 2)),
            (None, Some(x)) |
            (Some(x), None) => Some(x),
            (None, None) => None
        }
    }
}
impl PartialEq for PriceChange {
    fn eq(&self, other: &Self) -> bool {
        self.best_buy == other.best_buy && self.n_buy == other.n_buy && self.best_sell == other.best_sell && self.n_sell == other.n_sell && self.cause == other.cause
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceHistoryArgs {
    pub asset: tpex::AssetId,
}
