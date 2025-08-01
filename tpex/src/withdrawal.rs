use serde::{Deserialize, Serialize};

use crate::Coins;

use super::{AssetId, Audit, Auditable, Error, PlayerId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingSync {
    pub player: PlayerId,
    pub assets: std::collections::HashMap<AssetId, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WithdrawalSync {
    pub pending_withdrawals: std::collections::BTreeMap<u64, PendingSync>
}
impl From<&WithdrawalTracker> for WithdrawalSync {
    fn from(value: &WithdrawalTracker) -> Self {
        WithdrawalSync {
            pending_withdrawals:
                value.pending_withdrawals.values()
                .map(|PendingWithdrawal { id, player, assets, total_fee: _total_fee }|
                    (*id, PendingSync { player: player.clone(), assets: assets.clone() })
                )
                .collect()
        }
    }
}
impl TryFrom<WithdrawalSync> for WithdrawalTracker {
    type Error = Error;
    fn try_from(value: WithdrawalSync) -> Result<Self, Error> {
        let mut current_audit = Audit::default();
        Ok(WithdrawalTracker {
            pending_withdrawals:
                value.pending_withdrawals.into_iter()
                .map(|(id, PendingSync { player, assets })| {
                    for (asset, count) in &assets {
                        current_audit.add_asset(asset.clone(), *count);
                    }
                    (id, PendingWithdrawal { player, assets, id, total_fee: Coins::default() })
                })
                .collect(),
            current_audit
        })
    }
}

#[derive(Debug, Clone)]
pub struct PendingWithdrawal {
    pub id: u64,
    pub player: PlayerId,
    pub assets: std::collections::HashMap<AssetId, u64>,
    pub total_fee: Coins
}

#[derive(Debug, Default, Clone)]
pub(crate) struct WithdrawalTracker {
    pending_withdrawals: std::collections::BTreeMap<u64, PendingWithdrawal>,

    current_audit: Audit
}
impl WithdrawalTracker {
    /// List all withdrawals
    pub fn get_withdrawals(&self) -> std::collections::BTreeMap<u64, PendingWithdrawal> {
        self.pending_withdrawals.clone()
    }
    /// Get a withdrawal
    pub fn get_withdrawal(&self, id: u64) -> Result<PendingWithdrawal, Error> {
        self.pending_withdrawals.get(&id)
        .cloned()
        .ok_or(Error::InvalidId { id })
    }
    /// Get the next withdrawal that bankers should complete
    pub fn get_next_withdrawal(&self) -> Option<PendingWithdrawal> {
        self.pending_withdrawals.values().next().cloned()
    }
    pub fn track_withdrawal(&mut self, id: u64, player: PlayerId, assets: std::collections::HashMap<AssetId, u64>, total_fee: Coins) {
        self.pending_withdrawals.insert(id, PendingWithdrawal{ id, player, assets: assets.clone(), total_fee });
        self.current_audit += Audit{coins: total_fee, assets}
    }
    pub fn complete(&mut self, id: u64) -> Result<PendingWithdrawal, Error> {
        // Try to take out the pending transaction
        let Some(res) = self.pending_withdrawals.remove(&id)
        else { return Err(Error::InvalidId{id}); };
        // We are no longer responsible for the fee
        self.current_audit.sub_coins(res.total_fee);
        // We no longer have the items
        for (asset, count) in res.assets.iter() {
            self.current_audit.sub_asset(asset.clone(), *count);
        }
        Ok(res)
    }
}
impl Auditable for WithdrawalTracker {
    fn soft_audit(&self) -> Audit { self.current_audit.clone() }

    fn hard_audit(&self) -> Audit {
        let mut new_audit = Audit::default();
        for withdrawal in self.pending_withdrawals.values() {
            for (asset, count) in &withdrawal.assets {
                new_audit.add_asset(asset.clone(), *count);
            }
            new_audit.add_coins(withdrawal.total_fee);
        }
        if new_audit != self.current_audit {
            panic!("Recalculated withdrawal audit differs from soft audit");
        }
        new_audit
    }
}
