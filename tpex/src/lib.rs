use std::{collections::HashSet, ops::{Add, AddAssign}, pin::pin};

use auth::AuthSync;
use balance::BalanceSync;
use order::OrderSync;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

// We use a base coins, which represent 1/1000 of a diamond
use serde::{Deserialize, Serialize};
use withdrawal::WithdrawalSync;

pub use self::{order::PendingOrder, withdrawal::PendingWithdrawal};

mod balance;
mod order;
mod withdrawal;
mod coins;
mod auth;
mod tests;

pub use order::OrderType;
pub use coins::Coins;

pub const DIAMOND_NAME: &str = "diamond";
pub const DIAMOND_RAW_COINS: Coins = Coins::from_coins(1000);

const INITIAL_BANK_RATES: BankRates = BankRates {
    investment_ppm:    25_0000,
    buy_order_ppm:      0_0000,
    sell_order_ppm:     0_0000,
    diamond_buy_ppm:    1_0000,
    diamond_sell_ppm:   1_0000,
};

#[derive(PartialEq, PartialOrd, Eq, Ord, Default, Debug, Clone, Hash)]
pub struct PlayerId(String);
impl PlayerId {
    /// Creates a player id, assuming that the given id is valid, correct, and authorized.
    pub fn assume_username_correct(s: String) -> PlayerId { PlayerId(s.to_lowercase()) }
    /// Gets the internal name of the user
    pub fn get_raw_name(&self) -> &String { &self.0 }
    pub fn the_bank() -> PlayerId { PlayerId("#tpex".to_owned()) }
}
impl<'de> Deserialize<'de> for PlayerId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de> {
        String::deserialize(deserializer).map(PlayerId::assume_username_correct)
    }
}
impl Serialize for PlayerId {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer {
        String::serialize(&self.0, serializer)
    }
}
impl core::fmt::Display for PlayerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
pub type AssetId = String;

#[derive(PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum ActionLevel {
    Normal,
    Banker
}
#[derive(PartialEq, Eq, Debug)]
pub struct ActionPermissions {
    pub level: ActionLevel,
    pub player: PlayerId
}

