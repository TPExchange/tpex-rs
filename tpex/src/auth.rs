use serde::{Deserialize, Serialize};

use crate::{ids::HashMapCowExt, AccountId, Error, ItemId, Result, SharedId};

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct AuthSync {
    /// The restricted assets
    pub restricted: hashbrown::HashSet<ItemId<'static>>,
    /// The authorisations that various players have
    pub authorisations: hashbrown::HashMap<AccountId<'static>, hashbrown::HashMap<ItemId<'static>, u64>>,
    /// The shared accounts allowed to issue ETPs
    pub etp_authorised: hashbrown::HashSet<SharedId<'static>>,
}
impl From<&AuthTracker> for AuthSync {
    fn from(value: &AuthTracker) -> Self {
        AuthSync {
            restricted: value.restricted.clone(),
            authorisations: value.authorisations.clone(),
            etp_authorised: value.etp_authorised.clone()
        }
    }
}
impl TryFrom<AuthSync> for AuthTracker {
    type Error = Error;

    fn try_from(value: AuthSync) -> Result<AuthTracker> {
        Ok(AuthTracker {
            restricted: value.restricted,
            authorisations: value.authorisations,
            etp_authorised: value.etp_authorised
        })
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) struct AuthTracker {
    /// The restricted assets
    restricted: hashbrown::HashSet<ItemId<'static>>,
    /// The authorisations that various players have
    authorisations: hashbrown::HashMap<AccountId<'static>, hashbrown::HashMap<ItemId<'static>, u64>>,
    /// The shared accounts allowed to issue ETPs
    pub etp_authorised: hashbrown::HashSet<SharedId<'static>>,
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
            etp_authorised: Default::default()
        }
    }
    /// Returns true if the given item is currently restricted
    pub fn is_restricted(&self, asset: &ItemId) -> bool { self.restricted.contains(asset) }
    /// Lists all restricted items
    pub fn get_restricted(&self) -> &hashbrown::HashSet<ItemId<'static>> { &self.restricted }
    /// Sets the maximum amount a player is able to withdraw of a restricted item.
    ///
    /// XXX: This can and will nuke existing values, so check those race conditions
    pub fn set_authorisation(&mut self, player: AccountId, asset: ItemId, new_count: u64) {
        // Clean up the entry (or even the player) if they're being deauthed
        if new_count == 0 {
            let player_auths = self.authorisations.get_mut(player.as_ref()).unwrap();
            player_auths.remove(asset.as_ref());
            if player_auths.is_empty() {
                self.authorisations.remove(player.as_ref());
            }
        }
        else {
            self.authorisations.cow_get_or_default(player).1.insert(asset.into_owned(), new_count);
        }
    }
    /// Increases the maximum amount of an item a player is allowed to withdraw
    ///
    /// @returns The new limit the player has
    pub fn increase_authorisation<'player>(&mut self, player: AccountId<'player>, asset: ItemId, increase: u64) -> Result<u64> {
        self.authorisations.cow_get_or_default(player).1
            .cow_get_or_default(asset).1
            .checked_add(increase).ok_or(Error::Overflow)
    }
    /// Updates the list of restricted assets
    pub fn update_restricted(&mut self, restricted: hashbrown::HashSet<ItemId<'static>>) {
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
    pub fn check_withdrawal_authorized(&self, player: &AccountId, asset: &ItemId, count: u64) -> Result<()> {
        // If it's unrestricted, they can withdraw as much as they like
        if !self.is_restricted(asset) {
            return Ok(())
        }
        // Try to find the authorisation in the map. If it's not there, then they are not allowed this item.
        let Some(n) = self.authorisations.get(player).and_then(|x| x.get(asset)).copied()
        else { return Err(Error::UnauthorisedWithdrawal{ asset: asset.deep_clone(), amount_overdrawn: None}); };
        // Check to see if they can withdraw the entire amount
        if n < count {
            return Err(Error::UnauthorisedWithdrawal{ asset: asset.deep_clone(), amount_overdrawn: Some(count - n)});
        }
        Ok(())
    }
    /// Tries to remove assets for a player
    pub fn commit_withdrawal_authorized(&mut self, player: &AccountId, asset: &ItemId, count: u64) -> Result<()> {
        // If it's unrestricted, they can withdraw as much as they like
        if !self.is_restricted(asset) {
            return Ok(())
        }
        // Try to find the authorisation in the map. If it's not there, then they are not allowed this item.
        let Some(n) = self.authorisations.get_mut(player.as_ref()).and_then(|x| x.get_mut(asset.as_ref()))
        else { return Err(Error::UnauthorisedWithdrawal{ asset: asset.deep_clone(), amount_overdrawn: None}); };
        // Check to see if they can withdraw the entire amount
        if *n < count {
            return Err(Error::UnauthorisedWithdrawal{ asset: asset.deep_clone(), amount_overdrawn: Some(count - *n)});
        }
        *n -= count;
        // Clean up the entry (or even the player) if they've used their entire allowance
        if *n == 0 {
            let player_auths = self.authorisations.get_mut(player.as_ref()).unwrap();
            player_auths.remove(asset.as_ref());
            if player_auths.is_empty() {
                self.authorisations.remove(player.as_ref());
            }
        }
        Ok(())
    }
    /// Update the list of ETP authorised shared accounts
    pub fn update_etp_authorised(&mut self, etp_authorised: hashbrown::HashSet<SharedId<'static>>) {
        self.etp_authorised = etp_authorised;
    }
    /// Checks to see if a shared account is allowed to issue ETPs
    pub fn is_etp_authorised(&self, account: &SharedId) -> bool {
        self.etp_authorised.contains(account)
    }
}
