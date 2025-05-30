use serde::{Deserialize, Serialize};

use crate::Coins;

use super::{AssetId, Audit, Auditable, Error, PlayerId};

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct BalanceSync {
    pub balances: std::collections::HashMap<PlayerId, Coins>,
    pub assets: std::collections::HashMap<PlayerId, std::collections::HashMap<AssetId, u64>>,
}
impl From<&BalanceTracker> for BalanceSync {
    fn from(value: &BalanceTracker) -> Self {
        BalanceSync { balances: value.balances.clone(), assets: value.assets.clone() }
    }
}
impl TryFrom<BalanceSync> for BalanceTracker {
    type Error = Error;

    fn try_from(value: BalanceSync) -> Result<Self, Self::Error> {
        let current_audit = Audit {
            coins: value.balances.values().try_fold(Coins::default(), |x, y| x.checked_add(*y))?,
            assets: value.assets.values()
                .try_fold(std::collections::HashMap::default(), |mut acc, assets| {
                    for (asset_name, asset_count) in assets {
                        let tgt: &mut u64 = acc.entry(asset_name.clone()).or_default();
                        *tgt = tgt.checked_add(*asset_count).ok_or(Error::InvalidFastSync)?;
                    }
                    Ok(acc)
                })?
        };
        Ok(BalanceTracker {
            balances: value.balances,
            assets: value.assets,
            current_audit
        })
    }
}

#[derive(Default, Debug, Clone)]
pub struct BalanceTracker {
    balances: std::collections::HashMap<PlayerId, Coins>,
    assets: std::collections::HashMap<PlayerId, std::collections::HashMap<AssetId, u64>>,

    current_audit: Audit
}
impl BalanceTracker {
    /// Get a player's balance
    pub fn get_bal(&self, player: &PlayerId) -> Coins {
        self.balances.get(player).map_or(Coins::default(), Clone::clone)
    }
    /// Get a player's assets
    pub fn get_assets(&self, player: &PlayerId) -> std::collections::HashMap<AssetId, u64> {
        self.assets.get(player).map_or_else(Default::default, Clone::clone)
    }
    /// Get all balances
    pub fn get_bals(&self) -> std::collections::HashMap<PlayerId, Coins> { self.balances.clone() }

    /// Check if a player can afford to give up assets
    pub fn check_asset_removal(&self, player: &PlayerId, asset: &str, count: u64) -> Result<(), Error> {
        // If the player doesn't have an account, they definitely cannot withdraw
        let Some(tgt) = self.assets.get(player)
        else { return Err(Error::OverdrawnAsset { asset: asset.to_string(), amount_overdrawn: count }); };

        // If they aren't listed for an asset, they definitely cannot withdraw
        let Some(tgt) = tgt.get(asset)
        else { return Err(Error::OverdrawnAsset { asset: asset.to_string(), amount_overdrawn: count }); };

        // If they don't have enough, they cannot withdraw
        if *tgt < count  {
            return Err(Error::OverdrawnAsset { asset: asset.to_string(), amount_overdrawn: count - *tgt });
        }
        Ok(())
    }
    /// Decreases a player's asset count, but only if they can afford it
    pub fn commit_asset_removal(&mut self, player: &PlayerId, asset: &AssetId, count: u64) -> Result<(), Error> {
        // If the player doesn't have an account, they definitely cannot withdraw
        let Some(assets) = self.assets.get_mut(player)
        else { return Err(Error::OverdrawnAsset { asset: asset.clone(), amount_overdrawn: count }); };

        // If they aren't listed for an asset, they definitely cannot withdraw
        let Some(tgt) = assets.get_mut(asset)
        else { return Err(Error::OverdrawnAsset { asset: asset.clone(), amount_overdrawn: count }); };

        // If they don't have enough, they cannot withdraw
        if *tgt < count  {
            return Err(Error::OverdrawnAsset { asset: asset.to_string(), amount_overdrawn: count - *tgt });
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
        self.current_audit.sub_asset(asset.clone(), count);
        Ok(())
    }
    #[allow(dead_code)]
    /// Check if a player can afford to pay
    pub fn check_coin_removal(&self, player: &PlayerId, count: Coins) -> Result<(), Error> {
        // If the player doesn't have an account, they definitely cannot withdraw
        let Some(tgt) = self.balances.get(player)
        else { return Err(Error::OverdrawnCoins { amount_overdrawn: count }); };

        // If they don't have enough, they cannot withdraw
        if *tgt < count {
            return Err(Error::OverdrawnCoins { amount_overdrawn: count.checked_sub(*tgt).expect("Overdrawn underflow") });
        }
        Ok(())
    }
    /// Decreases a player's coin count, but only if they can afford it
    pub fn commit_coin_removal(&mut self, player: &PlayerId, count: Coins) -> Result<(), Error> {
        // If the player doesn't have an account, they definitely cannot withdraw
        let Some(tgt) = self.balances.get_mut(player)
        else { return Err(Error::OverdrawnCoins { amount_overdrawn: count }); };

        // If they don't have enough, they cannot withdraw
        if *tgt < count {
            return Err(Error::OverdrawnCoins { amount_overdrawn: count.checked_sub(*tgt).expect("Overdrawn underflow") });
        }

        // Take away their coins
        tgt.checked_sub_assign(count).expect("Coin removal underflow");

        // If it's zero, clean up
        if tgt.is_zero() {
            self.balances.remove(player);
        }

        self.current_audit.sub_coins(count);
        Ok(())
    }
    /// Increases a player's asset count
    pub fn commit_asset_add(&mut self, player: &PlayerId, asset: &AssetId, count: u64) {
        let tgt = self.assets.entry(player.clone()).or_default().entry(asset.clone()).or_default();
        *tgt = tgt.checked_add(count).ok_or(Error::Overflow).expect("Item count overflow");
        self.current_audit.add_asset(asset.clone(), count);
    }
    /// Increases a player's coin count
    pub fn commit_coin_add(&mut self, player: &PlayerId, count: Coins) {
        self.balances.entry(player.clone()).or_default().checked_add_assign(count).expect("Player balance overflow");
        self.current_audit.add_coins(count);
    }
}
impl Auditable for BalanceTracker {
    fn soft_audit(&self) -> Audit { self.current_audit.clone() }

    fn hard_audit(&self) -> Audit {
        if self.current_audit.coins != self.balances.values().fold(Coins::default(), |acc, i| acc.checked_add(*i).expect("Audit balance overflow")) {
            panic!("Coins inconsistent in balance");
        }
        let mut recalced_assets: std::collections::HashMap<AssetId, u64> = std::collections::HashMap::new();
        for  player_assets in self.assets.values() {
            for (asset, count) in player_assets {
                *recalced_assets.entry(asset.clone()).or_default() += count;
            }
        }
        if self.current_audit.assets != recalced_assets {
            panic!("Assets inconsistent in balance");
        }
        self.soft_audit()
    }
}