// #[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Default)]
// struct Conversion {
//     from: std::collections::HashMap<AssetId, u64>,
//     to: std::collections::HashMap<AssetId, u64>
// }

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Default)]
pub struct AutoConversion {
    pub from: AssetId,
    // We don't have n_from, as that would give inconsistent conversion. 1:n only!
    // pub n_from: u64,
    pub to: AssetId,
    pub n_to: u64
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum Action {
    /// Deleted transaction, for when someone does a bad
    Deleted {
        reason: String,
        banker: PlayerId
    },
    /// Player deposited assets
    Deposit {
        player: PlayerId,
        asset: AssetId,
        count: u64,
        banker: PlayerId,
    },
    /// Player asked to withdraw assets
    WithdrawalRequested {
        player: PlayerId,
        assets: std::collections::HashMap<AssetId,u64>
    },
    /// A banker has agreed to take out assets imminently
    WithdrawalCompleted {
        target: u64,
        banker: PlayerId,
    },
    /// The player got coins for giving diamonds
    BuyCoins {
        player: PlayerId,
        n_diamonds: u64,
    },
    /// The player got diamonds for giving coins
    SellCoins {
        player: PlayerId,
        n_diamonds: u64,
    },
    /// Player offers to buy assets at a price, and locks money away until cancelled
    ///
    /// Instant matches should favour the buyer
    BuyOrder {
        player: PlayerId,
        asset: AssetId,
        count: u64,
        coins_per: Coins,
    },
    /// Player offers to sell assets at a price, and locks away assets until cancelled
    ///
    /// Instant matches should favour the seller
    SellOrder {
        player: PlayerId,
        asset: AssetId,
        count: u64,
        coins_per: Coins,
    },
    /// Updates the list of assets that require prior authorisation from an admin
    UpdateRestricted {
        restricted_assets: Vec<AssetId>,
        banker: PlayerId,
    },
    /// Allows a player to place new withdrawal requests up to new_count of an item
    ///
    /// XXX: This can and will nuke existing values, so check those race conditions!
    AuthoriseRestricted {
        authorisee: PlayerId,
        banker: PlayerId,
        asset: AssetId,
        new_count: u64
    },
    /// Changes the fees
    UpdateBankRates {
        #[serde(flatten)]
        rates: BankRates,
        banker: PlayerId,
    },
    // TODO: Not sure about these yet, let's see what demand we get
    // /// A futures contract is when someone promises to pay someone for assets in the future
    // ///
    // /// In the case of a default: as much of the asset is moved into the player's account
    // Future {
    //     buyer: PlayerId,
    //     seller: Option<PlayerId>,
    //     asset: AssetId,
    //     count: i64,
    //     coins_per: u64,
    //     collateral: i64,
    //     delivery_date: DateTimeUtc
    // },
    // /// Performed when a player doesn't have the required assets for a future
    // Defaulted {

    // }
    /// A transfer of coins from one player to another, no strings attached
    TransferCoins {
        payer: PlayerId,
        payee: PlayerId,
        count: Coins
    },
    /// A transfer of items from one player to another, no strings attached
    TransferAsset {
        payer: PlayerId,
        payee: PlayerId,
        asset: AssetId,
        count: u64
    },
    /// Cancel the remaining assets and coins in a buy or sell order
    CancelOrder {
        target: u64
    },
    /// Update the list of bankers to the given list
    UpdateBankers {
        bankers: Vec<PlayerId>,
        banker: PlayerId,
    },
    /// Update the list of items that the bank is willing to convert
    // UpdateConvertables {
    //     convertables: Vec<Conversion>,
    //     banker: PlayerId,
    // },
    /// Give a player access to invested items, and lock away the items needed to replenish the invested stock
    // InstantConvert {
    //     player: PlayerId,
    //     from: AssetId,
    //     to: AssetId,
    //     count: u64
    // },
    /// Used to correct typos
    Undeposit {
        player: PlayerId,
        asset: AssetId,
        count: u64,
        banker: PlayerId
    },
}
impl Action {
    fn adjust_audit(&self, mut audit: Audit) -> Option<Audit> {
        match self {
            Action::Deposit { asset, count, .. } => {
                audit.add_asset(asset.clone(), *count);
                Some(audit)
            },
            Action::Undeposit { asset, count, .. } => {
                audit.sub_asset(asset.clone(), *count);
                Some(audit.clone())
            }
            Action::WithdrawalCompleted{..} => {
                // We don't know what the withdrawal is just from the id
                //
                // TODO: find a way to track this nicely
                None
            },
            Action::BuyCoins { n_diamonds, .. } => {
                audit.sub_asset(DIAMOND_NAME.into(), *n_diamonds);
                audit.add_coins(DIAMOND_RAW_COINS.checked_mul(*n_diamonds).unwrap());
                Some(audit.clone())
            },
            Action::SellCoins { n_diamonds, .. } => {
                audit.add_asset(DIAMOND_NAME.into(), *n_diamonds);
                audit.sub_coins(DIAMOND_RAW_COINS.checked_mul(*n_diamonds).unwrap());
                Some(audit.clone())
            },
            _ => Some(audit)
        }
    }
}

#[derive(Default, Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Audit {
    pub coins: Coins,
    pub assets: std::collections::HashMap<AssetId, u64>
}
impl Audit {
    pub fn add_asset(&mut self, asset: AssetId, count: u64) {
        if count > 0 {
            let entry = self.assets.entry(asset).or_default();
            *entry = entry.checked_add(count).expect("Failed to add asset to audit")
        }
    }
    pub fn sub_asset(&mut self, asset: AssetId, count: u64) {
        if count == 0 {
            return;
        }
        let std::collections::hash_map::Entry::Occupied(mut entry) = self.assets.entry(asset)
        else { panic!("Tried to remove empty asset from audit") };
        match entry.get().checked_sub(count) {
            Some(0) => {entry.remove();  },
            None => panic!("Failed to remove asset from audit"),
            Some(res) => { *entry.get_mut() = res; }
        }
    }
    pub fn add_coins(&mut self, count: Coins) {
        self.coins.checked_add_assign(count).expect("Failed to add coins to audit")
    }
    pub fn sub_coins(&mut self, count: Coins) {
        self.coins.checked_sub_assign(count).expect("Failed to remove coins from audit")
    }
}
impl Add for Audit {
    type Output = Audit;

