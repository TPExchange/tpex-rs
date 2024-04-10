use std::{collections::HashSet, ops::{Add, AddAssign, Mul}};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

// We use a base coins, which represent 1/1000 of a diamond
use serde::{Deserialize, Serialize, ser::SerializeMap};

use self::{order::PendingOrder, withdrawal::PendingWithdrawl};

mod balance;
mod investment;
mod order;
mod withdrawal;
#[cfg(test)]
mod tests;

pub use order::OrderType;

pub const COINS_PER_DIAMOND: u64 = 1000;
pub const DIAMOND_NAME: &str = "diamond";
const INITIAL_BANK_PRICES: UpdateBankPrices = UpdateBankPrices {
    withdraw_flat: 1000,
    withdraw_per_stack: 50,
    expedited: 5000,
    investment_share: 0.5,
    instant_smelt_per_stack: 100
};

#[derive(PartialEq, PartialOrd, Eq, Ord, Default, Debug, Clone, Hash)]
pub struct PlayerId(String);
impl PlayerId {
    #[deprecated = "Do not use this, use player_id instead"]
    pub fn evil_constructor(s: String) -> PlayerId { PlayerId(s) }
    #[deprecated = "Do not use this, use user_id instead"]
    pub fn evil_deref(&self) -> &String { &self.0 }
    pub fn the_bank() -> PlayerId { PlayerId("bank".to_owned()) }
}
impl<'de> Deserialize<'de> for PlayerId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de> {
        String::deserialize(deserializer).map(PlayerId)
    }
}
impl Serialize for PlayerId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
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

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub enum Action {
    /// Deleted transaction, for when someone does a bad
    Deleted {
        banker: PlayerId
    },
    /// Player deposited assets
    Deposit {
        player: PlayerId,
        asset: AssetId,
        count: u64,
        banker: PlayerId,
    },
    /// Player asked to expedite withdrawal
    Expedited {
        target: u64
    },
    /// Player asked to withdraw assets
    WithdrawlRequested {
        player: PlayerId,
        assets: std::collections::HashMap<AssetId,u64>
    },
    /// A banker has agreed to take out assets imminently
    WithdrawlCompleted {
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
        coins_per: u64,
    },
    /// Player offers to sell assets at a price, and locks away assets until cancelled
    ///
    /// Instant matches should favour the seller
    SellOrder {
        player: PlayerId,
        asset: AssetId,
        count: u64,
        coins_per: u64,
    },
    // Donation {
    //     asset: AssetId,
    //     count: u64,
    //     banker: PlayerId,
    // },
    /// Updates the list of assets that require prior authorisation from an admin
    UpdateRestricted {
        restricted_assets: Vec<AssetId>,
        banker: PlayerId,
    },
    /// Allows a player to place new withdrawal requests up to new_count of an item
    AuthoriseRestricted {
        authorisee: PlayerId,
        banker: PlayerId,
        asset: AssetId,
        new_count: u64
    },
    /// Changes the fees
    UpdateBankPrices {
        withdraw_flat: u64,
        withdraw_per_stack: u64,
        expedited: u64,
        investment_share: f64,
        instant_smelt_per_stack: u64,
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
        count: u64
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
        target_id: u64
    },
    /// Update the list of bankers to the given list
    UpdateBankers {
        bankers: Vec<PlayerId>,
        banker: PlayerId,
    },
    /// Update the list of investable assets to the given list
    UpdateInvestables {
        assets: Vec<AssetId>,
        banker: PlayerId,
    },
    /// Lock away assets for use by the bank, with the hope of profit
    Invest {
        player: PlayerId,
        asset: AssetId,
        count: u64
    },
    /// Stop the given assets from being locked away, but should fail if they're currently in use
    Uninvest {
        player: PlayerId,
        asset: AssetId,
        count: u64
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
    /// Used to normalise items on deposit
    UpdateAutoConvert {
        conversions: Vec<AutoConversion>,
        banker: PlayerId
    }
}
impl Action {
    fn adjust_audit(&self, mut audit: Audit) -> Option<Audit> {
        match self {
            Action::Deposit { .. } => {
                // Autoconversion messes this up
                None
                // audit.add_asset(asset.clone(), *count);
                // Some(audit)
            },
            Action::Undeposit { .. } => {
                // Autoconversion messes this up
                None
                // audit.sub_asset(asset.clone(), *count).expect("Unable to adjust down deposit");
            }
            Action::WithdrawlCompleted{..} => {
                // We don't know what the withdrawal is just from the action
                //
                // TODO: find a way to track this nicely
                None
            },
            Action::BuyCoins { n_diamonds,.. } => {
                audit.coins += *n_diamonds * COINS_PER_DIAMOND;
                audit.sub_asset(DIAMOND_NAME.to_owned(), *n_diamonds).expect("Unable to adjust down buy coins audit");
                Some(audit)
            },
            Action::SellCoins { n_diamonds, .. } => {
                audit.sub_coins(*n_diamonds * COINS_PER_DIAMOND).expect("Unable to adjust sell buy coins audit");
                audit.add_asset(DIAMOND_NAME.to_owned(), *n_diamonds);
                Some(audit)
            },
            _ => Some(audit)
        }
    }
}

#[derive(Default, Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Audit {
    pub coins: u64,
    pub assets: std::collections::HashMap<AssetId, u64>
}
impl Audit {
    pub fn add_asset(&mut self, asset: AssetId, count: u64) {
        if count > 0 {
            *self.assets.entry(asset).or_default() += count;
        }
    }
    #[must_use]
    pub fn sub_asset(&mut self, asset: AssetId, count: u64) -> Option<()> {
        if count == 0 {
            return Some(())
        }
        let std::collections::hash_map::Entry::Occupied(mut entry) = self.assets.entry(asset)
        else { return None; };
        match entry.get().checked_sub(count) {
            Some(0) => {entry.remove(); Some(()) },
            None => None,
            Some(res) => { *entry.get_mut() = res; Some(()) }
        }
    }
    #[must_use]
    pub fn sub_coins(&mut self, count: u64) -> Option<()> {
        self.coins.checked_sub(count).map(|res| self.coins = res)
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
        self.coins += rhs.coins;
        rhs.assets.into_iter().for_each(|(asset, count)| self.add_asset(asset, count));
    }
}

