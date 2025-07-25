use serde::{Deserialize, Serialize};

use crate::{AssetId, Error, PlayerId, Result};

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct AuthSync {
    /// The restricted assets
    pub restricted: std::collections::HashSet<AssetId>,
    /// The authorisations that various players have
    pub authorisations: std::collections::HashMap<PlayerId, std::collections::HashMap<AssetId, u64>>,
}
impl From<&AuthTracker> for AuthSync {
    fn from(value: &AuthTracker) -> Self {
        AuthSync {
            restricted: value.restricted.clone(),
            authorisations: value.authorisations.clone(),
        }
    }
}
impl TryFrom<AuthSync> for AuthTracker {
    type Error = Error;

    fn try_from(value: AuthSync) -> Result<AuthTracker> {
        Ok(AuthTracker {
            restricted: value.restricted,
            authorisations: value.authorisations
        })
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) struct AuthTracker {
    /// The restricted assets
    restricted: std::collections::HashSet<AssetId>,
    /// The authorisations that various players have
    authorisations: std::collections::HashMap<PlayerId, std::collections::HashMap<AssetId, u64>>,
}
impl Default for AuthTracker {
    fn default() -> Self {
        Self::new()
    }
}
impl AuthTracker {
    pub fn new() -> AuthTracker {
        AuthTracker {
            restricted: Default::default(),
            authorisations: Default::default(),
        }
    }
    /// Returns true if the given item is currently restricted
    pub fn is_restricted(&self, asset: &AssetId) -> bool { self.restricted.contains(asset) }
    /// Lists all restricted items
    pub fn get_restricted(&self) -> &std::collections::HashSet<AssetId> { &self.restricted }
    /// Sets the maximum amount a player is able to withdraw of a restricted item.
    ///
    /// XXX: This can and will nuke existing values, so check those race conditions
    pub fn set_authorisation(&mut self, player: PlayerId, asset: AssetId, new_count: u64) {
        // Clean up the entry (or even the player) if they're being deauthed
        if new_count == 0 {
            let player_auths = self.authorisations.get_mut(&player).unwrap();
            player_auths.remove(&asset);
            if player_auths.is_empty() {
                self.authorisations.remove(&player);
            }
        }
        else {
            self.authorisations.entry(player).or_default().insert(asset, new_count);
        }
    }
    /// Increases the maximum amount of an item a player is allowed to withdraw
    ///
    /// @returns The new limit the player has
    pub fn increase_authorisation(&mut self, player: PlayerId, asset: AssetId, increase: u64) -> Result<u64> {
        self.authorisations.entry(player).or_default()
            .entry(asset).or_default()
            .checked_add(increase).ok_or(Error::Overflow)
    }
    /// Updates the list of restricted assets
    pub fn update_restricted(&mut self, restricted: std::collections::HashSet<AssetId>) {
        // Clean up the irrelevant tables, so that auths don't secretly lie around
        let newly_unrestricted = self.restricted.difference(&restricted);
        for i in newly_unrestricted {
            for asset_auths in self.authorisations.values_mut() {
                asset_auths.remove(i);
            }
        }
        self.restricted = restricted;
    }
    /// Checks to see if a player can withdraw a certain asset
    pub fn check_withdrawal_authorized(&self, player: &PlayerId, asset: &AssetId, count: u64) -> Result<()> {
        // If it's unrestricted, they can withdraw as much as they like
        if !self.is_restricted(asset) {
            return Ok(())
        }
        // Try to find the authorisation in the map. If it's not there, then they are not allowed this item.
        let Some(n) = self.authorisations.get(player).and_then(|x| x.get(asset)).copied()
        else { return Err(Error::UnauthorisedWithdrawal{ asset: asset.clone(), amount_overdrawn: None}); };
        // Check to see if they can withdraw the entire amount
        if n < count {
            return Err(Error::UnauthorisedWithdrawal{ asset: asset.clone(), amount_overdrawn: Some(count - n)});
        }
        Ok(())
    }
    /// Tries to remove assets for a player
    pub fn commit_withdrawal_authorized(&mut self, player: &PlayerId, asset: &AssetId, count: u64) -> Result<()> {
        // If it's unrestricted, they can withdraw as much as they like
        if !self.is_restricted(asset) {
            return Ok(())
        }
        // Try to find the authorisation in the map. If it's not there, then they are not allowed this item.
        let Some(n) = self.authorisations.get_mut(player).and_then(|x| x.get_mut(asset))
        else { return Err(Error::UnauthorisedWithdrawal{ asset: asset.clone(), amount_overdrawn: None}); };
        // Check to see if they can withdraw the entire amount
        if *n < count {
            return Err(Error::UnauthorisedWithdrawal{ asset: asset.clone(), amount_overdrawn: Some(count - *n)});
        }
        *n -= count;
        // Clean up the entry (or even the player) if they've used their entire allowance
        if *n == 0 {
            let player_auths = self.authorisations.get_mut(player).unwrap();
            player_auths.remove(asset);
            if player_auths.is_empty() {
                self.authorisations.remove(player);
            }
        }
        Ok(())
    }
}