    fn add(self, rhs: Self) -> Self::Output {
        let mut ret = self;
        ret += rhs;
        ret
    }
}
impl AddAssign for Audit {
    fn add_assign(&mut self, rhs: Self) {
        self.coins.checked_add_assign(rhs.coins).expect("Audit merge failed: coin overflow");
        rhs.assets.into_iter().for_each(|(asset, count)| self.add_asset(asset, count));
    }
}
impl AddAssign<&Audit> for Audit {
    fn add_assign(&mut self, rhs: &Audit) {
        self.coins.checked_add_assign(rhs.coins).expect("Audit merge failed: coin overflow");
        rhs.assets.iter().for_each(|(asset, count)| self.add_asset(asset.clone(), *count));
    }
}

pub trait Auditable {
    // Check internal counters, will be called after every action
    fn soft_audit(&self) -> Audit;
    // Verify internal counters, will be called rarely. Should panic if inconsistencies found
    fn hard_audit(&self) -> Audit;
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct WrappedAction {
    // The id of the action, which should equal the line number of the trades list
    pub id: u64,
    // The time this action was performed
    pub time: chrono::DateTime<chrono::Utc>,
    // The action itself
    pub action: Action,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct AssetInfo {
    pub stack_size: u64
}
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    OverdrawnAsset {
        asset: AssetId,
        amount_overdrawn: u64
    },
    OverdrawnCoins {
        amount_overdrawn: Coins
    },
    UnauthorisedWithdrawal{
        asset: AssetId,
        // Set to be None if the player has no authorization at all to withdraw this
        amount_overdrawn: Option<u64>
    },
    /// Some 1337 hacker tried an overflow attack >:(
    Overflow,
    InvalidId{id: u64},
    UnknownAsset{asset: AssetId},
    AlreadyDone,
    NotABanker{player: PlayerId},
    CoinStringMangled,
    CoinStringTooPrecise,
    InvalidRates,
    InvalidFastSync
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::OverdrawnAsset { asset, amount_overdrawn } => {
                write!(f, "Player needs {amount_overdrawn} more {asset} to perform this action.")
            },
            Error::OverdrawnCoins { amount_overdrawn } => {
                write!(f, "Player needs {amount_overdrawn} more to perform this action.")
            },
            Error::UnauthorisedWithdrawal { asset, amount_overdrawn } => {
                match amount_overdrawn {
                    Some(amount_overdrawn) => write!(f, "Player needs authorisation to withdraw {amount_overdrawn} more {asset}."),
                    None => write!(f, "Player needs authorisation to withdraw {asset}.")
                }

            },
            Error::Overflow => {
                write!(f, "The request was so messed up it could have overflowed!")
            },
            Error::InvalidId { id } => {
                write!(f, "The action ID {id} was invalid.")
            }
            Error::UnknownAsset { asset } => {
                write!(f, "The item \"{asset}\" is not on our list.")
            },
            Error::AlreadyDone => {
                write!(f, "The requested action is redundant.")
            },
            Error::NotABanker { player } => {
                write!(f, "The requested action would require {player} to be a banker, but they are not.")
            },
            Error::CoinStringMangled => {
                write!(f, "The system could not understand the given coin amount.")
            },
            Error::CoinStringTooPrecise => {
                write!(f, "Too much precision was given for the coins: the system can only handle 3 decimal places.")
            },
            Error::InvalidRates => {
                write!(f, "The provided rates were invalid")
            },
            Error::InvalidFastSync => {
                write!(f, "The provided FastSync struct was corrupted")
            }
        }

    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct BankRates {
    investment_ppm: u64,
    buy_order_ppm: u64,
    sell_order_ppm: u64,
    diamond_buy_ppm: u64,
    diamond_sell_ppm: u64
}
impl BankRates {
    fn check(&self) -> Result<()> {
        if
            self.investment_ppm > 1_000_000 ||
            // We don't need to limit this, as they just pay a lot, rather than losing money
            // self.buy_order_ppm > 1_000_000 ||
            self.sell_order_ppm > 1_000_000 ||
            self.diamond_sell_ppm > 1_000_000
            // We don't need to limit this, as they just pay a lot, rather than losing money
            // self.diamond_buy_ppm > 1_000_000
        {
            Err(Error::InvalidRates)
        }
        else {
            Ok(())
        }
    }
}

static DEFAULT_ASSET_INFO: std::sync::LazyLock<std::collections::HashMap<AssetId, AssetInfo>> = std::sync::LazyLock::new(|| serde_json::from_str(include_str!("../resources/assets.json")).expect("Could not parse asset_info"));

#[derive(Debug, Clone)]
pub struct State {
    next_id: u64,
    rates: BankRates,