pub trait Auditable {
    // Check internal counters, will be called after every action
    fn soft_audit(&self) -> Audit;
    // Verify internal counters, will be called rarely. Should panic if inconsistencies found
    fn hard_audit(&self) -> Audit;
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct WrappedAction {
    // The id of the action, which should equal the line number of the trades list
    id: u64,
    // The time this action was performed
    time: chrono::DateTime<chrono::Utc>,
    // The action itself
    action: Action,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct AssetInfo {
    stack_size: u64
}
#[derive(Debug)]
pub enum Error {
    Overdrawn{
        /// If None, then it's coins overdrawn
        asset: Option<AssetId>,
        amount_overdrawn: u64
    },
    UnauthorisedWithdrawl{asset: AssetId, amount_overdrawn: Option<u64>},
    /// Some 1337 hacker tried an overflow attack >:(
    Overflow,
    InvalidId{id: u64},
    UnknownAsset{asset: AssetId},
    NotInvestable{asset: AssetId},
    InvestmentBusy{asset: AssetId, amount_over: u64},
    NotConvertable{from: AssetId, to: AssetId},
    AlreadyDone,
    IsNotABanker{player: PlayerId}
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Overdrawn { asset, amount_overdrawn } => {
                match asset {
                    Some(asset) => write!(f, "Player needs {amount_overdrawn} more {asset} to perform this action."),
                    None => write!(f, "Player needs {amount_overdrawn} more coins to perform this action.")
                }
            }
            Error::UnauthorisedWithdrawl { asset, amount_overdrawn } => {
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
            Error::NotInvestable { asset } => {
                write!(f, "The item \"{asset}\" is not investable yet.")
            },
            Error::InvestmentBusy { asset, amount_over} => {
                write!(f, "The action failed, as we would need {amount_over} more invested {asset}.")
            },
            Error::NotConvertable { from, to } => {
                write!(f, "We do not currently offer conversion from {from} to {to}.")
            },
            Error::AlreadyDone => {
                write!(f, "The requested action is redundant.")
            },
            Error::IsNotABanker { player } => {
                write!(f, "The requested action would require {player} to be a banker, but they are not.")
            }
        }

    }
}
impl std::error::Error for Error {}
#[derive(Debug, Serialize, Clone)]
struct UpdateBankPrices {
    withdraw_flat: u64,
    withdraw_per_stack: u64,
    expedited: u64,
    investment_share: f64,
    instant_smelt_per_stack: u64
}

#[derive(Debug, Clone)]
pub struct State {
    next_id: u64,
    asset_info: std::collections::HashMap<AssetId, AssetInfo>,
    fees: UpdateBankPrices,
    auto_convertables: std::collections::HashMap<AssetId, AutoConversion>,

