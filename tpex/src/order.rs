use serde::{Deserialize, Serialize};

use crate::Coins;

use super::{AssetId, Audit, Auditable, Error, PlayerId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingSync {
    pub id: u64,
    pub player: PlayerId,
    pub amount_remaining: u64,
    pub fee_ppm: u64
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderSync {
    pub buy_orders: std::collections::HashMap<AssetId, std::collections::BTreeMap<Coins, Vec<PendingSync>>>,
    pub sell_orders: std::collections::HashMap<AssetId, std::collections::BTreeMap<Coins, Vec<PendingSync>>>,
}

impl TryInto<OrderTracker> for OrderSync {
    type Error = Error;
    fn try_into(self) -> Result<OrderTracker, Error> {
        let mut current_audit = Audit::default();
        let mut orders: std::collections::BTreeMap<u64, PendingOrder> = Default::default();
        let mut best_buy: std::collections::HashMap<String, std::collections::BTreeMap<Coins, std::collections::VecDeque<u64>>> = Default::default();
        let mut best_sell: std::collections::HashMap<String, std::collections::BTreeMap<Coins, std::collections::VecDeque<u64>>> = Default::default();
        for (asset, levels) in self.buy_orders {
            let entry = best_buy.entry(asset.clone()).or_default();
            for (coins_per, pending) in levels {
                let mut data = std::collections::VecDeque::with_capacity(pending.len());

                // If it already exists, we have an error
                for i in pending {
                    data.push_back(i.id);
                    if orders.insert(i.id, PendingOrder {
                        id: i.id,
                        coins_per,
                        player: i.player,
                        amount_remaining: i.amount_remaining,
                        asset: asset.clone(),
                        order_type: OrderType::Buy,
                        fee_ppm: i.fee_ppm,
                    }).is_some() { return Err(Error::InvalidFastSync); }
                    // Buy orders lock up coins
                    current_audit.add_coins(
                        // The fee + the total amount is 1 mil + fee (i.e. 1 + fee/1e6)
                        coins_per.fee_ppm(i.fee_ppm.checked_add(1_000_000).ok_or(Error::InvalidFastSync)?)?.checked_mul(i.amount_remaining)?
                    );
                }
                if entry.insert(coins_per, data).is_some() {
                    return Err(Error::InvalidFastSync);
                }
            }
        }

        for (asset, levels) in self.sell_orders {
            let entry = best_sell.entry(asset.clone()).or_default();
            for (coins_per, pending) in levels {
                let mut data = std::collections::VecDeque::with_capacity(pending.len());

                // If it already exists, we have an error
                for i in pending {
                    data.push_back(i.id);
                    if orders.insert(i.id, PendingOrder {
                        id: i.id,
                        coins_per,
                        player: i.player,
                        amount_remaining: i.amount_remaining,
                        asset: asset.clone(),
                        order_type: OrderType::Sell,
                        fee_ppm: i.fee_ppm,
                    }).is_some() { return Err(Error::InvalidFastSync); }
                    // Sell orders lock up items
                    current_audit.add_asset(asset.clone(), i.amount_remaining);
                }
                if entry.insert(coins_per, data).is_some() {
                    return Err(Error::InvalidFastSync);
                }
            }
        }
        Ok(OrderTracker {
            orders,
            best_buy,
            best_sell,
            current_audit
        })
    }
}

impl From<&OrderTracker> for OrderSync {
    fn from(val: &OrderTracker) -> Self {
        OrderSync {
            buy_orders:
                val.best_buy.iter()
                .map(|(asset, levels)| {
                    (asset.clone(), levels.iter().map(|(coins_per, pending)| {
                        (*coins_per, pending.iter().filter_map(|i| val.orders.get(i)).map(|i| PendingSync {
                            id: i.id,
                            player: i.player.clone(),
                            amount_remaining: i.amount_remaining,
                            fee_ppm: i.fee_ppm,
                        }).collect())
                    }).collect())
                }).collect(),
            sell_orders:
                val.best_sell.iter()
                .map(|(asset, levels)| {
                    (asset.clone(), levels.iter().map(|(coins_per, pending)| {
                        (*coins_per, pending.iter().filter_map(|i| val.orders.get(i)).map(|i| PendingSync {
                            id: i.id,
                            player: i.player.clone(),
                            amount_remaining: i.amount_remaining,
                            fee_ppm: i.fee_ppm,
                        }).collect())
                    }).collect())
                }).collect()
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
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

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct PendingOrder {
    pub id: u64,
    pub coins_per: Coins,
    pub player: PlayerId,
    pub amount_remaining: u64,
    pub asset: AssetId,
    pub order_type: OrderType,
    pub fee_ppm: u64
}

#[derive(Default)]
pub(crate) struct BuyData {
    // pub coins_refunded: Coins,
    pub cost: Coins,
    pub assets_instant_matched: u64,
    pub instant_bank_fee: Coins,
    /// Maps sellers to the amount they're owed
    pub sellers: std::collections::HashMap<PlayerId, Coins>
}

#[derive(Default)]
pub(crate) struct SellData {
    pub coins_instant_earned: Coins,
    pub assets_instant_matched: std::collections::HashMap<PlayerId, u64>,
    pub instant_bank_fee: Coins,
}
pub(crate) enum CancelResult {
    BuyOrder{player: PlayerId, refund_coins: Coins},
    SellOrder{player: PlayerId, refunded_asset: AssetId, refund_count: u64}
}

#[derive(Debug, Default, Serialize, Clone)]
pub(crate) struct OrderTracker {
    orders: std::collections::BTreeMap<u64, PendingOrder>,

    best_buy: std::collections::HashMap<AssetId, std::collections::BTreeMap<Coins, std::collections::VecDeque<u64>>>,
    best_sell: std::collections::HashMap<AssetId, std::collections::BTreeMap<Coins, std::collections::VecDeque<u64>>>,

    current_audit: Audit
}
struct MatchResult<T> {
    order_remaining: u64,
    order_taken: u64,
    data: T
}
impl OrderTracker {
    pub fn get_order(&self, id: u64) -> Result<PendingOrder, Error> { self.orders.get(&id).cloned().ok_or(Error::InvalidId { id }) }
    pub fn get_orders_filter<'a>(&'a self, filter: impl Fn(&'a PendingOrder) -> bool + 'a) -> impl Iterator<Item=PendingOrder> + 'a {
        self.orders.iter()
        .filter_map(move |(_i, j)| if filter(j) { Some(j.clone()) } else { None })
    }
    pub fn get_all(&self) -> std::collections::BTreeMap<u64, PendingOrder> { self.orders.clone() }
    /// Prices for an asset, returns (price, amount) in (buy, sell)
    pub fn get_prices(&self, asset: &AssetId) -> (std::collections::BTreeMap<Coins, u64>, std::collections::BTreeMap<Coins, u64>) {
        let buy_levels = self.best_buy
            .get(asset)
            .iter()
            .flat_map(|x| x.iter())
            .filter_map(|(level, orders)| {
                orders
                    .iter()
                    .cloned()
                    .map(|id| self.orders[&id].amount_remaining)
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
                    .map(|id| self.orders[&id].amount_remaining)
                    // We have None here iff there are no non-canceled orders
                    .reduce(|a,b| a+b)
                    .map(|amount| (*level, amount))
            })
            .collect();

        (buy_levels, sell_levels)
    }

    /// Generic function to match buy and sell orders
    fn do_match<T>(count: u64, mut elems: impl Iterator<Item = (u64, T)>) -> (u64, Vec<MatchResult<T>>) {
        let mut amount_remaining = count;
        let mut ret = Vec::new();
        while amount_remaining > 0 {
            let Some((this_count, data)) = elems.next()
            else {break;};
            match this_count.cmp(&amount_remaining) {
                // If the elem is not enough...
                std::cmp::Ordering::Less => {
                    ret.push(MatchResult{order_taken: this_count, order_remaining: 0, data});
                    amount_remaining -= this_count;
                    continue;
                },
                // If the elem is exactly enough...
                std::cmp::Ordering::Equal => {
                    ret.push(MatchResult{order_taken: this_count, order_remaining: 0, data});
                    amount_remaining = 0;
                    break;
                }
                // If the elem is more than enough...
                std::cmp::Ordering::Greater => {
                    ret.push(MatchResult{order_taken: amount_remaining, order_remaining: this_count - amount_remaining, data});
                    amount_remaining = 0;
                    break;
                }
            }
        }
        (amount_remaining, ret)
    }
    fn iterate_best_buy<'a>(&'a self, asset: &'a AssetId, limit: Coins) -> impl Iterator<Item = u64> + 'a {
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
    fn iterate_best_sell<'a>(&'a self, asset: &'a AssetId, limit: Coins) -> impl Iterator<Item = u64> + 'a {
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

    #[must_use]
    pub fn handle_buy(&mut self, id: u64, player: &PlayerId, asset: &AssetId, count: u64, coins_per: Coins, fee_ppm: u64) -> BuyData {
        let mut ret = BuyData::default();

        // Match the orders
        let iter = self.iterate_best_sell(asset, coins_per)
            .map(|idx| {
                let order = &self.orders[&idx];
                (order.amount_remaining, Some(order.clone()))
            });
        let (amount_remaining, orders) = Self::do_match(count, iter);

        // Handle successful matches
        for match_res in orders {
            let order = {
                if match_res.order_remaining == 0 {
                    // Check to see this wasn't a canceled order
                    if let Some(order_val) = self.remove_best(asset.clone(), OrderType::Sell) {
                        order_val
                    }
                    else { continue; }
                }
                else {
                    let order_ref = self.orders.get_mut(&match_res.data.expect("Partial canceled order").id).expect("Cannot get mut order");
                    order_ref.amount_remaining = match_res.order_remaining;
                    order_ref.clone()
                }
            };
            // Calculate the number of coins ordered for the seller
            let sale_coins = order.coins_per.checked_mul(match_res.order_taken).expect("Coins earnt overflow");

            // Calculate the fee for the sell order
            let seller_fee = sale_coins.fee_ppm(order.fee_ppm).expect("Fee overflow");
            // We can now pass the fee to the bank
            ret.instant_bank_fee.checked_add_assign(seller_fee).expect("Fee overflow");

            // Calculate the fee for the buy order
            let buyer_fee = sale_coins.fee_ppm(fee_ppm).expect("Fee overflow");
            // We can now pass the fee to the bank
            ret.instant_bank_fee.checked_add_assign(buyer_fee).expect("Fee overflow");

            // Give the assets ...
            ret.assets_instant_matched += match_res.order_taken;
            ret.cost.checked_add_assign(sale_coins.checked_add(buyer_fee).expect("Fee overflow")).expect("Cost overflow");
            // (they can't have saved on the fee, so they don't get a refund for that)
            // ... and track the seller
            ret.sellers.entry(order.player).or_default().checked_add_assign(sale_coins.checked_sub(seller_fee).expect("Fee greater than cost")).expect("Seller balance overflow");
        }

        // If needs be, list the remaining amount
        if amount_remaining > 0 {
            let mut remaining_cost = coins_per.checked_mul(amount_remaining).expect("Buy order remaining coins overflow");
            remaining_cost.checked_add_assign(remaining_cost.fee_ppm(fee_ppm).expect("Fee overflow")).expect("Buy order remaining fee coins overflow");
            self.best_buy.entry(asset.clone()).or_default().entry(coins_per).or_default().push_back(id);
            self.orders.insert(id, PendingOrder{ id, coins_per, player: player.clone(), amount_remaining, asset: asset.clone(), order_type: OrderType::Buy, fee_ppm });
            // We are responsible for the coins bound up in the buy order
            self.current_audit.add_coins(remaining_cost);
            ret.cost.checked_add_assign(remaining_cost).expect("Add remaining cost overflow");
        }
        // We are no longer responsible for the bought items
        self.current_audit.sub_asset(asset.clone(), ret.assets_instant_matched);

        ret
    }

    #[must_use]
    pub fn handle_sell(&mut self, id:u64, player: &PlayerId, asset: &AssetId, count: u64, coins_per: Coins, fee_ppm: u64) -> SellData {
        let mut ret = SellData::default();

        // Then match the orders
        let iter = self.iterate_best_buy(asset, coins_per)
            .map(|idx| {
                let order = &self.orders[&idx];
                (order.amount_remaining, Some(order.clone()))
            });
        let (amount_remaining, orders) = Self::do_match(count, iter);

        // Handle successful matches
        for match_res in orders {
            let order = {
                if match_res.order_remaining == 0 {
                    // Check to see this wasn't a canceled order
                    if let Some(order_val) = self.remove_best(asset.clone(), OrderType::Buy) {
                        order_val
                    }
                    else { continue; }
                }
                else {
                    let order_ref = self.orders.get_mut(&match_res.data.expect("Partial canceled order").id).expect("Cannot get mut order");
                    order_ref.amount_remaining = match_res.order_remaining;
                    order_ref.clone()
                }
            };
            // Calculate the number of coins ordered for the seller
            let sale_coins = order.coins_per.checked_mul(match_res.order_taken).expect("Sell order instant earned increment overflow");

            // Calculate the fee for the sell order
            let seller_fee = sale_coins.fee_ppm(fee_ppm).expect("Fee overflow");
            // We can now pass the fee to the bank
            ret.instant_bank_fee.checked_add_assign(seller_fee).expect("Fee overflow");

            // Calculate the fee for the buy order
            let buyer_fee = sale_coins.fee_ppm(order.fee_ppm).expect("Fee overflow");
            // We can now pass the fee to the bank
            ret.instant_bank_fee.checked_add_assign(buyer_fee).expect("Fee overflow");

            // Give the money ...
            ret.coins_instant_earned.checked_add_assign(sale_coins.checked_sub(seller_fee).expect("Fee greater than cost")).expect("Sell order instant earned overflow");
            // ... give the assets ...
            *ret.assets_instant_matched.entry(order.player).or_default() += match_res.order_taken;

            // We are no longer responsible for the sale coins + fees
            self.current_audit.sub_coins(sale_coins);
            self.current_audit.sub_coins(buyer_fee);
        }

        // If needs be, list the remaining amount
        if amount_remaining > 0 {
            self.best_sell.entry(asset.clone()).or_default().entry(coins_per).or_default().push_back(id);
            self.orders.insert(id, PendingOrder{ id, coins_per, player: player.clone(), amount_remaining, asset: asset.clone(), order_type: OrderType::Sell, fee_ppm });
        }

        // We are responsible for the remaining listed items
        self.current_audit.add_asset(asset.clone(), amount_remaining);

        ret
    }
    pub fn cancel(&mut self, target_id: u64) -> Result<CancelResult, Error> {
        if let Some(found) = self.orders.remove(&target_id) {
            match found.order_type {
                // If we found it as a buy...
                OrderType::Buy => {
                    let refund_coins =
                        found.coins_per.checked_mul(found.amount_remaining).expect("Order cancel refund overflow")
                        // refund the fee too
                        .fee_ppm(1_000_000_u64.checked_add(found.fee_ppm).expect("Order cancel fee overflow")).expect("Order cancel fee overflow");
                    // ... we are no longer responsible for the refunded coins ...
                    self.current_audit.sub_coins(refund_coins);
                    // ... remove it from the order list ...
                    {
                        let levels = self.best_buy.get_mut(&found.asset).expect("Failed to find asset in cancel buy");
                        let std::collections::btree_map::Entry::Occupied(mut target) = levels.entry(found.coins_per)
                        else { unreachable!("Failed to find level in cancel buy") };
                        let target_val = target.get_mut();
                        target_val.remove(target_val.iter().position(|i| *i == target_id).expect("Failed to find order in cancel buy"));
                        if target_val.is_empty() {
                            target.remove();
                        }
                        if levels.is_empty() {
                            self.best_buy.remove(&found.asset);
                        }
                    }
                    // ... and refund the money
                    Ok(CancelResult::BuyOrder { player: found.player, refund_coins })
                },
                // If we found it as a sell...
                OrderType::Sell => {
                    // ... we are no longer responsible for the refunded assets ...
                    self.current_audit.sub_asset(found.asset.clone(), found.amount_remaining);
                    // ... remove it from the order list ...
                    {
                        let levels = self.best_sell.get_mut(&found.asset).expect("Failed to find asset in cancel sell");
                        let std::collections::btree_map::Entry::Occupied(mut target) = levels.entry(found.coins_per)
                        else { unreachable!("Failed to find level in cancel sell") };
                        let target_val = target.get_mut();
                        target_val.remove(target_val.iter().position(|i| *i == target_id).expect("Failed to find order in cancel sell"));
                        if target_val.is_empty() {
                            target.remove();
                        }
                        if levels.is_empty() {
                            self.best_sell.remove(&found.asset);
                        }
                    }
                    // ... and refund the assets
                    Ok(CancelResult::SellOrder { player: found.player, refunded_asset: found.asset, refund_count: found.amount_remaining })
                }
            }
        }
        // If we didn't find it, it was invalid
        else {
            Err(Error::InvalidId{id: target_id})
        }
    }
}
impl Auditable for OrderTracker {
    fn soft_audit(&self) -> Audit { self.current_audit.clone() }

    fn hard_audit(&self) -> Audit {
        let mut new_audit = Audit::default();
        for order in self.orders.values() {
            match order.order_type {
                // A buy order has taken coins from someone's account
                OrderType::Buy => {
                    let mut cost = order.coins_per.checked_mul(order.amount_remaining).expect("Hard audit coin increment overflow");
                    cost.checked_add_assign(cost.fee_ppm(order.fee_ppm).expect("Fee overflow")).expect("Hard audit coin fee increment overflow");
                    new_audit.add_coins(cost);
                },
                // A buy order has taken assets from someone's account
                OrderType::Sell => new_audit.add_asset(order.asset.clone(), order.amount_remaining),
            }
        }
        if new_audit != self.current_audit {
            panic!("Order tracker has inconsistent audit: hard {:?} vs soft {:?} for all {:?}", new_audit, self.current_audit, self.orders);
        }
        new_audit
    }
}
