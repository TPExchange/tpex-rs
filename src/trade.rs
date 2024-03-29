use std::{collections::HashSet, ops::Mul};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

// We use a base coins, which represent 1/1000 of a diamond
use serde::{Deserialize, Serialize, ser::SerializeMap};

#[derive(PartialEq, PartialOrd, Eq, Ord, Default, Debug, Clone, Hash)]
pub struct PlayerId(String);
impl PlayerId {
    #[deprecated = "Do not use this, use player_id instead"]
    pub(crate) fn evil_constructor(s: String) -> PlayerId { PlayerId(s) }
    #[deprecated = "Do not use this, use user_id instead"]
    pub(crate) fn evil_deref(&self) -> &String { &self.0 }
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

pub const COINS_PER_DIAMOND: u64 = 1000;
pub const DIAMOND_NAME: &str = "diamond";
const INITIAL_BANK_PRICES: UpdateBankPrices = UpdateBankPrices {
    withdraw_flat: 1000,
    withdraw_per_stack: 50,
    expedited: 5000,
    investment_share: 0.5,
    instant_smelt_per_stack: 100
};

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
        expedited: u64,
        investment_share: f64,
        instant_smelt_per_stack: u64
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
    UpdateInvestables {
        assets: Vec<AssetId>
    },
    Invest {
        player: PlayerId,
        asset: AssetId,
        count: u64
    },
    Uninvest {
        player: PlayerId,
        asset: AssetId,
        count: u64
    },
    UpdateConvertables {
        convertables: Vec<(AssetId, AssetId)>
    },
    InstantConvert {
        player: PlayerId,
        from: AssetId,
        to: AssetId,
        count: u64
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

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct AssetInfo {
    stack_size: u64
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize)]pub enum OrderType {
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

#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
pub struct PendingOrder {
    pub id: u64,
    pub coins_per: u64,
    pub player: PlayerId,
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
    InvalidId{id: u64},
    UnknownAsset{asset: AssetId},
    NotInvestable{asset: AssetId},
    InvestmentBusy{asset: AssetId, amount_over: u64},
    NotConvertable{from: AssetId, to: AssetId}
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
            StateApplyError::UnknownAsset { asset } => {
                write!(f, "The item \"{asset}\" is not on our list")
            },
            StateApplyError::NotInvestable { asset } => {
                write!(f, "The item \"{asset}\" is not investable yet")
            },
            StateApplyError::InvestmentBusy { asset, amount_over} => {
                write!(f, "The action failed, as we would need {amount_over} more invested {asset}")
            },
            StateApplyError::NotConvertable { from, to } => {
                write!(f, "We do not currently offer conversion from {from} to {to}")
            },
        }
        
    }
}
impl std::error::Error for StateApplyError {}
#[derive(Debug, Serialize)]
struct UpdateBankPrices {
    withdraw_flat: u64,
    withdraw_per_stack: u64,
    expedited: u64,
    investment_share: f64,
    instant_smelt_per_stack: u64
}

#[derive(Debug, Serialize)]
struct Investment {
    player: PlayerId,
    asset: AssetId,
    count: u64
}

#[derive(Debug)]
pub struct State {
    next_id: u64,
    asset_info: std::collections::HashMap<AssetId, AssetInfo>,
    fees: UpdateBankPrices,
    convertables: std::collections::HashSet<(AssetId, AssetId)>,

    restricted_assets: std::collections::HashSet<AssetId>,
    authorisations: std::collections::HashMap<PlayerId, std::collections::BTreeMap<AssetId, u64>>,
    investables: std::collections::HashSet<AssetId>,

    orders: std::collections::BTreeMap<u64, PendingOrder>,

    /// XXX: this contains cancelled orders, skip over them
    best_buy: std::collections::HashMap<AssetId, std::collections::BTreeMap<u64, std::collections::VecDeque<u64>>>,
    /// XXX: this contains cancelled orders, skip over them
    best_sell: std::collections::HashMap<AssetId, std::collections::BTreeMap<u64, std::collections::VecDeque<u64>>>,

