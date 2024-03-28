use std::collections::HashSet;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

// We use a base coins, which represent 1/1000 of a diamond
use serde::{Deserialize, Serialize};

#[derive(PartialEq, PartialOrd, Eq, Ord, Default, Debug, Clone, Hash)]
pub struct PlayerId(String);
impl PlayerId {
    #[deprecated = "Do not use this, use player_id instead"]
    pub(crate) fn evil_constructor(s: String) -> PlayerId { PlayerId(s) }
    #[deprecated = "Do not use this, use user_id instead"]
    pub(crate) fn evil_deref(&self) -> &String { &self.0 }
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
pub type AssetId = String;

const COINS_PER_DIAMOND: u64 = 1000;
const DIAMOND_NAME: &str = "diamond";
const INITIAL_BANK_PRICES: UpdateBankPrices = UpdateBankPrices {
    withdraw_flat: 1000,
    withdraw_per_stack: 40,
    expedited: 1000,
};
const DEFAULT_STACK: u64 = 64;

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
    /// Player asked to withdrew assets
    WithdrawlRequested {
        player: PlayerId,
        assets: std::collections::BTreeMap<AssetId,u64>
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
    BuyOrder {
        player: PlayerId,
        asset: AssetId,
        count: u64,
        coins_per: u64,
    },
    /// Player offers to sell assets at a price
    SellOrder {
        player: PlayerId,
        asset: AssetId,
        count: u64,
        coins_per: u64,
    },
    Donation {
        asset: AssetId,
        count: u64,
        banker: PlayerId,
    },
    UpdateRestricted {
        restricted_assets: Vec<AssetId>
    },
    AuthoriseRestricted {
        authorisee: PlayerId,
        authoriser: PlayerId,
        asset: AssetId,
        new_count: u64
    },
    UpdateBankPrices {
        withdraw_flat: u64,
        withdraw_per_stack: u64,
        expedited: u64
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
    // A transfer of coins from one player to another, no strings attached
    TransferCoins {
        payer: PlayerId,
        payee: PlayerId,
        count: u64
    },
    // A transfer of items from one player to another, no strings attached
    TransferAsset {
        payer: PlayerId,
        payee: PlayerId,
        asset: AssetId,
        count: u64
    },
    CancelOrder {
        target_id: u64
    },
    UpdateBankers {
        bankers: Vec<PlayerId>
    },
    WithdrawProfit {
        n_diamonds: u64,
        banker: PlayerId
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct WrappedAction {
    // The id of the action, which should equal the line number of the trades list
    id: u64,
    // The time this action was performed
    time: chrono::DateTime<chrono::Utc>,
    // The action itself
    action: Action,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum OrderType {
    Buy,
    Sell
}
impl std::fmt::Display for OrderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderType::Buy => write!(f, "buy"),
            OrderType::Sell => write!(f, "sell"),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct PendingOrder {
    pub id: u64,
    pub coins_per: u64,
    pub player: Option<PlayerId>,
    pub amount_remaining: u64,
    pub asset: AssetId,
    pub order_type: OrderType
}

#[derive(Debug, Clone)]
pub struct PendingWithdrawl {
    pub player: PlayerId,
    pub assets: std::collections::BTreeMap<AssetId, u64>,
    pub expedited: bool,
    pub total_fee: u64
}
#[derive(Debug)]
pub enum StateApplyError {
    Overdrawn{
        /// If None, then it's coins overdrawn
        asset: Option<AssetId>, 
        amount_overdrawn: u64
    },
    UnauthorisedWithdrawl{asset: AssetId, amount_overdrawn: Option<u64>},
    /// Some 1337 hacker tried an overflow attack >:(
    Overflow,
    InvalidId{id: u64}
}
impl std::fmt::Display for StateApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateApplyError::Overdrawn { asset, amount_overdrawn } => {
                match asset {
                    Some(asset) => write!(f, "Player needs {amount_overdrawn} more {asset} to perform this action"),
                    None => write!(f, "Player needs {amount_overdrawn} more coins to perform this action")
                }
            }
            StateApplyError::UnauthorisedWithdrawl { asset, amount_overdrawn } => {
                match amount_overdrawn {
                    Some(amount_overdrawn) => write!(f, "Player needs authorisation to withdraw {amount_overdrawn} more {asset}"),
                    None => write!(f, "Player needs authorisation to withdraw {asset}")
                }
                
            },
            StateApplyError::Overflow => {
                write!(f, "The request was so messed up it could have overflowed!")
            },
            StateApplyError::InvalidId { id } => {
                write!(f, "The action ID {id} was invalid")
            }
        }
        
    }
}
impl std::error::Error for StateApplyError {}
#[derive(Debug)]
struct UpdateBankPrices {
    withdraw_flat: u64,
    withdraw_per_stack: u64,
    expedited: u64
}

#[derive(Debug)]
pub struct State {
    next_id: u64,
    stack_sizes: std::collections::HashMap<AssetId, u64>,
    fees: UpdateBankPrices,

    restricted_assets: std::collections::HashSet<AssetId>,

    // buy_outstanding: std::collections::HashMap<AssetId, std::collections::BinaryHeap<BuyOrder>>,
    // sell_outstanding: std::collections::HashMap<AssetId, std::collections::BinaryHeap<SellOrder>>,

    orders: std::collections::BTreeMap<u64, PendingOrder>,

    /// XXX: this contains cancelled orders, skip over them
    best_buy: std::collections::HashMap<AssetId, std::collections::BTreeMap<u64, std::collections::VecDeque<u64>>>,
    /// XXX: this contains cancelled orders, skip over them
    best_sell: std::collections::HashMap<AssetId, std::collections::BTreeMap<u64, std::collections::VecDeque<u64>>>,

    balances: std::collections::HashMap<PlayerId, u64>,
    assets: std::collections::HashMap<PlayerId, std::collections::BTreeMap<AssetId, u64>>,
    authorisations: std::collections::HashMap<PlayerId, std::collections::BTreeMap<AssetId, u64>>,
    pending_normal_withdrawals: std::collections::BTreeMap<u64, PendingWithdrawl>,
    pending_expedited_withdrawals: std::collections::BTreeMap<u64, PendingWithdrawl>,

    profit: u64,
    earnings: std::collections::HashMap<PlayerId, u64>,
    bankers: std::collections::HashSet<PlayerId>
    // futures: std::collections::BTreeMap<i64, Action::Future>
}
impl State {
    /// Get a player's balance
    pub fn get_bal(&self, player: &PlayerId) -> u64 {
        self.balances.get(player).map_or(0, Clone::clone)
    }
    /// Get a player's assets
    pub fn get_assets(&self, player: &PlayerId) -> std::collections::BTreeMap<AssetId, u64> {
        self.assets.get(player).map_or_else(Default::default, Clone::clone)
    }
    /// Create a new empty state
    pub fn new() -> State {
        State{
            stack_sizes: Default::default(),
            fees: INITIAL_BANK_PRICES,
            restricted_assets: Default::default(),
            orders: Default::default(),
            best_buy: Default::default(),
            best_sell: Default::default(),
            balances: Default::default(),
            assets: Default::default(),
            authorisations: Default::default(),
            pending_normal_withdrawals: Default::default(),
            pending_expedited_withdrawals: Default::default(),
            earnings: Default::default(),
            // Start on ID 1 for nice mapping to line numbers
            next_id: 1,
            bankers: Default::default(),
            profit: 0,
        }
    }
    /// Calculate the withdrawal fees
    pub fn calc_withdrawal_fee(&self, assets: &std::collections::BTreeMap<AssetId, u64>) -> Result<u64, StateApplyError> {
        let mut total_fee = self.fees.withdraw_flat;
        for (asset, count) in assets {
            total_fee += count.div_ceil(*self.stack_sizes.get(asset).unwrap_or(&DEFAULT_STACK)).checked_mul(self.fees.withdraw_per_stack).ok_or(StateApplyError::Overflow)?;
        }
        Ok(total_fee)
    }
    /// Get the expedite fee
    pub fn expedite_fee(&self) -> u64 { self.fees.expedited }
    /// List all expedited
    pub fn get_expedited_withdrawals(&self) -> std::collections::BTreeMap<u64, PendingWithdrawl> { self.pending_expedited_withdrawals.clone() }
    /// List all non-expedited withdrawals
    pub fn get_normal_withdrawals(&self) -> std::collections::BTreeMap<u64, PendingWithdrawl> { self.pending_normal_withdrawals.clone() }
    /// List all withdrawals
    pub fn get_withdrawals(&self) -> std::collections::BTreeMap<u64, PendingWithdrawl> { 
        let mut ret = self.pending_normal_withdrawals.clone();
        ret.extend(self.get_expedited_withdrawals());
        ret
    }
    /// Get the withdrawal the bankers should examine next
    pub fn get_next_withdrawal(&self) -> Option<PendingWithdrawl> {
        self.pending_expedited_withdrawals.values().next().or_else(|| self.pending_normal_withdrawals.values().next()).cloned()
    }
    /// List all orders
    pub fn get_orders(&self) -> std::collections::BTreeMap<u64, PendingOrder> { self.orders.clone() }
    /// Get a specific order
    pub fn get_order(&self, id: u64) -> Option<PendingOrder> { self.orders.get(&id).cloned() }
    /// Prices for an asset, returns (price, amount) in (buy, sell)
    pub fn get_prices(&self, asset: &AssetId) -> (std::collections::BTreeMap<u64, u64>, std::collections::BTreeMap<u64, u64>) {
        let buy_levels = self.best_buy
            .get(asset)
            .iter()
            .flat_map(|x| x.iter())
            .filter_map(|(level, orders)| {
                orders
                    .iter()
                    .cloned()
                    .filter_map(|id| self.orders.get(&id).map(|x| x.amount_remaining))
                    // We have None here iff there are no non-canceled orders
                    .reduce(|a,b| a+b)
                    .map(|amount| (*level, amount))
            })
            .collect();

        let sell_levels = self.best_sell
            .get(asset)
            .iter()
            .flat_map(|x| x.iter())
            .filter_map(|(level, orders)| {
                orders
                    .iter()
                    .cloned()
                    .filter_map(|id| self.orders.get(&id).map(|x| x.amount_remaining))
                    // We have None here iff there are no non-canceled orders
                    .reduce(|a,b| a+b)
                    .map(|amount| (*level, amount))
            })
            .collect();
        
        (buy_levels, sell_levels)
    }
    /// Returns true if the given item is currently restricted
    pub fn is_restricted(&self, asset: &AssetId) -> bool { self.restricted_assets.contains(asset) }
    /// Lists all restricted items
    pub fn get_restricted(&self) -> impl Iterator<Item = &AssetId> { self.restricted_assets.iter() }
    /// Gets a list of all bankers
    pub fn get_bankers(&self) -> HashSet<PlayerId> { self.bankers.clone() }
    /// Returns true if the given player is an banker
    pub fn is_banker(&self, player: &PlayerId) -> bool { self.bankers.contains(player) }
    /// Check if a player can afford to give up assets
    fn check_asset_removal(&self, player: &PlayerId, asset: &str, count: u64) -> Result<(), StateApplyError> {
        // If the player doesn't have an account, they definitely cannot withdraw
        let Some(tgt) = self.assets.get(player) 
        else { return Err(StateApplyError::Overdrawn { asset: Some(asset.to_string()), amount_overdrawn: count }); };

        // If they aren't listed for an asset, they definitely cannot withdraw
        let Some(tgt) = tgt.get(asset) 
        else { return Err(StateApplyError::Overdrawn { asset: Some(asset.to_string()), amount_overdrawn: count }); };

        // If they don't have enough, they cannot withdraw
        if *tgt < count  {
            return Err(StateApplyError::Overdrawn { asset: Some(asset.to_string()), amount_overdrawn: count - *tgt });
        }
        Ok(())
    }
    /// Decreases a player's asset count, but only if they can afford it
    fn commit_asset_removal(&mut self, player: &PlayerId, asset: &str, count: u64) -> Result<(), StateApplyError> {
        // If the player doesn't have an account, they definitely cannot withdraw
        let Some(assets) = self.assets.get_mut(player) 
        else { return Err(StateApplyError::Overdrawn { asset: Some(asset.to_string()), amount_overdrawn: count }); };

        // If they aren't listed for an asset, they definitely cannot withdraw
        let Some(tgt) = assets.get_mut(asset) 
        else { return Err(StateApplyError::Overdrawn { asset: Some(asset.to_string()), amount_overdrawn: count }); };

        // If they don't have enough, they cannot withdraw
        if *tgt < count  {
            return Err(StateApplyError::Overdrawn { asset: Some(asset.to_string()), amount_overdrawn: count - *tgt });
        }

        // Take away their assets
        *tgt -= count;
        // If it's zero, clean up
        if *tgt == 0 {
            assets.remove(asset);
            if assets.is_empty() {
                self.assets.remove(player);
            }
        }
        Ok(())
    }
    /// Check if a player can afford to pay
    fn check_coin_removal(&self, player: &PlayerId, count: u64) -> Result<(), StateApplyError> {
        // If the player doesn't have an account, they definitely cannot withdraw
        let Some(tgt) = self.balances.get(player) 
        else { return Err(StateApplyError::Overdrawn { asset: None, amount_overdrawn: count }); };

        // If they don't have enough, they cannot withdraw
        if *tgt < count {
            return Err(StateApplyError::Overdrawn { asset: None, amount_overdrawn: count - *tgt });
        }
        Ok(())
    }
    /// Decreases a player's coin count, but only if they can afford it
    fn commit_coin_removal(balances: &mut std::collections::HashMap<PlayerId, u64>, player: &PlayerId, count: u64) -> Result<(), StateApplyError> {
        // If the player doesn't have an account, they definitely cannot withdraw
        let Some(tgt) = balances.get_mut(player) 
        else { return Err(StateApplyError::Overdrawn { asset: None, amount_overdrawn: count }); };

        // If they don't have enough, they cannot withdraw
        if *tgt < count {
            return Err(StateApplyError::Overdrawn { asset: None, amount_overdrawn: count - *tgt });
        }
        
        // Take away their coins
        *tgt -= count;

        // If it's zero, clean up
        if *tgt == 0 {
            balances.remove(player);
        }
        Ok(())
    }
    // Atomic (but not parallelisable!). 
    // This means the function will change significant things (i.e. more than just creating empty lists) IF AND ONLY IF it fully succeeds.
    // As such, we don't have to worry about giving it bad actions
    fn apply_inner(&mut self, id: u64, action: Action) -> Result<(), StateApplyError> {
        match action {
            Action::Deleted{..} => Ok(()),
            Action::Deposit { player, asset, count, .. } => {
                // Create an entry for this player and asset if one doesn't exist, and get a reference to its value
                let tgt = self.assets.entry(player).or_default().entry(asset).or_default();
                // Give them the items
                *tgt += count;

                Ok(())
            },
            Action::WithdrawlRequested { player, assets} => {
                let total_fee = self.calc_withdrawal_fee(&assets)?;

                let mut tracked_assets: std::collections::BTreeMap<AssetId, u64> = Default::default();
                // There's no good way of doing this without two passes, so we check then commit
                //
                // BTreeMap ensures that the same asset cannot occur twice, so we don't have to worry about double spending
                for (asset, count) in assets {
                    // Check to see if they can afford it
                    self.check_asset_removal(&player, &asset, count)?;
                    let is_restricted = self.is_restricted(&asset);
                    // Check if restricted
                    if is_restricted {
                        // If it is restricted, we have to check before we take their assets
                        // Check if they are authorised to withdraw any amount of these items
                        let Some(auth_amount) = self.authorisations.get(&player).and_then(|x| x.get(&asset))
                        else { return Err(StateApplyError::UnauthorisedWithdrawl{ asset: asset.clone(), amount_overdrawn: None}); };
                        // Check if they are authorised to withdraw at least this many items
                        if *auth_amount < count {
                            return Err(StateApplyError::UnauthorisedWithdrawl{ asset: asset.clone(), amount_overdrawn: Some(count - *auth_amount)});
                        }
                    }
                    tracked_assets.insert(asset, count);
                }
                // Check they can afford the fee, and if they can, take it
                Self::commit_coin_removal(&mut self.balances, &player, total_fee)?;

                // Now take the assets, as we've confirmed they can afford it
                for (asset, count) in tracked_assets.iter() {
                    // Remove assets
                    self.commit_asset_removal(&player, asset, *count).expect("Assets disappeared after check");
                    // Remove allowance if restricted
                    if self.is_restricted(asset) {
                        // TODO: Clean up after ourselves
                        *self.authorisations.get_mut(&player).expect("Asset player disappeared after check")
                                            .get_mut(asset).expect("Asset auth disappeared after check") -= count;
                    }
                }

                // Register the withdrawal. This cannot fail, so we don't have to worry about atomicity
                self.pending_normal_withdrawals.insert(id, PendingWithdrawl{ player, assets: tracked_assets, expedited: false, total_fee });
                Ok(())
            },
            Action::SellOrder { player, asset, count, coins_per } => {
                // Check and take their assets first
                self.commit_asset_removal(&player, &asset, count)?;
                let player_balance = self.balances.entry(player.clone()).or_default();

                // Perform immediate fulfillments. This cannot fail, so we don't have to worry about atomicity
                let mut amount_remaining = count;
                if let Some(best_asset_buys) = self.best_buy.get_mut(&asset) {
                    // Loop until we can't take away any more assets
                    //
                    // If we fulfill the entire order, we *return* out of the loop instead of breaking:
                    // breaking means that we still need to track it
                    while amount_remaining > 0 {
                        let Some(mut best_entry) = best_asset_buys.last_entry()
                        else {break;};
                        // If we are out of buy orders, stop
                        let (buy_order, buy_order_id) = {
                            let buy_id = *best_entry.get_mut().front().expect("Empty best_buy not cleaned up");
                            if let Some(order) = self.orders.get_mut(&buy_id) {
                                (order, buy_id)
                            }
                            // Keep looping till we find a buy order we can actually use
                            else { continue; }
                        };
                        // If this is less than the seller is willing to take, stop, as all further ones will be too.
                        if buy_order.coins_per < coins_per { break; }

                        // Note that, unlike in the BuyOrder case, we do not need to return funds for favourable fulfillments, as we didn't take any initially
                        match buy_order.amount_remaining.cmp(&amount_remaining) {
                            // If the buy order is not enough...
                            std::cmp::Ordering::Less => {
                                // ... give the money ...
                                *player_balance += buy_order.amount_remaining * buy_order.coins_per;
                                // ... give the assets ...
                                *self.assets.entry(buy_order.player.as_ref().expect("Buy order without player").clone()).or_default().entry(asset.clone()).or_default() += buy_order.amount_remaining;
                                // ... remove the amount ...
                                amount_remaining -= buy_order.amount_remaining;
                                // ... delete the buy order ...
                                self.orders.remove(&buy_order_id);
                                best_entry.get_mut().pop_front();
                                // ... clean up if empty
                                if best_entry.get().is_empty() { best_entry.remove(); }
                                // We still have potentially more fulfullments
                                continue;
                            },
                            // If the buy order is exactly enough...
                            std::cmp::Ordering::Equal => {
                                // ... give the money ...
                                *player_balance += buy_order.amount_remaining * buy_order.coins_per;
                                // ... give the assets ...
                                *self.assets.entry(buy_order.player.as_ref().expect("Buy order without player").clone()).or_default().entry(asset.clone()).or_default() += buy_order.amount_remaining;
                                // ... delete the buy order ...
                                best_entry.get_mut().pop_front();
                                self.orders.remove(&buy_order_id);
                                // ... clean up if empty
                                if best_entry.get().is_empty() { best_entry.remove(); }
                                // We've fulfilled the whole sell order, so we can just return
                                return Ok(());
                            }
                            // If the buy order is more than enough...
                            std::cmp::Ordering::Greater => {
                                // ... give the money ...
                                *player_balance += amount_remaining * buy_order.coins_per;
                                // ... give the assets ...
                                *self.assets.entry(buy_order.player.as_ref().expect("Buy order without player").clone()).or_default().entry(asset.clone()).or_default() += amount_remaining;
                                // ... reduce the buy order
                                buy_order.amount_remaining -= amount_remaining;
                                // We've fulfilled the whole sell order, so we can just return
                                return Ok(());
                            }
                        }
                    }
                    // If we didn't return early, that means we still need more items
                }
                self.best_sell.entry(asset.clone()).or_default().entry(coins_per).or_default().push_back(id);
                self.orders.insert(id, PendingOrder{ id, coins_per, player: Some(player), amount_remaining, asset, order_type: OrderType::Sell });
                Ok(())
            },
            Action::BuyOrder { player, asset, count, coins_per } => {
                // Check and take their money first
                Self::commit_coin_removal(&mut self.balances, &player, count.checked_mul(coins_per).ok_or(StateApplyError::Overflow)?)?;
                let player_balance = self.balances.entry(player.clone()).or_default();
                let player_assets = self.assets.entry(player.clone()).or_default();
                let player_asset_count = player_assets.entry(asset.clone()).or_default();

                // Perform immediate fulfillments. This cannot fail, so we don't have to worry about atomicity
                let mut amount_remaining = count;
                if let Some(best_asset_sells) = self.best_sell.get_mut(&asset) {
                    // Loop until we can't take away any more assets
                    //
                    // If we fulfill the entire order, we *return* out of the loop instead of breaking:
                    // breaking means that we still need to track it
                    while amount_remaining > 0 {
                        let Some(mut best_entry) = best_asset_sells.first_entry()
                        else {break;};
                        // If we are out of buy orders, stop
                        let (sell_order, sell_order_id) = {
                            let sell_id = *best_entry.get_mut().front().expect("Empty best_buy not cleaned up");
                            if let Some(order) = self.orders.get_mut(&sell_id) {
                                (order, sell_id)
                            }
                            // Keep looping till we find a buy order we can actually use
                            else { continue; }
                        };
                        // If this is more than the buyer is willing to pay, stop, as all further ones will be too.
                        if sell_order.coins_per > coins_per { break; }

                        match sell_order.amount_remaining.cmp(&amount_remaining) {
                            // If the sell order is not enough...
                            std::cmp::Ordering::Less => {
                                // ... give the assets ...
                                *player_asset_count += sell_order.amount_remaining;
                                // ... if they bought it cheap, give them the difference ...
                                *player_balance += sell_order.amount_remaining * (coins_per - sell_order.coins_per);
                                // ... remove the amount ...
                                amount_remaining -= sell_order.amount_remaining;
                                // ... delete the sell order ...
                                self.orders.remove(&sell_order_id);
                                best_entry.get_mut().pop_front();
                                // ... clean up if empty
                                if best_entry.get().is_empty() { best_entry.remove(); }
                                // We still have potentially more fulfullments
                                continue;
                            },
                            // If the buy order is exactly enough...
                            std::cmp::Ordering::Equal => { 
                                // ... give the assets ...
                                *player_asset_count += sell_order.amount_remaining;
                                // ... if they bought it cheap, give them the difference ...
                                *player_balance += sell_order.amount_remaining * (coins_per - sell_order.coins_per);
                                // ... delete the sell order ...
                                self.orders.remove(&sell_order_id);
                                best_entry.get_mut().pop_front();
                                // ... clean up if empty
                                if best_entry.get().is_empty() { best_entry.remove(); }
                                // We've fulfilled the whole buy order, so we can just return
                                return Ok(());
                            }
                            // If the buy order is more than enough...
                            std::cmp::Ordering::Greater => {
                                // ... give the money ...
                                *player_balance += amount_remaining * sell_order.coins_per;
                                // ... if they bought it cheap, give them the difference ...
                                *player_balance += amount_remaining * (coins_per - sell_order.coins_per);
                                // ... reduce the sell order
                                sell_order.amount_remaining -= amount_remaining;
                                // We've fulfilled the whole buy order, so we can just return
                                return Ok(());
                            }
                        }
                    }
                    // If we didn't return early, that means we still need more items
                }
                
                // Clean up
                if *player_asset_count == 0 {
                    player_assets.remove(&asset);
                }
                self.best_buy.entry(asset.clone()).or_default().entry(coins_per).or_default().push_back(id);
                self.orders.insert(id, PendingOrder{ id, coins_per, player: Some(player), amount_remaining, asset, order_type: OrderType::Buy });
                Ok(())
            },
            Action::WithdrawlCompleted { target, banker } => {
                // Try to take out the pending transaction
                let Some(res) = self.pending_normal_withdrawals.remove(&target).or_else(|| self.pending_expedited_withdrawals.remove(&target))
                else { return Err(StateApplyError::InvalidId{id: target}); };
                // Mark who delivered
                *self.earnings.entry(banker).or_default() += res.total_fee;
                // Add the profit
                self.profit += res.total_fee;
                Ok(())
            },
            Action::CancelOrder { target_id } => {
                if let Some(found) = self.orders.remove(&target_id) {
                    match found.order_type {
                        // If we found it as a buy...
                        OrderType::Buy => {
                            // ... refund the money ...
                            *self.balances.entry(found.player.expect("Buy order found without player")).or_default() += found.amount_remaining * found.coins_per;
                            Ok(())
                        },
                        // If we found it as a sell...
                        OrderType::Sell => {
                            // ... refund the assets
                            *self.assets
                                .entry(found.player.expect("Sell order cancelled without player")).or_default()
                                .entry(found.asset).or_default() += found.amount_remaining * found.coins_per;
                            Ok(())
                        }
                    }
                }
                // If we didn't find it, it was invalid
                else {
                    Err(StateApplyError::InvalidId{id: target_id})
                }
            },
            Action::BuyCoins { player, n_diamonds } => {
                // Check and take diamonds from payer...
                self.commit_asset_removal(&player,  DIAMOND_NAME, n_diamonds)?;
                // ... and give them the coins
                *self.balances.entry(player).or_default() += n_diamonds * COINS_PER_DIAMOND;
                Ok(())
            },
            Action::SellCoins { player, n_diamonds } => {
                // Check and take coins from payer...
                Self::commit_coin_removal(&mut self.balances, &player, n_diamonds.checked_mul(COINS_PER_DIAMOND).ok_or(StateApplyError::Overflow)?)?;
                // ... and give them the coins
                *self.balances.entry(player).or_default() += n_diamonds * COINS_PER_DIAMOND;
                Ok(())
            },
            Action::Donation { asset, count, .. } => {
                // Nothing here can fail, so we can write code nicely :)

                // Perform immediate fulfillments. This cannot fail, so we don't have to worry about atomicity
                let mut amount_remaining = count;
                if let Some(best_asset_buys) = self.best_buy.get_mut(&asset) {
                    // Loop until we can't take away any more assets
                    //
                    // If we fulfill the entire donation, we *return* out of the loop instead of breaking:
                    // breaking means that we still need to track it
                    while amount_remaining > 0 {
                        let Some(mut best_entry) = best_asset_buys.last_entry()
                        else {break;};
                        // If we are out of buy orders, stop
                        let (buy_order, buy_order_id) = {
                            let buy_id = *best_entry.get_mut().front().expect("Empty best_buy not cleaned up");
                            if let Some(order) = self.orders.get_mut(&buy_id) {
                                (order, buy_id)
                            }
                            // Keep looping till we find a buy order we can actually use
                            else { continue; }
                        };

                        let buyer = buy_order.player.as_ref().expect("Buy order without player").clone();
                        let player_balance = self.balances.entry(buyer).or_default();

                        // We need to return funds, as this is likely below their price
                        match buy_order.amount_remaining.cmp(&amount_remaining) {
                            // If the buy order is not enough...
                            std::cmp::Ordering::Less => {
                                // ... give back the money ...
                                *player_balance += buy_order.amount_remaining.checked_mul(buy_order.coins_per).ok_or(StateApplyError::Overflow)?;
                                // ... remove the amount ...
                                amount_remaining -= buy_order.amount_remaining;
                                // ... delete the buy order ...
                                self.orders.remove(&buy_order_id);
                                best_entry.get_mut().pop_front();
                                // ... clean up if empty
                                if best_entry.get().is_empty() { best_entry.remove(); }
                                // We still have potentially more fulfullments
                                continue;
                            },
                            // If the buy order is exactly enough...
                            std::cmp::Ordering::Equal => {
                                // ... give back the money ...
                                *player_balance += buy_order.amount_remaining.checked_mul(buy_order.coins_per).ok_or(StateApplyError::Overflow)?;
                                // ... delete the buy order ...
                                self.orders.remove(&buy_order_id);
                                best_entry.get_mut().pop_front();
                                // ... clean up if empty
                                if best_entry.get().is_empty() { best_entry.remove(); }
                                // We've fulfilled the whole sell order, so we can just return
                                return Ok(());
                            }
                            // If the buy order is more than enough...
                            std::cmp::Ordering::Greater => {
                                // ... give back the money ...
                                *player_balance += amount_remaining * buy_order.coins_per;
                                // ... reduce the buy order
                                buy_order.amount_remaining -= amount_remaining;
                                // We've fulfilled the whole sell order, so we can just return
                                return Ok(());
                            }
                        }
                    }
                    // If we didn't return early, that means we still need more items
                }

                self.best_sell.entry(asset.clone()).or_default().entry(0).or_default().push_back(id);
                self.orders.insert(id, PendingOrder{ id, coins_per: 0, player: None, amount_remaining, asset, order_type: OrderType::Sell });

                Ok(())
            },
            Action::UpdateRestricted { restricted_assets } => {
                self.restricted_assets = std::collections::HashSet::from_iter(restricted_assets);
                Ok(())
            },
            Action::AuthoriseRestricted { authorisee, asset, new_count, .. } => {
                self.authorisations.entry(authorisee).or_default().insert(asset, new_count);
                Ok(())
            },
            Action::UpdateBankPrices { withdraw_flat, withdraw_per_stack, expedited } => {
                self.fees = UpdateBankPrices{ withdraw_flat, withdraw_per_stack, expedited };
                Ok(())
            },
            Action::TransferCoins { payer, payee, count } => {
                // Check and take money from payer...
                Self::commit_coin_removal(&mut self.balances, &payer, count)?;
                // ... and give it to payee
                *self.balances.entry(payee).or_default() += count;
                Ok(())
            },
            Action::TransferAsset { payer, payee, asset, count } => {
                // Check and take assets from payer...
                self.commit_asset_removal(&payer, &asset, count)?;
                // ... and give it to payee
                *self.assets.entry(payee).or_default().entry(asset).or_default() += count;
                Ok(())
            }
            Action::Expedited { target } => {
                // Try to find this withdrawal
                let std::collections::btree_map::Entry::Occupied(entry) = self.pending_normal_withdrawals.entry(target)
                else { return Err(StateApplyError::InvalidId { id: target }); };
                // Take the coins away
                Self::commit_coin_removal(&mut self.balances, &entry.get().player, self.fees.expedited)?;
                // Remove them from the normal list
                let mut entry = entry.remove();
                // Give them the expedited flag, and track the money
                entry.expedited = true;
                entry.total_fee += self.fees.expedited;
                // Insert them into the expedited list
                self.pending_expedited_withdrawals.insert(target, entry);
                Ok(())
            },
            Action::UpdateBankers { bankers } => {
                self.bankers = std::collections::HashSet::from_iter(bankers);
                Ok(())
            },
            Action::WithdrawProfit { n_diamonds, .. } => {
                self.profit -= n_diamonds * COINS_PER_DIAMOND;
                Ok(())
            }
        }
    }
    /// Load in the transactions from a trade file. Because of numbering, we must do this first; we cannot append
    pub async fn replay(trade_file: &mut (impl tokio::io::AsyncRead + std::marker::Unpin)) -> Result<State, StateApplyError> {
        let mut state = Self::new();

        let trade_file_reader = tokio::io::BufReader::new(trade_file);
        let mut trade_file_lines = trade_file_reader.lines();
        while let Some(line) = trade_file_lines.next_line().await.expect("Could not read line from trade list") {
            let wrapped_action: WrappedAction = serde_json::from_str(&line).expect("Corrupted trade file");
            if wrapped_action.id != state.next_id {
                panic!("Trade file ID mismatch: action {} found on line {}", wrapped_action.id, state.next_id);
            }
            state.apply_inner(state.next_id, wrapped_action.action)?;
            state.next_id += 1;
        }
        Ok(state)
    }
    /// Atomically try to apply an action, and if successful, write to given stream
    pub async fn apply(&mut self, action: Action, out: &mut (impl tokio::io::AsyncWrite + std::marker::Unpin)) -> Result<u64, StateApplyError> {
        let id = self.next_id;
        let wrapped_action = WrappedAction {
            id,
            time: chrono::offset::Utc::now(),
            action: action.clone(),
        };
        let mut line = serde_json::to_string(&wrapped_action).expect("Cannot serialise action");
        line.push('\n');
        match self.apply_inner(self.next_id, action).map(|()| {}) {
            Ok(()) => {
                self.next_id += 1;
                out.write_all(line.as_bytes()).await.expect("Could not write to log, must immediately stop!");
                Ok(id)
            }
            Err(e) => {
                Err(e)
            }
        }
    }
}