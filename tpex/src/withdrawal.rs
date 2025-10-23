use serde::{Deserialize, Serialize};

use crate::ItemId;

use super::{Audit, Auditable, Error, AccountId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingSync {
    pub player: AccountId<'static>,
    pub assets: hashbrown::HashMap<ItemId<'static>, u64>,
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
                .map(|PendingWithdrawal { id, player, assets }|
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
                        current_audit.add_asset(asset.shallow_clone().into(), *count);
                    }
                    (id, PendingWithdrawal { player, assets, id })
                })
                .collect(),
            current_audit
        })
    }
}

#[derive(Clone, Debug)]
pub struct PendingWithdrawal {
    pub id: u64,
    pub player: AccountId<'static>,
    pub assets: hashbrown::HashMap<ItemId<'static>, u64>
}
// impl<'a> PendingWithdrawal<'a> {
//     fn shallow_clone(&'a self) -> Self {
//         Self {
//             id: self.id,
//             player: self.player.shallow_clone(),
//             assets: self.assets.iter().map(|(k, v)| (k.shallow_clone(), *v)).collect()
//         }
//     }
// }

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
    pub fn get_withdrawal(&self, id: u64) -> Result<&PendingWithdrawal, Error> {
        self.pending_withdrawals.get(&id)
        .ok_or(Error::InvalidId { id })
    }
    /// Get the next withdrawal that bankers should complete
    pub fn get_next_withdrawal(&self) -> Option<&PendingWithdrawal> {
        self.pending_withdrawals.values().next()
    }
    pub fn track(&mut self, id: u64, player: AccountId, assets: hashbrown::HashMap<ItemId<'static>, u64>)  {
        for (asset, count) in &assets {
            self.current_audit.add_asset(asset.shallow_clone().into(), *count);
        }
        self.pending_withdrawals.insert(id, PendingWithdrawal{ id, player: player.into_owned(), assets: assets.clone() });
    }
    /// Stops tracking the withdrawal, either for a completion or a cancel
    pub fn finalise(&mut self, id: u64) -> Result<PendingWithdrawal, Error> {
        // Try to take out the pending transaction
        let Some(res) = self.pending_withdrawals.remove(&id)
        else { return Err(Error::InvalidId{id}); };
        // We no longer have the items
        for (asset, count) in res.assets.iter() {
            self.current_audit.sub_asset(&asset.shallow_clone().into(), *count);
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
                new_audit.add_asset(asset.shallow_clone().into(), *count);
            }
        }
        if new_audit != self.current_audit {
            panic!("Recalculated withdrawal audit differs from soft audit");
        }
        new_audit
    }
}
