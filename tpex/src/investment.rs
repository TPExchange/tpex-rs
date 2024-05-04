use serde::Serialize;

use crate::Coins;

use super::{AssetId, Audit, Auditable, PlayerId, Error};

#[derive(Debug, Serialize)]
struct Investment {
    player: PlayerId,
    asset: AssetId,
    count: u64
}

#[derive(Default, Debug, Serialize, Clone)]
pub struct InvestmentTracker {
    // These three tables must be kept consistent
    asset_investments: std::collections::HashMap<AssetId, std::collections::HashMap<PlayerId, u64>>,
    player_investments: std::collections::HashMap<PlayerId, std::collections::HashMap<AssetId, u64>>,
    amount_invested: std::collections::HashMap<AssetId, u64>,

    investment_busy: std::collections::HashMap<AssetId, u64>,
    investment_confirmed: std::collections::HashMap<PlayerId, std::collections::HashMap<AssetId, u64>>,

    current_audit: Audit
}
impl InvestmentTracker {
    // /// Distribute the profits among the investors
    // fn distribute_profit(&mut self, asset: &AssetId, amount: u64) {
    //     let mut investors = self.investment.get_investors(asset);
    //     // Let's be fair and not give ourselves all the money
    //     investors.remove(&PlayerId::the_bank());
    //     let share = (self.fees.investment_share.mul(amount as f64) / (investors.values().sum::<u64>() as f64)).floor() as u64;
    //     let mut total_distributed = 0;
    //     for (investor, shares) in investors {
    //         let investor_profit = share * shares;
    //         total_distributed += investor_profit;
    //         self.balance.commit_coin_add(&investor, investor_profit);
    //     }
    //     if total_distributed > amount {
    //         panic!("Profit distribution imprecision was too bad");
    //     }
    //     self.balance.commit_coin_add(&PlayerId::the_bank(), amount - total_distributed);
    // }

    pub fn add_investment(&mut self, player: &PlayerId, asset: &AssetId, count: u64) {
        *self.asset_investments.entry(asset.clone()).or_default().entry(player.clone()).or_default() += count;
        *self.player_investments.entry(player.clone()).or_default().entry(asset.clone()).or_default() += count;
        // Auditing
        self.current_audit.add_asset(asset.clone(), count);
    }
    pub fn try_remove_investment(&mut self, player: &PlayerId, asset: &AssetId, count: u64) -> Result<(), Error> {
        let std::collections::hash_map::Entry::Occupied(mut player_investment_list) = self.player_investments.entry(player.clone())
        else { return Err(Error::OverdrawnAsset { asset: asset.clone(), amount_overdrawn: count }) };
        let std::collections::hash_map::Entry::Occupied(mut asset_count) = player_investment_list.get_mut().entry(asset.clone())
        else { return Err(Error::OverdrawnAsset { asset: asset.clone(), amount_overdrawn: count }) };

        let std::collections::hash_map::Entry::Occupied(mut asset_investment_list) = self.asset_investments.entry(asset.clone())
        else { panic!("Investment table corruption: player_investments found but asset missing"); };
        let std::collections::hash_map::Entry::Occupied(mut asset_count2) = asset_investment_list.get_mut().entry(player.clone())
        else { panic!("Investment table corruption: player_investments found but player missing"); };

        match asset_count.get_mut().checked_sub(count) {
            Some(0) => {
                // Do clean up
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
                return Err(Error::OverdrawnAsset { asset: asset.clone(), amount_overdrawn: count - asset_count.get() })
            },
            Some(count) => {
                *asset_count .get_mut() = count;
                *asset_count2.get_mut() = count;
            }
        }

        // Auditing
        self.current_audit.sub_asset(asset.clone(), count);

        Ok(())
    }
    #[allow(dead_code)]
    pub fn try_mark_busy(&mut self, asset: &AssetId, count: u64) -> Result<(), Error> {
        let amount_invested = self.amount_invested.get(asset).cloned().unwrap_or_default();
        let amount_busy = self.investment_busy.entry(asset.clone());
        let amount_free = amount_invested - match amount_busy {
            std::collections::hash_map::Entry::Occupied(ref x) => *x.get(),
            _ => 0
        };
        if amount_free < count {
            return Err(Error::InvestmentBusy { asset: asset.clone(), amount_over: count - amount_free })
        }
        *amount_busy.or_default() += count;

        self.current_audit.sub_asset(asset.clone(), count);
        Ok(())
    }
    #[allow(dead_code)]
    pub fn mark_confirmed(&mut self, player: &PlayerId, asset: &AssetId, count: u64) {
        *self.investment_confirmed.entry(player.clone()).or_default().entry(asset.clone()).or_default() += count;
        self.current_audit.add_asset(asset.clone(), count);
    }
    #[allow(dead_code)]
    pub fn get_investors(&self, asset: &AssetId) -> std::collections::HashMap<PlayerId, u64> {
        self.asset_investments.get(asset).cloned().unwrap_or_default()
    }
}
impl Auditable for InvestmentTracker {
    fn soft_audit(&self) -> Audit { self.current_audit.clone() }

    fn hard_audit(&self) -> Audit {
        // Check the tables are consistent
        let asset_recalc: std::collections::HashMap<AssetId, u64> = self.asset_investments.iter()
            .map(|(asset, tab)| (asset.clone(),tab.values().sum())).collect();
        let player_recalc: std::collections::HashMap<AssetId, u64> = self.player_investments.values()
            .fold(Default::default(), |mut a, b| {
                b.iter().for_each(|(asset, count)| {
                    *a.entry(asset.clone()).or_default() += count;
                });
                a
            });
        if player_recalc != asset_recalc {
            panic!("Investment table inconsistent: player does not match asset");
        }
        // Doesn't matter which one, they're the same
        let mut total_invested = player_recalc;
        // Now add what has been promised
        self.investment_confirmed.values()
            .for_each(|b| {
                b.iter().for_each(|(asset, count)| {
                    *total_invested.entry(asset.clone()).or_default() += count;
                });
            });
        // Take away what we have lent out
        for (asset, count) in &self.investment_busy {
            let target = total_invested.entry(asset.clone()).or_default();
            if let Some(res) = target.checked_sub(*count) {
                *target = res;
            }
            else {
                panic!("Investment table inconsistent: lent out non-existent asset");
            }
        }
        // Finally, filter out the empty assets
        total_invested.retain(|_asset, count| *count != 0);
        let new_audit = Audit{coins: Coins::default(), assets: total_invested};
        // Check to see if this matches our info
        if new_audit != self.current_audit {
            panic!("Investment table inconsistent: recalculated audit differed from soft result");
        }
        new_audit
    }
}