    asset_info: std::collections::HashMap<AssetId, AssetInfo>,
    auth: auth::AuthTracker,
    balance: balance::BalanceTracker,
    order: order::OrderTracker,
    withdrawal: withdrawal::WithdrawalTracker
}
impl Default for State {
    fn default() -> State {
        let asset_info = DEFAULT_ASSET_INFO.clone();
        State {
            rates: INITIAL_BANK_RATES,
            // earnings: Default::default(),
            // Start on ID 1 for nice mapping to line numbers
            next_id: 1,
            auth: Default::default(),
            balance: Default::default(),
            order: Default::default(),
            withdrawal: Default::default(),
            asset_info,
        }
    }
}
impl State {
    /// Create a new empty state
    pub fn new() -> State { Self::default() }
    /// Adds or updates the given asset infos
    pub fn update_asset_info(&mut self, asset_info: std::collections::HashMap<AssetId, AssetInfo>) {
        self.asset_info.extend(asset_info);
    }
    /// Gets info about a certain asset
    pub fn asset_info(&self, asset: &AssetId) -> Result<AssetInfo> {
        self.asset_info.get(asset).cloned().ok_or_else(|| Error::UnknownAsset { asset: asset.clone() })
    }
    /// Get the next line
    pub fn get_next_id(&self) -> u64 { self.next_id }
    /// Get a player's balance
    pub fn get_bal(&self, player: &PlayerId) -> Coins { self.balance.get_bal(player) }
    /// Get all balances
    pub fn get_bals(&self) -> std::collections::HashMap<PlayerId, Coins> { self.balance.get_bals() }
    /// Get a player's assets
    pub fn get_assets(&self, player: &PlayerId) -> std::collections::HashMap<AssetId, u64> { self.balance.get_assets(player) }
    /// Get all players' assets
    pub fn get_all_assets(&self) -> &std::collections::HashMap<PlayerId, std::collections::HashMap<AssetId, u64>> { self.balance.get_all_assets() }
    /// List all withdrawals
    pub fn get_withdrawals(&self) -> std::collections::BTreeMap<u64, PendingWithdrawal> { self.withdrawal.get_withdrawals() }
    /// List all withdrawals
    pub fn get_withdrawal(&self, id: u64) -> Result<PendingWithdrawal> { self.withdrawal.get_withdrawal(id) }
    /// Get the withdrawal the bankers should examine next
    pub fn get_next_withdrawal(&self) -> Option<PendingWithdrawal> { self.withdrawal.get_next_withdrawal() }
    /// List all orders
    pub fn get_orders(&self) -> std::collections::BTreeMap<u64, PendingOrder> { self.order.get_all() }
    /// List all orders
    pub fn get_orders_filter<'a>(&'a self, filter: impl Fn(&PendingOrder) -> bool + 'a) -> impl Iterator<Item=PendingOrder> + 'a { self.order.get_orders_filter(filter) }
    /// Get a specific order
    pub fn get_order(&self, id: u64) -> Result<PendingOrder> { self.order.get_order(id) }
    /// Prices for an asset, returns (price, amount) in (buy, sell)
    pub fn get_prices(&self, asset: &AssetId) -> (std::collections::BTreeMap<Coins, u64>, std::collections::BTreeMap<Coins, u64>) { self.order.get_prices(asset) }
    /// Returns true if the given item is currently restricted
    pub fn is_restricted(&self, asset: &AssetId) -> bool { self.auth.is_restricted(asset) }
    /// Lists all restricted items
    pub fn get_restricted(&self) -> impl IntoIterator<Item = &AssetId> { self.auth.get_restricted() }
    /// Gets a list of all bankers
    pub fn get_bankers(&self) -> HashSet<PlayerId> { self.auth.get_bankers() }
    /// Returns true if the given player is an banker
    pub fn is_banker(&self, player: &PlayerId) -> bool { self.auth.is_banker(player) }
    /// Get the required permissions for a given action
    pub fn perms(&self, action: &Action) -> Result<ActionPermissions> {
        match action {
            Action::AuthoriseRestricted { banker, .. } |
            Action::Deleted { banker, .. } |
            Action::Deposit { banker, .. } |
            Action::UpdateBankRates { banker, .. } |
            Action::UpdateBankers { banker, .. } |
            Action::UpdateRestricted { banker, .. } |
            Action::WithdrawalCompleted { banker, .. } |
            Action::Undeposit { banker, .. }
                => Ok(ActionPermissions{level: ActionLevel::Banker, player: banker.clone()}),

            Action::BuyCoins { player, .. } |
            Action::BuyOrder { player, .. } |
            Action::SellCoins { player, .. } |
            Action::SellOrder { player, .. } |
            Action::TransferAsset { payer: player, .. } |
            Action::TransferCoins { payer: player, .. } |
            Action::WithdrawalRequested { player, .. }
                => Ok(ActionPermissions{level: ActionLevel::Normal, player: player.clone()}),

            Action::CancelOrder { target } =>
                Ok(ActionPermissions{level: ActionLevel::Normal, player: self.order.get_order(*target)?.player.clone()})
        }
    }
    /// Nice macro for checking whether a player is a banker
    fn check_banker(&self, player: &PlayerId) -> Result<()> {
        if self.auth.is_banker(player) {
            Ok(())
        }
        else {
            Err(Error::NotABanker { player: player.clone() })
        }
    }
    // Atomic (but not parallelisable!).
    // This means the function will change significant things (i.e. more than just creating empty lists) IF AND ONLY IF it fully succeeds.
    // As such, we don't have to worry about giving it bad actions
    fn apply_inner(&mut self, id: u64, action: Action) -> Result<()> {
        // Blanket check perms
        //
        // TODO: optimise
        if let ActionPermissions { level: ActionLevel::Banker, player } = self.perms(&action)? {
            if !self.is_banker(&player) {
                return Err(Error::NotABanker { player });
            }
        }

        match action {
            Action::Deleted{..} => Ok(()),
            Action::Deposit { player, asset, count, banker } => {
                self.check_banker(&banker)?;
                if !self.asset_info.contains_key(&asset) {
                    return Err(Error::UnknownAsset { asset });
                }
                self.balance.commit_asset_add(&player, &asset, count);
                self.auth.increase_authorisation(player, asset, count).expect("Authorisation overflow");

                Ok(())
            },
            Action::Undeposit { player, asset, count, banker } => {
                self.check_banker(&banker)?;
                self.balance.commit_asset_removal(&player, &asset, count)
            },
            Action::WithdrawalRequested { player, assets} => {
                // There's no good way of doing this without two passes, so we check then commit
                //
                // BTreeMap ensures that the same asset cannot occur twice, so we don't have to worry about double spending
                for (asset, count) in assets.iter() {
                    // Check to see if they can afford it
                    self.balance.check_asset_removal(&player, asset, *count)?;
                    // Check to see if they are allowed it
                    self.auth.check_withdrawal_authorized(&player, asset, *count)?;
                }

                // Now take the assets, as we've confirmed they can afford it
                for (asset, count) in assets.iter() {
                    // Remove assets
                    self.balance.commit_asset_removal(&player, asset, *count).expect("Assets disappeared after check");
                    // Remove allowance if restricted
                    self.auth.commit_withdrawal_authorized(&player, asset, *count).expect("Auth disappeared after check");
                }

                // Register the withdrawal. This cannot fail, so we don't have to worry about atomicity
                self.withdrawal.track_withdrawal(id, player, assets, Coins::default());
                Ok(())
            },
            Action::SellOrder { player, asset, count, coins_per } => {
                // Check and take their assets first
                self.balance.commit_asset_removal(&player, &asset, count)?;
                // Do the matching and listing
                let res = self.order.handle_sell(id, &player, &asset, count, coins_per, self.rates.sell_order_ppm);
                // Transfer the assets
                for (buyer, count) in res.assets_instant_matched {
                    self.balance.commit_asset_add(&buyer, &asset, count);
                }
                // Transfer the money
                self.balance.commit_coin_add(&player, res.coins_instant_earned);
                // Pay the bank
                self.balance.commit_coin_add(&PlayerId::the_bank(), res.instant_bank_fee);

                Ok(())
            },
            Action::BuyOrder { player, asset, count, coins_per } => {
                // Check their money first
                let mut max_cost = coins_per.checked_mul(count)?;
                max_cost.checked_add_assign(max_cost.fee_ppm(self.rates.buy_order_ppm)?)?;
                self.balance.check_coin_removal(&player, max_cost)?;

                // Do the matching and listing
                let res = self.order.handle_buy(id, &player, &asset, count, coins_per, self.rates.buy_order_ppm);
                // Transfer the money
                self.balance.commit_coin_removal(&player, res.cost).expect("Somehow used more money in buy order than expected");
                // Pay the sellers
                for (seller, coins) in res.sellers {
                    self.balance.commit_coin_add(&seller, coins)
                }
                // Transfer the assets
                if res.assets_instant_matched > 0 {
                    self.balance.commit_asset_add(&player, &asset, res.assets_instant_matched);
                }
                // Pay the bank
                self.balance.commit_coin_add(&PlayerId::the_bank(), res.instant_bank_fee);

                Ok(())
            },
            Action::WithdrawalCompleted { target, banker } => {
                self.check_banker(&banker)?;
                // Try to take out the pending transaction
                let res = self.withdrawal.complete(target)?;
                // Add the profit
                self.balance.commit_coin_add(&PlayerId::the_bank(), res.total_fee);
                Ok(())
            },
            Action::CancelOrder { target } => {
                match self.order.cancel(target)? {
                    order::CancelResult::BuyOrder { player, refund_coins } => {
                        self.balance.commit_coin_add(&player, refund_coins);
                    },
                    order::CancelResult::SellOrder { player, refunded_asset, refund_count } => {
                        self.balance.commit_asset_add(&player, &refunded_asset, refund_count);
                    }
                }
                Ok(())
            },
            Action::BuyCoins { player, n_diamonds } => {
                // Check and take diamonds from payer...
                self.balance.commit_asset_removal(&player,&DIAMOND_NAME.to_owned(), n_diamonds)?;
                // ... and give them the coins
                let n_coins = DIAMOND_RAW_COINS.checked_mul(n_diamonds).expect("BuyCoins overflow");
                let fee = n_coins.fee_ppm(self.rates.diamond_sell_ppm).expect("BuyCoins fee overflow"); // This panic stops inconsistencies
                self.balance.commit_coin_add(&PlayerId::the_bank(), fee);
                self.balance.commit_coin_add(&player, n_coins.checked_sub(fee).unwrap()); // This panic stops inconsistencies
                Ok(())
            },
            Action::SellCoins { player, n_diamonds } => {
                // Check and take coins from payer...
                let n_coins = DIAMOND_RAW_COINS.checked_mul(n_diamonds)?;
                let fee = n_coins.fee_ppm(self.rates.diamond_sell_ppm)?; // This panic stops inconsistencies
                self.balance.commit_coin_removal(&player, n_coins.checked_add(fee)?)?;
                self.balance.commit_coin_add(&PlayerId::the_bank(), fee);
                // ... and give them the diamonds
                self.balance.commit_asset_add(&player, &DIAMOND_NAME.to_owned(), n_diamonds);
                Ok(())
            },
            Action::UpdateRestricted { restricted_assets , banker} => {
                self.check_banker(&banker)?;
                // Check they're valid assets
                if let Some(asset) =
                    restricted_assets.iter()
                    .find(|id| !self.asset_info.contains_key(*id))
                {
                    return Err(Error::UnknownAsset { asset: asset.clone() });
                }
                let restricted_assets = HashSet::from_iter(restricted_assets);
                // A list of the assets that just became restricted
                let newly_restricted = restricted_assets.difference(self.auth.get_restricted()).cloned().collect::<Vec<_>>();
                self.auth.update_restricted(restricted_assets.clone());
                // Authorise everyone who is already holding the item to withdraw what they have
                for asset in newly_restricted {
                    for (player, their_assets) in self.balance.get_all_assets() {
                        let Some(count) = their_assets.get(&asset).copied() else { continue; };
                        self.auth.set_authorisation(player.clone(), asset.clone(), count);
                    }
                }
                Ok(())
            },
            Action::AuthoriseRestricted { authorisee, asset, new_count, banker } => {
                self.check_banker(&banker)?;
                // Check it's a valid asset (not necessarily authorisable to enable pre-authorisation)
                if !self.asset_info.contains_key(&asset) {
                    return Err(Error::UnknownAsset { asset });
                }
                self.auth.set_authorisation(authorisee, asset, new_count);
                Ok(())
            },
            Action::UpdateBankRates { rates , banker } => {
                // Check that the banker is actually a banker
                self.check_banker(&banker)?;
                // Check they're consistent
                rates.check()?;
                // Set the rates
                self.rates = rates;
                Ok(())
            },
            Action::TransferCoins { payer, payee, count } => {
                // Check and take money from payer...
                self.balance.commit_coin_removal(&payer, count)?;
                // ... and give it to payee
                self.balance.commit_coin_add(&payee, count);
                Ok(())
            },
            Action::TransferAsset { payer, payee, asset, count } => {
                // Check and take assets from payer...
                self.balance.commit_asset_removal(&payer, &asset, count)?;
                // ... and give it to payee
                self.balance.commit_asset_add(&payee,  &asset, count);
                Ok(())
            },
            Action::UpdateBankers { bankers, banker } => {
                self.check_banker(&banker)?;
                self.auth.update_bankers(FromIterator::from_iter(bankers));
                Ok(())
            },
            /*
            Action::InstantConvert { from, to, count, player } => {
                // BUG: will fail audit
                // Check convertable
                if self.convertables.contains(&(from.clone(), to.clone())) {
                    return Err(Error::NotConvertable { from, to });
                }
                // Calculate the fee
                let min_stack_size = self.asset_info(&from)?.stack_size.min(self.asset_info(&to)?.stack_size);
                let n_stacks = count.div_ceil(min_stack_size);
                let fee = n_stacks * self.fees.instant_smelt_per_stack;

                // Check to see if they can afford the fees
                self.balance.check_coin_removal(&player, fee)?;
                // Check to see if they can afford the assets
                self.balance.check_asset_removal(&player, &from, count)?;
                // Check to see if we can lend this out, and if so, do everything
                self.investment.try_mark_busy(&to, count)?;
                self.investment.mark_confirmed(&player, &to, count);
                self.balance.commit_asset_removal(&player, &from, count).expect("Unable to commit asset removal after check");
                self.balance.commit_coin_removal(&player, count).expect("Unable to commit coin removal after check");
                // Distribute the fee
                self.distribute_profit(&to, fee);

                // Give the assets
                self.balance.commit_asset_add(&player, &to, count);

                Ok(())
            }
            Action::UpdateConvertables { convertables, .. } => {
                self.convertables = convertables.into_iter().collect();
                Ok(())
            } */
        }
    }
    /// Load in the transactions from a trade file. Because of numbering, we must do this first; we cannot append
    pub async fn replay(&mut self, trade_file: &mut (impl tokio::io::AsyncBufRead + std::marker::Unpin), hard_audit: bool) -> Result<()> {
        let mut trade_file_lines = trade_file.lines();
        macro_rules! do_audit {
            () => {
                if hard_audit { self.hard_audit() } else { self.soft_audit() }
            };
        }
        let mut last_audit = do_audit!();
        while let Some(line) = trade_file_lines.next_line().await.expect("Could not read line from trade list") {
            let wrapped_action: WrappedAction = serde_json::from_str(&line).expect("Corrupted trade file");
            if wrapped_action.id != self.next_id {
                panic!("Trade file ID mismatch: action {} found on line {}: {}", wrapped_action.id, self.next_id, line);
            }
            self.apply_inner(self.next_id, wrapped_action.action.clone())?;
            if let Some(new_audit) = wrapped_action.action.adjust_audit(last_audit) {
                let post = do_audit!();
                if new_audit != post {
                    panic!("Failed audit on {line}: expected {new_audit:?} vs actual {post:?}");
                }
                last_audit = new_audit;
            }
            else {
                // The state has changed, adjust the audit
                last_audit = do_audit!();
            }
            self.next_id += 1;
        }
        Ok(())
    }
    /// Atomically try to apply an action, and if successful, write to given stream
    pub async fn apply(&mut self, action: Action, mut out: impl tokio::io::AsyncWrite) -> Result<u64> {
        let id = self.next_id;
        let wrapped_action = WrappedAction {
            id,
            time: chrono::offset::Utc::now(),
            action: action.clone(),
        };
        let mut line = serde_json::to_string(&wrapped_action).expect("Cannot serialise action");
        let pre = self.hard_audit();
        self.apply_inner(self.next_id, wrapped_action.action)?;
        // We can soft audit, as the last one was checked as required
        if let Some(expected) = action.adjust_audit(pre) {
            let post = self.soft_audit();
            if expected != post {
                panic!("Failed audit on {line}: expected {expected:?} vs actual {post:?}");
            }
        }
        line.push('\n');
        self.next_id += 1;
        let mut out = pin!(out);
        out.write_all(line.as_bytes()).await.expect("Could not write to log, must immediately stop!");
        out.flush().await.expect("Could not flush to log, must immediately stop!");
        Ok(id)
    }
}
impl Auditable for State {
    fn soft_audit(&self) -> Audit {
        self.balance.soft_audit() + self.order.soft_audit() + self.withdrawal.soft_audit()
    }