    // These two tables must be kept consistent
    asset_investments: std::collections::HashMap<AssetId, std::collections::HashMap<PlayerId, u64>>,
    player_investments: std::collections::HashMap<PlayerId, std::collections::HashMap<AssetId, u64>>,
    investment_busy: std::collections::HashMap<AssetId, u64>,

    balances: std::collections::HashMap<PlayerId, u64>,
    assets: std::collections::HashMap<PlayerId, std::collections::BTreeMap<AssetId, u64>>,
    pending_normal_withdrawals: std::collections::BTreeMap<u64, PendingWithdrawl>,
    pending_expedited_withdrawals: std::collections::BTreeMap<u64, PendingWithdrawl>,

    earnings: std::collections::HashMap<PlayerId, u64>,
    bankers: std::collections::HashSet<PlayerId>
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
    pub fn new(asset_info: std::collections::HashMap<AssetId, AssetInfo>) -> State {
        State{
            asset_info,
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
            asset_investments: Default::default(),
            player_investments: Default::default(),
            investables: Default::default(),
            investment_busy: Default::default(),
            convertables: Default::default(),
        }
    }
    /// Calculate the withdrawal fees
    pub fn calc_withdrawal_fee(&self, assets: &std::collections::BTreeMap<AssetId, u64>) -> Result<u64, StateApplyError> {
        let mut total_fee = self.fees.withdraw_flat;
        for (asset, count) in assets {
            total_fee += count.div_ceil(self.asset_info.get(asset).ok_or(StateApplyError::UnknownAsset{asset:asset.clone()})?.stack_size)
                              .checked_mul(self.fees.withdraw_per_stack).ok_or(StateApplyError::Overflow)?;
        }
        Ok(total_fee)
    }
    /// Get the expedite fee
    pub fn expedite_fee(&self) -> u64 { self.fees.expedited }
    /// List all expedited
    pub fn get_expedited_withdrawals(&self) -> std::collections::BTreeMap<u64, PendingWithdrawl> { self.pending_expedited_withdrawals.clone() }
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
    /// Gets info about a certain asset
    pub fn asset_info(&self, asset: &AssetId) -> Result<AssetInfo, StateApplyError> {
        self.asset_info.get(asset).cloned().ok_or_else(|| StateApplyError::UnknownAsset { asset: asset.clone() })
    }
    /// Gets the amount of an asset that is invested
    pub fn amount_invested(&self, asset: &AssetId) -> u64 {
        let Some(investments) = self.asset_investments.get(asset)
        else { return 0; };
        investments.values().sum()
    }
    /// Gives the amount of an investable asset that remains to be lent
    pub fn amount_free(&self, asset: &AssetId) -> u64 {
        self.amount_invested(asset) - self.investment_busy.get(asset).cloned().unwrap_or(0)
    }
    /// Generic function to match buy and sell orders, investments, etc
    fn do_match<T>(count: u64, mut elems: impl Iterator<Item = (u64, T)>) -> (u64, Vec<(u64, T)>) {
        let mut amount_remaining = count;
        let mut ret = Vec::new();
        while amount_remaining > 0 {
            let Some((this_count, data)) = elems.next()
            else {break;};
            match this_count.cmp(&amount_remaining) {
                // If the elem is not enough...
                std::cmp::Ordering::Less => {
                    ret.push((0, data));
                    amount_remaining -= this_count;
                    continue;
                },
                // If the elem is exactly enough...
                std::cmp::Ordering::Equal => {
                    ret.push((0, data));
                    amount_remaining = 0;
                    break;
                }
                // If the elem is more than enough...
                std::cmp::Ordering::Greater => {
                    ret.push((this_count - amount_remaining, data));
                    amount_remaining = 0;
                    break;
                }
            }
        }
        (amount_remaining, ret)        
    }
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
    fn iterate_best_buy<'a>(&'a self, asset: &'a AssetId, limit: u64) -> impl Iterator<Item = u64> + 'a {
        // Get all assets...
        self.best_buy
            // ... only look at the asset in question ...
            .get(asset)
            .into_iter()
            // ... write out all the levels in order ... 
            .flat_map(|i| i.iter())
            // ... put price points in descending order ...
            .rev()
            // ... only look at offers above the limit ...
            .take_while(move |(price, _)| **price >= limit)
            // ... write out ids within each price point ...
            .flat_map(|(_price, ids)| ids.iter().cloned())
    }
    fn iterate_best_sell<'a>(&'a self, asset: &'a AssetId, limit: u64) -> impl Iterator<Item = u64> + 'a {
        // Get all assets...
        self.best_sell
            // ... only look at the asset in question ...
            .get(asset)
            .into_iter()
            // ... write out all the levels in order ... 
            .flat_map(|i| i.iter())
            // ... price points are already in ascending order ...
            // ... only look at offers below the limit ...
            .take_while(move |(price, _)| **price <= limit)
            // ... write out ids within each price point ...
            .flat_map(|(_price, ids)| ids.iter().cloned())
    }
    fn remove_best(&mut self, asset: AssetId, order_type: OrderType) -> Option<PendingOrder> {
        let target = match order_type { OrderType::Buy => &mut self.best_buy, OrderType::Sell => &mut self.best_sell };

        let std::collections::hash_map::Entry::Occupied(mut asset_class) = target.entry(asset)
        else { panic!("Tried to remove non-existent asset class"); };
        let Some(mut best_level) = (match order_type {
            // Best buy order is the highest
            OrderType::Buy => asset_class.get_mut().last_entry(),
            // Best sell order is the lowest
            OrderType::Sell => asset_class.get_mut().first_entry()
        })
        else { panic!("Empty asset class"); };
        let Some(id) = best_level.get_mut().pop_front()
        else { panic!("Empty price point"); };
        // If it exists, remove the order
        let ret = self.orders.remove(&id);
        // Clean up
        if best_level.get().is_empty() { best_level.remove(); }
        if asset_class.get().is_empty() { asset_class.remove(); }

        ret
    }
    // Check if we can afford to lend
    fn check_busy(&mut self, asset: &AssetId, count: u64) -> Result<(), StateApplyError> {
        let amount_free = self.amount_free(asset);
        if amount_free < count {
            return Err(StateApplyError::InvestmentBusy { asset: asset.clone(), amount_over: count - amount_free })
        }
        Ok(())
    }
    // Check if we can afford to lend, and if so, mark it as lended
    fn commit_busy(&mut self, asset: &AssetId, count: u64) -> Result<(), StateApplyError> {
        let amount_invested = self.amount_invested(asset);
        let amount_busy = self.investment_busy.entry(asset.clone());
        let amount_free = amount_invested - match amount_busy {
            std::collections::hash_map::Entry::Occupied(ref x) => *x.get(),
            _ => 0
        };
        if amount_free < count {
            return Err(StateApplyError::InvestmentBusy { asset: asset.clone(), amount_over: count - amount_free })
        }
        *amount_busy.or_default() += count;
        Ok(())
    }
    // Distribute the profits among the investors
    fn distribute_profit(&mut self, asset: &AssetId, amount: u64) {
        let mut investors = self.asset_investments.get(asset).expect("Somehow got profit from nothing? That's almost definitely wrong").clone();
        // Let's be fair and not give ourselves all the money
        investors.remove(&PlayerId::the_bank());
        let share = (self.fees.investment_share.mul(amount as f64) / (investors.values().sum::<u64>() as f64)).floor() as u64;
        let mut total_distributed = 0;
        for (investor, shares) in investors {
            let investor_profit = share * shares;
            total_distributed += investor_profit;
            *self.balances.entry(investor).or_default() += investor_profit;
        }
        if total_distributed > amount {
            panic!("Profit distribution imprecision was too bad");
        }
        *self.balances.entry(PlayerId::the_bank()).or_default() += amount - total_distributed;
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
                // Then match the orders
                let iter = self.iterate_best_buy(&asset, coins_per)
                    .map(|idx| {
                        match self.orders.get(&idx) {
                            Some(order) => (order.amount_remaining, Some(order.clone())),
                            None => (0, None)
                        }
                    });
                let (amount_remaining, orders) = Self::do_match(count, iter);

                // Handle successful matches
                let mut n_coins_earned = 0;
                for (order_remaining, maybe_order) in orders {
                    let order_amount;
                    let order = {
                        if order_remaining == 0 {
                            // Check to see this wasn't a canceled order
                            if let Some(order_val) = self.remove_best(asset.clone(), OrderType::Buy) {
                                order_amount = order_val.amount_remaining;
                                order_val
                            }
                            else { continue; }
                        }
                        else {
                            let order_ref = self.orders.get_mut(&maybe_order.expect("Partial canceled order").id).expect("Cannot get mut order");
                            order_amount = order_ref.amount_remaining;
                            order_ref.amount_remaining = order_remaining;
                            order_ref.clone()
                        }
                    };
                    // Calculate the amount transferred
                    let order_transferred = order_amount - order_remaining;
                    // ... give the money ...
                    n_coins_earned += order_transferred * order.coins_per;
                    // ... give the assets ...
                    *self.assets.entry(order.player.clone()).or_default().entry(asset.clone()).or_default() += order_transferred;
                }
                // Transfer the money
                *self.balances.entry(player.clone()).or_default() += n_coins_earned;
                
                // If needs be, list the remaining amount
                if amount_remaining > 0 {
                    self.best_sell.entry(asset.clone()).or_default().entry(coins_per).or_default().push_back(id);
                    self.orders.insert(id, PendingOrder{ id, coins_per, player, amount_remaining, asset, order_type: OrderType::Sell });
                }
                
                Ok(())
            },
            Action::BuyOrder { player, asset, count, coins_per } => {
                // Check and take their money first
                Self::commit_coin_removal(&mut self.balances, &player, count.checked_mul(coins_per).ok_or(StateApplyError::Overflow)?)?;
                // Then match the orders
                let iter = self.iterate_best_sell(&asset, coins_per)
                    .map(|idx| {
                        match self.orders.get(&idx) {
                            Some(order) => (order.amount_remaining, Some(order.clone())),
                            None => (0, None)
                        }
                    });
                let (amount_remaining, orders) = Self::do_match(count, iter);

                // Handle successful matches
                let mut n_coins_saved = 0;
                let mut n_asset_bought = 0;
                for (order_remaining, maybe_order) in orders {
                    let order_amount;
                    let order = {
                        if order_remaining == 0 {
                            // Check to see this wasn't a canceled order
                            if let Some(order_val) = self.remove_best(asset.clone(), OrderType::Sell) {
                                order_amount = order_val.amount_remaining;
                                order_val
                            }
                            else { continue; }
                        }
                        else {
                            let order_ref = self.orders.get_mut(&maybe_order.expect("Partial canceled order").id).expect("Cannot get mut order");
                            order_amount = order_ref.amount_remaining;
                            order_ref.amount_remaining = order_remaining;
                            order_ref.clone()
                        }
                    };
                    // Calculate the amount transferred
                    let order_transferred = order_amount - order_remaining;
                    // ... give the assets ...
                    n_asset_bought += order_transferred;
                    // ... if they bought it cheap, give them the difference ...
                    n_coins_saved += order_transferred * (coins_per - order.coins_per);
                }
                // Transfer the money
                *self.balances.entry(player.clone()).or_default() += n_coins_saved;
                // Transfer the assets
                if n_asset_bought > 0 {
                    *self.assets.entry(player.clone()).or_default().entry(asset.clone()).or_default() += n_asset_bought;
                }
                
                // If needs be, list the remaining amount
                if amount_remaining > 0 {
                    self.best_buy.entry(asset.clone()).or_default().entry(coins_per).or_default().push_back(id);
                    self.orders.insert(id, PendingOrder{ id, coins_per, player, amount_remaining, asset, order_type: OrderType::Buy });
                }
                
                Ok(())
            },
            Action::WithdrawlCompleted { target, banker } => {
                // Try to take out the pending transaction
                let Some(res) = self.pending_normal_withdrawals.remove(&target).or_else(|| self.pending_expedited_withdrawals.remove(&target))
                else { return Err(StateApplyError::InvalidId{id: target}); };
                // Mark who delivered
                *self.earnings.entry(banker).or_default() += res.total_fee;
                // Add the profit
                *self.balances.entry(PlayerId::the_bank()).or_default() += res.total_fee;
                Ok(())
            },
            Action::CancelOrder { target_id } => {
                if let Some(found) = self.orders.remove(&target_id) {
                    match found.order_type {
                        // If we found it as a buy...
                        OrderType::Buy => {
                            // ... refund the money ...
                            *self.balances.entry(found.player).or_default() += found.amount_remaining * found.coins_per;
                            Ok(())
                        },
                        // If we found it as a sell...
                        OrderType::Sell => {
                            // ... refund the assets
                            *self.assets
                                .entry(found.player).or_default()
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
                let iter = self.iterate_best_buy(&asset, 0)
                    .map(|idx| {
                        match self.orders.get(&idx) {
                            Some(order) => (order.amount_remaining, Some(order.clone())),
                            None => (0, None)
                        }
                    });
                let (amount_remaining, orders) = Self::do_match(count, iter);
                // Nothing here can fail, so we can write code nicely :)

                // Handle successful matches
                for (order_remaining, maybe_order) in orders {
                    let order_amount;
                    let order = {
                        if order_remaining == 0 {
                            // Check to see this wasn't a canceled order
                            if let Some(order_val) = self.remove_best(asset.clone(), OrderType::Buy) {
                                order_amount = order_val.amount_remaining;
                                order_val
                            }
                            else { continue; }
                        }
                        else {
                            let order_ref = self.orders.get_mut(&maybe_order.expect("Partial canceled order").id).expect("Cannot get mut order");
                            order_amount = order_ref.amount_remaining;
                            order_ref.amount_remaining = order_remaining;
                            order_ref.clone()
                        }
                    };
                    // Calculate the amount transferred
                    let order_transferred = order_amount - order_remaining;
                    // ... give the money back to the buyer ...
                    let player = order.player;
                    *self.balances.entry(player.clone()).or_default() += order_transferred * order.coins_per;
                    // ... give the assets ...
                    *self.assets.entry(player).or_default().entry(asset.clone()).or_default() += order_transferred;
                }
                
                // If needs be, list the remaining amount
                if amount_remaining > 0 {
                    self.best_sell.entry(asset.clone()).or_default().entry(0).or_default().push_back(id);
                    self.orders.insert(id, PendingOrder{ id, coins_per: 0, player: PlayerId::the_bank(), amount_remaining, asset, order_type: OrderType::Sell });
                }
                
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
            Action::UpdateBankPrices { withdraw_flat, withdraw_per_stack, expedited, investment_share, instant_smelt_per_stack } => {
                self.fees = UpdateBankPrices{ withdraw_flat, withdraw_per_stack, expedited, investment_share, instant_smelt_per_stack };
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
            Action::UpdateInvestables { assets } => {
                self.investables = assets.into_iter().collect();
                Ok(())
            },
            Action::Invest { player, asset, count } => {
                // Check to see if we can invest it
                if !self.investables.contains(&asset) {
                    return Err(StateApplyError::NotInvestable {asset});
                }
                // Check to see if the user can afford it, and if so, invest
                self.commit_asset_removal(&player, &asset, count)?;
                *self.player_investments.entry(player.clone()).or_default().entry(asset.clone()).or_default() += count;
                *self.asset_investments.entry(asset).or_default().entry(player).or_default() += count;
                Ok(())
            },
            Action::Uninvest { player, asset, count } => {
                // Don't check to see if it's currently investable, or else stuff might get trapped
                let std::collections::hash_map::Entry::Occupied(mut player_investment_list) = self.player_investments.entry(player.clone())
                else { return Err(StateApplyError::Overdrawn { asset: Some(asset), amount_overdrawn: count }) };
                let std::collections::hash_map::Entry::Occupied(mut asset_count) = player_investment_list.get_mut().entry(asset.clone())
                else { return Err(StateApplyError::Overdrawn { asset: Some(asset), amount_overdrawn: count }) };

                let std::collections::hash_map::Entry::Occupied(mut asset_investment_list) = self.asset_investments.entry(asset.clone())
                else { panic!("Investment table corruption: player_investments found but asset missing"); };
                let std::collections::hash_map::Entry::Occupied(mut asset_count2) = asset_investment_list.get_mut().entry(player.clone())
                else { panic!("Investment table corruption: player_investments found but player missing"); };

                match asset_count.get_mut().checked_sub(count) {
                    Some(0) => {
                        asset_count.remove();
                        if player_investment_list.get().is_empty() {
                            player_investment_list.remove();
                        }
                        asset_count2.remove();
                        if asset_investment_list.get().is_empty() {
                            asset_investment_list.remove();
                        }
                    }
                    None => { 
                        return Err(StateApplyError::Overdrawn { asset: Some(asset), amount_overdrawn: count - asset_count.get() })
                    },
                    Some(count) => {
                        *asset_count .get_mut() = count;
                        *asset_count2.get_mut() = count;
                    }
                }
                Ok(())
            },
            Action::InstantConvert { from, to, count, player } => {
                // Check convertable
                if self.convertables.contains(&(from.clone(), to.clone())) {
                    return Err(StateApplyError::NotConvertable { from, to });
                }
                // Calculate the fee
                let min_stack_size = self.asset_info(&from)?.stack_size.min(self.asset_info(&to)?.stack_size);
                let n_stacks = count.div_ceil(min_stack_size);
                let fee = n_stacks * self.fees.instant_smelt_per_stack;

                // Check to see if we can afford it
                self.check_busy(&to, count)?;
                // Check to see if they can afford the fees
                self.check_coin_removal(&player, fee)?;
                // Check to see if they can afford the assets, and if so, commit the changes
                self.commit_asset_removal(&player, &from, count)?;
                Self::commit_coin_removal(&mut self.balances, &player, count).expect("Unable to commit coin removal after check");
                self.commit_busy(&to, count).expect("Unable to commit busy after check");
                // Distribute the fee
                self.distribute_profit(&to, fee);
                
                // Give the assets
                *self.assets.entry(player).or_default().entry(to).or_default() += count;

                Ok(())
            }
            Action::UpdateConvertables { convertables } => {
                self.convertables = convertables.into_iter().collect();
                Ok(())
            }
        }
    }
    /// Load in the transactions from a trade file. Because of numbering, we must do this first; we cannot append
    pub async fn replay(trade_file: &mut (impl tokio::io::AsyncRead + std::marker::Unpin), asset_info: std::collections::HashMap<AssetId, AssetInfo>) -> Result<State, StateApplyError> {
        let mut state = Self::new(asset_info);

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
                out.flush().await.expect("Could not flush to log, must immediately stop!");
                Ok(id)
            }
            Err(e) => {
                Err(e)
            }
        }
    }
}

impl Serialize for State {
    // Returns an object that can be used to check we haven't gone off the rails
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("next_id", &self.next_id)?;
        map.serialize_entry("balances", &self.balances)?;
        map.serialize_entry("assets", &self.assets)?;
        map.serialize_entry("orders", &self.orders)?;
        map.serialize_entry("investments", &self.asset_investments)?;
        map.serialize_entry("authorisations", &self.authorisations)?;
        map.serialize_entry("restricted", &self.restricted_assets)?;
        map.serialize_entry("investables", &self.investables)?;
        map.serialize_entry("bankers", &self.bankers)?;
        map.serialize_entry("fees", &self.fees)?;
        map.end()
    }
}