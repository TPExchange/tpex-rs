use serde::Serialize;

use super::{AssetId, Audit, Auditable, Error, PlayerId};

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

#[derive(Default)]
pub struct BuyData {
    pub coins_refunded: u64,
    pub assets_instant_matched: u64,
    /// Maps sellers to the amount they're owed
    pub sellers: std::collections::HashMap<PlayerId, u64>
}

#[derive(Default)]
pub struct SellData {
    pub coins_instant_earned: u64,
    pub assets_instant_matched: std::collections::HashMap<PlayerId, u64>
}
pub enum CancelResult {
    BuyOrder{player: PlayerId, refund_coins: u64},
    SellOrder{player: PlayerId, refunded_asset: AssetId, refund_count: u64}
}

#[derive(Debug, Default, Serialize, Clone)]
pub struct OrderTracker {
    orders: std::collections::BTreeMap<u64, PendingOrder>,

    /// XXX: this contains cancelled orders, skip over them
    best_buy: std::collections::HashMap<AssetId, std::collections::BTreeMap<u64, std::collections::VecDeque<u64>>>,
    /// XXX: this contains cancelled orders, skip over them
    best_sell: std::collections::HashMap<AssetId, std::collections::BTreeMap<u64, std::collections::VecDeque<u64>>>,

    current_audit: Audit
}
struct MatchResult<T> {
    order_remaining: u64,
    order_taken: u64,
    data: T
}
impl OrderTracker {
    pub fn get_order(&self, id: u64) -> Result<PendingOrder, Error> { self.orders.get(&id).cloned().ok_or(Error::InvalidId { id }) }
    pub fn get_all(&self) -> std::collections::BTreeMap<u64, PendingOrder> { self.orders.clone() }
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

    /// Generic function to match buy and sell orders, investments, etc
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

    #[must_use]
    pub fn handle_buy(&mut self, id: u64, player: &PlayerId, asset: &AssetId, count: u64, coins_per: u64) -> BuyData {
        let mut ret = BuyData::default();

        // Match the orders
        let iter = self.iterate_best_sell(asset, coins_per)
            .map(|idx| {
                match self.orders.get(&idx) {
                    Some(order) => (order.amount_remaining, Some(order.clone())),
                    None => (0, None)
                }
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
            // Give the assets ...
            ret.assets_instant_matched += match_res.order_taken;
            // ... if they bought it cheap, give them the difference ...
            ret.coins_refunded += match_res.order_taken * (coins_per - order.coins_per);
            // ... and track the seller
            *ret.sellers.entry(order.player).or_default() += order.coins_per * match_res.order_taken
        }

        // If needs be, list the remaining amount
        if amount_remaining > 0 {
            self.best_buy.entry(asset.clone()).or_default().entry(coins_per).or_default().push_back(id);
            self.orders.insert(id, PendingOrder{ id, coins_per, player: player.clone(), amount_remaining, asset: asset.clone(), order_type: OrderType::Buy });
            // We are responsible for the coins bound up in the buy order
            self.current_audit.coins += amount_remaining * coins_per;
        }
        // We are no longer responsible for the bought items
        self.current_audit.sub_asset(asset.clone(), ret.assets_instant_matched).expect("Buy order took non-audited assets");

        ret
    }

    #[must_use]
    pub fn handle_sell(&mut self, id:u64, player: &PlayerId, asset: &AssetId, count: u64, coins_per: u64) -> SellData {
        let mut ret = SellData::default();

        // Then match the orders
        let iter = self.iterate_best_buy(asset, coins_per)
            .map(|idx| {
                match self.orders.get(&idx) {
                    Some(order) => (order.amount_remaining, Some(order.clone())),
                    None => (0, None)
                }
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
            // Give the money ...
            ret.coins_instant_earned += match_res.order_taken * order.coins_per;
            // ... give the assets ...
            *ret.assets_instant_matched.entry(order.player).or_default() +=  match_res.order_taken;
        }

        // If needs be, list the remaining amount
        if amount_remaining > 0 {
            self.best_sell.entry(asset.clone()).or_default().entry(coins_per).or_default().push_back(id);
            self.orders.insert(id, PendingOrder{ id, coins_per, player: player.clone(), amount_remaining, asset: asset.clone(), order_type: OrderType::Sell });
        }

        // We are no longer responsible for the earnt coins
        self.current_audit.sub_coins(ret.coins_instant_earned).expect("Sell order earned non-audited coins");
        // We are responsible for the remaining listed items
        self.current_audit.add_asset(asset.clone(), amount_remaining);

        ret
    }
    pub fn cancel(&mut self, target_id: u64) -> Result<CancelResult, Error> {
        if let Some(found) = self.orders.remove(&target_id) {
            match found.order_type {
                // If we found it as a buy...
                OrderType::Buy => {
                    // ... we are no longer responsible for the refunded coins ...
                    self.current_audit.sub_coins(found.amount_remaining * found.coins_per).expect("Canceled order with unaudited coins");
                    // ... and refund the money ...
                    Ok(CancelResult::BuyOrder { player: found.player, refund_coins: found.amount_remaining * found.coins_per })
                },
                // If we found it as a sell...
                OrderType::Sell => {
                    // ... we are no longer responsible for the refunded assets ...
                    self.current_audit.sub_asset(found.asset.clone(), found.amount_remaining).expect("Canceled order with unaudited coins");
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
                OrderType::Buy => new_audit.coins += order.amount_remaining * order.coins_per,
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
