use crate::Coins;

use super::{AssetId, Audit, Auditable, Error, PlayerId};

#[derive(Debug, Clone)]
pub struct PendingWithdrawl {
    pub id: u64,
    pub player: PlayerId,
    pub assets: std::collections::HashMap<AssetId, u64>,
    pub expedited: bool,
    pub total_fee: Coins
}

#[derive(Debug, Default, Clone)]
pub struct WithdrawalTracker {
    pending_normal_withdrawals: std::collections::BTreeMap<u64, PendingWithdrawl>,
    pending_expedited_withdrawals: std::collections::BTreeMap<u64, PendingWithdrawl>,

    current_audit: Audit
}
impl WithdrawalTracker {
    /// List all expedited
    pub fn get_expedited_withdrawals(&self) -> std::collections::BTreeMap<u64, PendingWithdrawl> { self.pending_expedited_withdrawals.clone() }
    /// List all withdrawals
    pub fn get_withdrawals(&self) -> std::collections::BTreeMap<u64, PendingWithdrawl> {
        let mut ret = self.pending_normal_withdrawals.clone();
        ret.extend(self.get_expedited_withdrawals());
        ret
    }
    /// Get a withdrawal
    pub fn get_withdrawal(&self, id: u64) -> Result<PendingWithdrawl, Error> {
        self.pending_normal_withdrawals.get(&id)
        .or_else(|| self.pending_expedited_withdrawals.get(&id)).cloned()
        .ok_or(Error::InvalidId { id })
    }
    /// Get the next withdrawal that bankers should complete
    pub fn get_next_withdrawal(&self) -> Option<PendingWithdrawl> {
        self.pending_expedited_withdrawals.values().next().or_else(|| self.pending_normal_withdrawals.values().next()).cloned()
    }
    pub fn track_withdrawal(&mut self, id: u64, player: PlayerId, assets: std::collections::HashMap<AssetId, u64>, total_fee: Coins) {
        self.pending_normal_withdrawals.insert(id, PendingWithdrawl{ id, player, assets: assets.clone(), expedited: false, total_fee });
        self.current_audit += Audit{coins: total_fee, assets}
    }
    pub fn expedite(&mut self, id: u64, fee: Coins) -> Result<(), Error> {
        // Try to find this withdrawal
        let std::collections::btree_map::Entry::Occupied(entry) = self.pending_normal_withdrawals.entry(id)
        else { return Err(Error::InvalidId { id }); };
        // Remove them from the normal list
        let mut entry = entry.remove();
        // Give them the expedited flag, and track the money
        entry.expedited = true;
        entry.total_fee.checked_add_assign(fee).expect("Withdraw fee overflow");
        self.current_audit.add_coins(fee);
        // Insert them into the expedited list
        self.pending_expedited_withdrawals.insert(id, entry);
        Ok(())
    }
    pub fn complete(&mut self, id: u64) -> Result<PendingWithdrawl, Error> {
        // Try to take out the pending transaction
        let Some(res) = self.pending_normal_withdrawals.remove(&id).or_else(|| self.pending_expedited_withdrawals.remove(&id))
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
        for withdrawal in self.pending_normal_withdrawals.values().chain(self.pending_expedited_withdrawals.values()) {
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
