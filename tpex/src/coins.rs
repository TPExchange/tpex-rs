use std::{fmt::Display, str::FromStr};
use num_format::{Locale, ToFormattedString};

use crate::{Error, Result};

#[must_use]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Coins {
    milli: u64
}
impl Coins {
    pub const fn from_millicoins(milli: u64) -> Coins {
        Coins{milli}
    }
    pub const fn from_coins(coins: u32) -> Coins {
        Coins{milli: coins as u64 * 1000 }
    }
    // pub fn from_diamonds(diamonds: u64) -> Result<Coins> {
    //     diamonds.checked_mul(1_000_000)
    //     .map(Coins::from_millicoins)
    //     .ok_or(Error::Overflow)
    // }
    pub const fn is_zero(&self) -> bool { self.milli == 0 }

    pub fn checked_add(&self, other: Coins) -> Result<Coins> {
        self.milli.checked_add(other.milli).ok_or(Error::Overflow).map(Coins::from_millicoins)
    }
    pub fn checked_sub(&self, other: Coins) -> Result<Coins> {
        self.milli.checked_sub(other.milli).ok_or(Error::Overflow).map(Coins::from_millicoins)
    }
    pub fn checked_mul(&self, other: u64) -> Result<Coins> {
        self.milli.checked_mul(other).ok_or(Error::Overflow).map(Coins::from_millicoins)
    }
    pub fn checked_add_assign(&mut self, other: Coins) -> Result<()> {
        self.checked_add(other).map(|x| self.milli = x.milli)
    }
    pub fn checked_sub_assign(&mut self, other: Coins) -> Result<()> {
        self.checked_sub(other).map(|x| self.milli = x.milli)
    }
    pub fn checked_mul_assign(&mut self, other: u64) -> Result<()> {
        self.checked_mul(other).map(|x| self.milli = x.milli)
    }
    pub const fn millicoins(&self) -> u64 {
        self.milli
    }
    pub const fn fee_ppm(&self, ppm: u64) -> Result<Coins> {
        let mut fee: u128 = self.milli as u128;
        // Will not overflow
        fee *= ppm as u128;
        fee = fee.div_ceil(1_000_000);
        if fee >= u64::MAX as u128 {
            return Err(Error::Overflow);
        }
        Ok(Coins::from_millicoins(fee as u64))
    }
}
// impl Add for Coins {
//     type Output = Coins;

//     fn add(self, rhs: Self) -> Self::Output {
//         self.checked_add(rhs).expect("Coin add overflow")
//     }
// }
// impl Sub for Coins {
//     type Output = Coins;

//     fn add(self, rhs: Self) -> Self::Output {
//         self.checked_add(rhs).expect("Coin add overflow")
//     }
// }
impl Display for Coins {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let whole = self.milli / 1000;
        let frac = self.milli % 1000;
        write!(f, "{}", whole.to_formatted_string(&Locale::en))?;
        if frac == 0 {
            write!(f, "c")
        }
        else if frac % 100 == 0 {
            write!(f, ".{}c", frac/100)
        }
        else if frac % 10 == 0 {
            write!(f, ".{:02}c", frac/10)
        }
        else {
            write!(f, ".{:03}c", frac)
        }
    }
}
impl FromStr for Coins {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        // Trim trailing c
        let s = s.strip_suffix('c').unwrap_or(s);
        let s = s.strip_suffix('C').unwrap_or(s);
        let s = s.replace(",", "");
        let Some((whole, frac)) = s.split_once('.')
        else {
            return
                u64::from_str(&s).ok()
                .map(|x| x * 1000)
                .map(Coins::from_millicoins)
                .ok_or(Error::CoinStringMangled);
        };
        let frac_millis = match frac.len() {
            0 => 0,
            1 => u64::from_str(frac).map_err(|_| Error::CoinStringMangled)? * 100,
            2 => u64::from_str(frac).map_err(|_| Error::CoinStringMangled)? * 10,
            3 => u64::from_str(frac).map_err(|_| Error::CoinStringMangled)?,
            _ => return Err(Error::CoinStringTooPrecise)
        };

        u64::from_str(whole).ok()
        .and_then(|whole| whole.checked_mul(1000))
        .and_then(|whole_millis| whole_millis.checked_add(frac_millis))
        .map(Coins::from_millicoins)
        .ok_or(Error::CoinStringMangled)

    }
}
impl serde::Serialize for Coins {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer {
            serializer.serialize_str(&self.to_string())
    }
}
impl<'de> serde::Deserialize<'de> for Coins {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de> {
            struct Visitor;
            impl serde::de::Visitor<'_> for Visitor {
                type Value = Coins;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    write!(formatter, "a Coins value string")
                }

                fn visit_str<E>(self, v: &str) -> std::prelude::v1::Result<Self::Value, E>
                    where
                        E: serde::de::Error, {
                            Coins::from_str(v).map_err(E::custom)
                }
            }
            deserializer.deserialize_str(Visitor)
    }
}
