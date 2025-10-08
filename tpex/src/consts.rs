use crate::{BankRates, Coins};

pub const SHARED_ACCOUNT_DELIM: char = '.';
pub const ETP_DELIM : char = ':';
pub const DIAMOND_RAW_COINS: Coins = Coins::from_coins(1000);

pub const INITIAL_BANK_RATES: BankRates = BankRates {
    buy_order_ppm:      0_0000,
    sell_order_ppm:     0_0000,
    coins_sell_ppm:    5_0000,
    coins_buy_ppm:   5_0000,
};

/// Checks whether `x` is a safe name (i.e. free from annoying bs that could hack us)
///
/// This will reject a lot of valid IDs, so it should only be used for something that cannot be decomposed further
/// (i.e. for parts of an SharedId, not for a PlayerId)
pub fn is_safe_name(x: impl AsRef<str>) -> bool {
    let x = x.as_ref();
    if x.is_empty() { return false; }
    x.chars().all(|i| i.is_ascii_alphanumeric() || i == '_' || i == '-')
}
