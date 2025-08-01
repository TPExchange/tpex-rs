use std::{fmt::Display, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{AssetId, SharedId};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ETPId {
    issuer: SharedId,
    name: String,
}

impl ETPId {
    /// Creates an ETPId from the given parameters, ensuring that the name is valid
    pub fn try_new(issuer: SharedId, name: String) -> Result<ETPId, (SharedId, String)> {
        if name.contains('%') {
            return Err((issuer, name))
        }
        Ok(ETPId { issuer, name })
    }

    /// The issuer of this ETP
    pub fn issuer(&self) -> &SharedId {
        &self.issuer
    }

    /// The name of this ETP
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Takes the owned (issue, name) tuple
    pub fn take(self) -> (SharedId, String) {
        (self.issuer, self.name)
    }
}
impl From<&ETPId> for AssetId {
    fn from(value: &ETPId) -> Self {
        format!("{value}")
    }
}
impl TryFrom<AssetId> for ETPId {
    type Error = AssetId;

    fn try_from(value: AssetId) -> std::result::Result<Self, Self::Error> {
        value.parse().map_err(|_| value)
    }
}
impl FromStr for ETPId {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (issuer, name) = s.split_once('%').ok_or(crate::Error::InvalidETPId)?;
        let issuer = SharedId::from_str(issuer).map_err(|_| crate::Error::InvalidETPId)?;
        Ok(Self {
            name: name.to_owned(),
            issuer
        })
    }
}
impl Display for ETPId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}%{}", self.issuer, self.name)
    }
}
impl Serialize for ETPId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        AssetId::from(self).serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for ETPId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let asset_id = AssetId::deserialize(deserializer)?;
        Self::from_str(&asset_id).map_err(serde::de::Error::custom)
    }
}