    fn hard_audit(&self) -> Audit {
        self.balance.hard_audit() + self.order.hard_audit() + self.withdrawal.hard_audit()
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct StateSync {
    pub current_id: u64,
    pub balances: BalanceSync,
    pub rates: BankRates,
    pub auth: AuthSync,
    // pub investment: InvestmentSync,
    pub order: OrderSync,
    pub withdrawal: WithdrawalSync
}
impl From<&State> for StateSync {
    fn from(value: &State) -> Self {
        Self {
            current_id: value.next_id.checked_sub(1).unwrap(),
            balances: (&value.balance).into(),
            rates: value.rates.clone(),
            auth: (&value.auth).into(),
            order: (&value.order).into(),
            withdrawal: (&value.withdrawal).into(),
        }
    }
}
impl TryFrom<StateSync> for State {
    type Error = Error;
    fn try_from(value: StateSync) -> Result<Self> {
        Ok(Self {
            next_id: value.current_id.checked_add(1).ok_or(Error::Overflow)?,
            rates: value.rates,
            balance: value.balances.try_into()?,
            asset_info: DEFAULT_ASSET_INFO.clone(),
            order: value.order.try_into()?,
            withdrawal: value.withdrawal.try_into()?,
            auth: value.auth.try_into()?,
        })
    }
}