    restricted_assets: std::collections::HashSet<AssetId>,
    authorisations: std::collections::HashMap<PlayerId, std::collections::HashMap<AssetId, u64>>,
    investables: std::collections::HashSet<AssetId>,

    earnings: std::collections::HashMap<PlayerId, u64>,
    bankers: std::collections::HashSet<PlayerId>,

    balance: balance::BalanceTracker,
    investment: investment::InvestmentTracker,
    order: order::OrderTracker,
    withdrawal: withdrawal::WithdrawalTracker
}
impl Default for State {
    fn default() -> State {
        let asset_info = serde_json::from_str(include_str!("../resources/assets.json")).expect("Could not parse asset_info");
        State {
            asset_info,
            fees: INITIAL_BANK_PRICES,
            restricted_assets: Default::default(),
            authorisations: Default::default(),
            earnings: Default::default(),
            // Start on ID 1 for nice mapping to line numbers
            next_id: 1,
            bankers: [PlayerId::the_bank()].into_iter().collect(),
            investables: Default::default(),
            auto_convertables: Default::default(),
            balance: Default::default(),
            investment: Default::default(),
            order: Default::default(),
            withdrawal: Default::default(),
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
    /// Get the next line
    pub fn get_next_id(&self) -> u64 { self.next_id }
    /// Get a player's balance
    pub fn get_bal(&self, player: &PlayerId) -> u64 { self.balance.get_bal(player) }
    /// Get a player's assets
    pub fn get_assets(&self, player: &PlayerId) -> std::collections::HashMap<AssetId, u64> { self.balance.get_assets(player) }
    /// Calculate the withdrawal fees
    pub fn calc_withdrawal_fee(&self, assets: &std::collections::HashMap<AssetId, u64>) -> Result<u64, Error> {
        let mut total_fee = self.fees.withdraw_flat;
        for (asset, count) in assets {
            total_fee += count.div_ceil(self.asset_info.get(asset).ok_or(Error::UnknownAsset{asset:asset.clone()})?.stack_size)
                              .checked_mul(self.fees.withdraw_per_stack).ok_or(Error::Overflow)?;
        }
        Ok(total_fee)
    }
    /// Get the expedite fee
    pub fn expedite_fee(&self) -> u64 { self.fees.expedited }
    /// List all withdrawals
    pub fn get_withdrawals(&self) -> std::collections::BTreeMap<u64, PendingWithdrawl> { self.withdrawal.get_withdrawals() }
    /// Get the withdrawal the bankers should examine next
    pub fn get_next_withdrawal(&self) -> Option<PendingWithdrawl> { self.withdrawal.get_next_withdrawal() }
    /// List all orders
    pub fn get_orders(&self) -> std::collections::BTreeMap<u64, PendingOrder> { self.order.get_all() }
    /// Get a specific order
    pub fn get_order(&self, id: u64) -> Result<PendingOrder, Error> { self.order.get_order(id) }
    /// Prices for an asset, returns (price, amount) in (buy, sell)
    pub fn get_prices(&self, asset: &AssetId) -> (std::collections::BTreeMap<u64, u64>, std::collections::BTreeMap<u64, u64>) { self.order.get_prices(asset) }
    /// Returns true if the given item is currently restricted
    pub fn is_restricted(&self, asset: &AssetId) -> bool { self.restricted_assets.contains(asset) }
    /// Lists all restricted items
    pub fn get_restricted(&self) -> impl Iterator<Item = &AssetId> { self.restricted_assets.iter() }
    /// Gets a list of all bankers
    pub fn get_bankers(&self) -> HashSet<PlayerId> { self.bankers.clone() }
    /// Returns true if the given player is an banker
    pub fn is_banker(&self, player: &PlayerId) -> bool { self.bankers.contains(player) }
    /// Gets info about a certain asset
    pub fn asset_info(&self, asset: &AssetId) -> Result<AssetInfo, Error> {
        self.asset_info.get(asset).cloned().ok_or_else(|| Error::UnknownAsset { asset: asset.clone() })
    }
    /// Get the required permissions for a given action
    pub fn perms(&self, action: &Action) -> Result<ActionPermissions, Error> {
        match action {
            Action::AuthoriseRestricted { banker, .. } |
            Action::Deleted { banker, .. } |
            Action::Deposit { banker, .. } |
            Action::UpdateBankPrices { banker, .. } |
            Action::UpdateBankers { banker, .. } |
            // Action::UpdateConvertables { banker, .. } |
            Action::UpdateInvestables { banker, .. } |
            Action::UpdateRestricted { banker, .. } |
            Action::WithdrawlCompleted { banker, .. } |
            Action::Undeposit { banker, .. } |
            Action::UpdateAutoConvert { banker, .. }
                => Ok(ActionPermissions{level: ActionLevel::Banker, player: banker.clone()}),

            Action::BuyCoins { player, .. } |
            Action::BuyOrder { player, .. } |
            // Action::InstantConvert { player, .. }  |
            Action::Invest { player, .. } |
            Action::SellCoins { player, .. } |
            Action::SellOrder { player, .. } |
            Action::TransferAsset { payer: player, .. } |
            Action::TransferCoins { payer: player, .. } |
            Action::Uninvest { player, .. } |
            Action::WithdrawlRequested { player, .. }
                => Ok(ActionPermissions{level: ActionLevel::Normal, player: player.clone()}),

            Action::Expedited { target } =>
                Ok(ActionPermissions{level: ActionLevel::Normal, player: self.withdrawal.get_withdrawal(*target)?.player.clone()}),
            Action::CancelOrder { target_id } =>
                Ok(ActionPermissions{level: ActionLevel::Normal, player: self.order.get_order(*target_id)?.player.clone()})


        }
    }
    /// Distribute the profits among the investors
    fn distribute_profit(&mut self, asset: &AssetId, amount: u64) {
        let mut investors = self.investment.get_investors(asset);
        // Let's be fair and not give ourselves all the money
        investors.remove(&PlayerId::the_bank());
        let share = (self.fees.investment_share.mul(amount as f64) / (investors.values().sum::<u64>() as f64)).floor() as u64;
        let mut total_distributed = 0;
        for (investor, shares) in investors {
            let investor_profit = share * shares;
            total_distributed += investor_profit;
            self.balance.commit_coin_add(&investor, investor_profit);
        }
        if total_distributed > amount {
            panic!("Profit distribution imprecision was too bad");
        }
        self.balance.commit_coin_add(&PlayerId::the_bank(), amount - total_distributed);
    }
    // Atomic (but not parallelisable!).
    // This means the function will change significant things (i.e. more than just creating empty lists) IF AND ONLY IF it fully succeeds.
    // As such, we don't have to worry about giving it bad actions
    fn apply_inner(&mut self, id: u64, action: Action) -> Result<(), Error> {
        // Blanket check perms
        //
        // TODO: optimise
        if let ActionPermissions { level: ActionLevel::Banker, player } = self.perms(&action)? {
            if !self.is_banker(&player) {
                return Err(Error::IsNotABanker { player });
            }
        }

        match action {
            Action::Deleted{..} => Ok(()),
            Action::Deposit { player, asset, count, .. } => {
                if !self.asset_info.contains_key(&asset) {
                    return Err(Error::UnknownAsset { asset });
                }
                if let Some(conversion) = self.auto_convertables.get(&asset) {
                    self.balance.commit_asset_add(&player, &conversion.to, count * conversion.n_to);
                }
                else {
                    self.balance.commit_asset_add(&player, &asset, count)
                }

                Ok(())
            },
            Action::Undeposit { player, asset, count, .. } => {
                self.balance.commit_asset_removal(&player, &asset, count)
            },
            Action::WithdrawlRequested { player, assets} => {
                let total_fee = self.calc_withdrawal_fee(&assets)?;

                let mut tracked_assets: std::collections::HashMap<AssetId, u64> = Default::default();
                // There's no good way of doing this without two passes, so we check then commit
                //
                // BTreeMap ensures that the same asset cannot occur twice, so we don't have to worry about double spending
                for (asset, count) in assets {
                    // Check to see if they can afford it
                    self.balance.check_asset_removal(&player, &asset, count)?;
                    let is_restricted = self.is_restricted(&asset);
                    // Check if restricted
                    if is_restricted {
                        // If it is restricted, we have to check before we take their assets
                        // Check if they are authorised to withdraw any amount of these items
                        let Some(auth_amount) = self.authorisations.get(&player).and_then(|x| x.get(&asset))
                        else { return Err(Error::UnauthorisedWithdrawl{ asset: asset.clone(), amount_overdrawn: None}); };
                        // Check if they are authorised to withdraw at least this many items
                        if *auth_amount < count {
                            return Err(Error::UnauthorisedWithdrawl{ asset: asset.clone(), amount_overdrawn: Some(count - *auth_amount)});
                        }
                    }
                    tracked_assets.insert(asset, count);
                }
                // Check they can afford the fee, and if they can, take it
                self.balance.commit_coin_removal(&player, total_fee)?;

                // Now take the assets, as we've confirmed they can afford it
                for (asset, count) in tracked_assets.iter() {
                    // Remove assets
                    self.balance.commit_asset_removal(&player, asset, *count).expect("Assets disappeared after check");
                    // Remove allowance if restricted
                    if self.is_restricted(asset) {
                        // TODO: Clean up after ourselves
                        *self.authorisations.get_mut(&player).expect("Asset player disappeared after check")
                                            .get_mut(asset).expect("Asset auth disappeared after check") -= count;
                    }
                }

                // Register the withdrawal. This cannot fail, so we don't have to worry about atomicity
                self.withdrawal.track_withdrawal(id, player, tracked_assets, total_fee);
                Ok(())
            },
            Action::SellOrder { player, asset, count, coins_per } => {
                // Check and take their assets first
                self.balance.commit_asset_removal(&player, &asset, count)?;
                // Do the matching and listing
                let res = self.order.handle_sell(id, &player, &asset, count, coins_per);
                // Transfer the assets
                for (buyer, count) in res.assets_instant_matched {
                    self.balance.commit_asset_add(&buyer, &asset, count);
                }
                // Transfer the money
                self.balance.commit_coin_add(&player, res.coins_instant_earned);

                Ok(())
            },
            Action::BuyOrder { player, asset, count, coins_per } => {
                // Check and take their money first
                self.balance.commit_coin_removal(&player, count.checked_mul(coins_per).ok_or(Error::Overflow)?)?;
                // Do the matching and listing
                let res = self.order.handle_buy(id, &player, &asset, count, coins_per);
                // Transfer the money
                self.balance.commit_coin_add(&player, res.coins_refunded);
                // Pay the sellers
                for (seller, coins) in res.sellers {
                    self.balance.commit_coin_add(&seller, coins)
                }
                // Transfer the assets
                if res.assets_instant_matched > 0 {
                    self.balance.commit_asset_add(&player, &asset, res.assets_instant_matched);
                }

                Ok(())
            },
            Action::WithdrawlCompleted { target, banker } => {
                // Try to take out the pending transaction
                let res = self.withdrawal.complete(target)?;
                // Mark who delivered
                *self.earnings.entry(banker).or_default() += res.total_fee;
                // Add the profit
                self.balance.commit_coin_add(&PlayerId::the_bank(), res.total_fee);
                Ok(())
            },
            Action::CancelOrder { target_id } => {
                match self.order.cancel(target_id)? {
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
                self.balance.commit_coin_add(&player, n_diamonds * COINS_PER_DIAMOND);
                Ok(())
            },
            Action::SellCoins { player, n_diamonds } => {
                // Check and take coins from payer...
                self.balance.commit_coin_removal(&player, n_diamonds.checked_mul(COINS_PER_DIAMOND).ok_or(Error::Overflow)?)?;
                // ... and give them the diamonds
                self.balance.commit_asset_add(&player, &DIAMOND_NAME.to_owned(), n_diamonds);
                Ok(())
            },
            Action::UpdateRestricted { restricted_assets , ..} => {
                // Check they're valid assets
                if let Some(asset) =
                    restricted_assets.iter()
                    .find(|id| !self.asset_info.contains_key(*id))
                {
                    return Err(Error::UnknownAsset { asset: asset.clone() });
                }
                self.restricted_assets = std::collections::HashSet::from_iter(restricted_assets);
                Ok(())
            },
            Action::AuthoriseRestricted { authorisee, asset, new_count, .. } => {
                // Check it's a valid asset (not necessarily authorisable to enable pre-authorisation)
                if !self.asset_info.contains_key(&asset) {
                    return Err(Error::UnknownAsset { asset });
                }
                self.authorisations.entry(authorisee).or_default().insert(asset, new_count);
                Ok(())
            },
            Action::UpdateBankPrices { withdraw_flat, withdraw_per_stack, expedited, investment_share, instant_smelt_per_stack , ..} => {
                self.fees = UpdateBankPrices{ withdraw_flat, withdraw_per_stack, expedited, investment_share, instant_smelt_per_stack };
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
            Action::Expedited { target, .. } => {
                // Find the withdrawal
                let withdrawal = self.withdrawal.get_withdrawal(id)?;
                // If the withdrawal is already expedited, this should not be attempted
                if withdrawal.expedited {
                    return Err(Error::AlreadyDone)
                }
                // Take the coins, as expediting must now work
                let fee = self.fees.expedited;
                self.balance.commit_coin_removal(&withdrawal.player, fee)?;
                // Expediting should always work here
                self.withdrawal.expedite(target, fee).expect("Withdrawal exists and is normal but cannot be expedited");
                Ok(())
            },
            Action::UpdateBankers { bankers, .. } => {
                self.bankers = std::collections::HashSet::from_iter(bankers);
                Ok(())
            },
            Action::UpdateInvestables { assets, .. } => {
                // Check they're valid assets
                if let Some(asset) =
                    assets.iter()
                    .find(|id| !self.asset_info.contains_key(*id))
                {
                    return Err(Error::UnknownAsset { asset: asset.clone() });
                }
                self.investables = assets.into_iter().collect();
                Ok(())
            },
            Action::Invest { player, asset, count } => {
                // Check to see if we can invest it
                if !self.investables.contains(&asset) {
                    return Err(Error::NotInvestable {asset});
                }
                // Check to see if the user can afford it, and if so, invest
                self.balance.commit_asset_removal(&player, &asset, count)?;
                self.investment.add_investment(&player, &asset, count);
                Ok(())
            },
            Action::Uninvest { player, asset, count } => {
                // Don't check to see if it's currently investable, or else stuff might get trapped
                self.investment.try_remove_investment(&player, &asset, count)?;
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
            Action::UpdateAutoConvert { conversions, .. } => {
                // Check each from and to are valid assets
                if let Some(asset) =
                    conversions.iter()
                    .flat_map(|conv| [&conv.from,&conv.to])
                    .find(|id| !self.asset_info.contains_key(*id))
                {
                    return Err(Error::UnknownAsset { asset: asset.clone() });
                }

                self.auto_convertables =
                    conversions.into_iter()
                    // Get the `from` asset for each conversion for quick lookup
                    .map(|conversion| (conversion.from.clone(), conversion.clone()))
                    .collect();
                Ok(())
            },
        }
    }
    /// Load in the transactions from a trade file. Because of numbering, we must do this first; we cannot append
    pub async fn replay(&mut self, trade_file: &mut (impl tokio::io::AsyncRead + std::marker::Unpin)) -> Result<(), Error> {
        let trade_file_reader = tokio::io::BufReader::new(trade_file);
        let mut trade_file_lines = trade_file_reader.lines();
        let mut last_audit = self.hard_audit();
        while let Some(line) = trade_file_lines.next_line().await.expect("Could not read line from trade list") {
            let wrapped_action: WrappedAction = serde_json::from_str(&line).expect("Corrupted trade file");
            if wrapped_action.id != self.next_id {
                panic!("Trade file ID mismatch: action {} found on line {}: {}", wrapped_action.id, self.next_id, line);
            }
            self.apply_inner(self.next_id, wrapped_action.action.clone())?;
            if let Some(new_audit) = wrapped_action.action.adjust_audit(last_audit) {
                let post = self.hard_audit();
                if new_audit != post {
                    panic!("Failed audit on {line}: expected {new_audit:?} vs actual {post:?}");
                }
                last_audit = new_audit;
            }
            else {
                // The state has changed, adjust the audit
                last_audit = self.hard_audit();
            }
            self.next_id += 1;
        }
        Ok(())
    }
    /// Atomically try to apply an action, and if successful, write to given stream
    pub async fn apply(&mut self, action: Action, out: &mut (impl tokio::io::AsyncWrite + std::marker::Unpin)) -> Result<u64, Error> {
        let id = self.next_id;
        let wrapped_action = WrappedAction {
            id,
            time: chrono::offset::Utc::now(),
            action: action.clone(),
        };
        let mut line = serde_json::to_string(&wrapped_action).expect("Cannot serialise action");
        let pre = self.soft_audit();
        self.apply_inner(self.next_id, wrapped_action.action)?;
        // We can soft audit, as the last one was checked as required
        if let Some(expected) = action.adjust_audit(pre) {
            let post = self.hard_audit();
            if expected != post {
                panic!("Failed audit on {line}: expected {expected:?} vs actual {post:?}");
            }
        }
        line.push('\n');
        self.next_id += 1;
        out.write_all(line.as_bytes()).await.expect("Could not write to log, must immediately stop!");
        out.flush().await.expect("Could not flush to log, must immediately stop!");
        Ok(id)
    }
}
impl Auditable for State {
    fn soft_audit(&self) -> Audit {
        self.balance.soft_audit() + self.investment.soft_audit() + self.order.soft_audit() + self.withdrawal.soft_audit()
    }

    fn hard_audit(&self) -> Audit {
        self.balance.hard_audit() + self.investment.hard_audit() + self.order.hard_audit() + self.withdrawal.hard_audit()
    }
}

impl Serialize for State {
    // Returns an object that can be used to check we haven't gone off the rails
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("next_id", &self.next_id)?;
        map.serialize_entry("balance", &self.balance)?;
        map.serialize_entry("order", &self.order)?;
        map.serialize_entry("investment", &self.investment)?;
        map.serialize_entry("authorisations", &self.authorisations)?;
        map.serialize_entry("restricted", &self.restricted_assets)?;
        map.serialize_entry("investables", &self.investables)?;
        map.serialize_entry("bankers", &self.bankers)?;
        map.serialize_entry("fees", &self.fees)?;
        map.end()
    }
}
