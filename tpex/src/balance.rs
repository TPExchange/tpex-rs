use serde::Serialize;

use super::{AssetId, Audit, Auditable, Error, PlayerId};


#[derive(Default, Debug, Serialize, Clone)]
pub struct BalanceTracker {
    balances: std::collections::HashMap<PlayerId, u64>,
    assets: std::collections::HashMap<PlayerId, std::collections::HashMap<AssetId, u64>>,

    current_audit: Audit
}
impl BalanceTracker {
    /// Get a player's balance
    pub fn get_bal(&self, player: &PlayerId) -> u64 {
        self.balances.get(player).map_or(0, Clone::clone)
    }
    /// Get a player's assets
    pub fn get_assets(&self, player: &PlayerId) -> std::collections::HashMap<AssetId, u64> {
        self.assets.get(player).map_or_else(Default::default, Clone::clone)
    }
    /// Get all balances
    pub fn get_bals(&self) -> std::collections::HashMap<PlayerId, u64> { self.balances.clone() }

    /// Check if a player can afford to give up assets
    pub fn check_asset_removal(&self, player: &PlayerId, asset: &str, count: u64) -> Result<(), Error> {
        // If the player doesn't have an account, they definitely cannot withdraw
        let Some(tgt) = self.assets.get(player)
        else { return Err(Error::Overdrawn { asset: Some(asset.to_string()), amount_overdrawn: count }); };

        // If they aren't listed for an asset, they definitely cannot withdraw
        let Some(tgt) = tgt.get(asset)
        else { return Err(Error::Overdrawn { asset: Some(asset.to_string()), amount_overdrawn: count }); };

        // If they don't have enough, they cannot withdraw
        if *tgt < count  {
            return Err(Error::Overdrawn { asset: Some(asset.to_string()), amount_overdrawn: count - *tgt });
        }
        Ok(())
    }
    /// Decreases a player's asset count, but only if they can afford it
    pub fn commit_asset_removal(&mut self, player: &PlayerId, asset: &AssetId, count: u64) -> Result<(), Error> {
        // If the player doesn't have an account, they definitely cannot withdraw
        let Some(assets) = self.assets.get_mut(player)
        else { return Err(Error::Overdrawn { asset: Some(asset.clone()), amount_overdrawn: count }); };

        // If they aren't listed for an asset, they definitely cannot withdraw
        let Some(tgt) = assets.get_mut(asset)
        else { return Err(Error::Overdrawn { asset: Some(asset.clone()), amount_overdrawn: count }); };

        // If they don't have enough, they cannot withdraw
        if *tgt < count  {
            return Err(Error::Overdrawn { asset: Some(asset.to_string()), amount_overdrawn: count - *tgt });
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
        self.current_audit.sub_asset(asset.clone(), count).expect("Asset removal failed audit");
        Ok(())
    }
    /// Check if a player can afford to pay
    pub fn check_coin_removal(&self, player: &PlayerId, count: u64) -> Result<(), Error> {
        // If the player doesn't have an account, they definitely cannot withdraw
        let Some(tgt) = self.balances.get(player)
        else { return Err(Error::Overdrawn { asset: None, amount_overdrawn: count }); };

        // If they don't have enough, they cannot withdraw
        if *tgt < count {
            return Err(Error::Overdrawn { asset: None, amount_overdrawn: count - *tgt });
        }
        Ok(())
    }
    /// Decreases a player's coin count, but only if they can afford it
    pub fn commit_coin_removal(&mut self, player: &PlayerId, count: u64) -> Result<(), Error> {
        // If the player doesn't have an account, they definitely cannot withdraw
        let Some(tgt) = self.balances.get_mut(player)
        else { return Err(Error::Overdrawn { asset: None, amount_overdrawn: count }); };

        // If they don't have enough, they cannot withdraw
        if *tgt < count {
            return Err(Error::Overdrawn { asset: None, amount_overdrawn: count - *tgt });
        }

        // Take away their coins
        *tgt -= count;

        // If it's zero, clean up
        if *tgt == 0 {
            self.balances.remove(player);
        }

        self.current_audit.sub_coins(count).expect("Balance removal failed audit");
        Ok(())
    }
    /// Increases a player's asset count
    pub fn commit_asset_add(&mut self, player: &PlayerId, asset: &AssetId, count: u64) {
        *self.assets.entry(player.clone()).or_default().entry(asset.clone()).or_default() += count;
        self.current_audit.add_asset(asset.clone(), count);
    }
    /// Increases a player's coin count
    pub fn commit_coin_add(&mut self, player: &PlayerId, count: u64) {
        *self.balances.entry(player.clone()).or_default() += count;
        self.current_audit.coins += count;
    }
}
impl Auditable for BalanceTracker {
    fn soft_audit(&self) -> Audit { self.current_audit.clone() }

    fn hard_audit(&self) -> Audit {
        if self.current_audit.coins != self.balances.values().sum::<u64>() {
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
