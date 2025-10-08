use std::collections::{BTreeMap};
use hashbrown::{HashMap, HashSet};
use serde::{Deserialize, Serialize};

use crate::{AccountId, Action, SharedId, UnsharedId};

pub const SHARED_ACCOUNT_DELIM: char = '.';

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Proposal {
    pub target: SharedId<'static>,
    pub action: Action<'static>,
    pub agree: HashSet<AccountId<'static>>,
    pub disagree: HashSet<AccountId<'static>>,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SharedAccount {
    /// The players who own the shared account
    owners: HashSet<AccountId<'static>>,
    /// The minimum value of (agree - disagree) before a vote passes
    min_difference: u64,
    /// The minimum number of owners who need to vote in order for a proposal to be considered
    min_votes: u64,
    /// The accounts owned by this shared account
    children: HashMap<UnsharedId<'static>, SharedAccount>,
}
impl SharedAccount {
    pub fn new(owners: HashSet<AccountId<'static>>, min_difference: u64, min_votes: u64, children: HashMap<UnsharedId<'static>, SharedAccount>) -> Result<Self, crate::Error> {
        // If consensus is trivial or impossible, this clearly was an error
        if
            min_difference > owners.len() as u64 ||
            min_votes > owners.len() as u64 ||
            min_votes == 0
        {
            Err(crate::Error::InvalidThreshold)
        }
        else {
            Ok(SharedAccount { owners, min_difference, min_votes, children })
        }
    }

    /// The players who own the shared account
    pub fn owners(&'_ self) -> &'_ HashSet<AccountId<'_>> {
        &self.owners
    }

    fn get<'a, 'b>(&'b self, name: impl IntoIterator<Item=UnsharedId<'a>>) -> Option<&'b SharedAccount> {
        let mut iter = name.into_iter();
        match iter.next() {
            Some(next) => { self.children.get(next.as_ref()).and_then(|i| i.get(iter)) },
            None => Some(self)
        }
    }

    fn get_mut<'a, 'b>(&'b mut self, name: impl IntoIterator<Item=UnsharedId<'a>>) -> Option<&'b mut SharedAccount> {
        let mut iter = name.into_iter();
        match iter.next() {
            Some(next) => { self.children.get_mut(next.as_ref()).and_then(|i| i.get_mut(iter)) },
            None => Some(self)
        }
    }

    pub fn min_difference(&self) -> u64 {
        self.min_difference
    }

    pub fn min_votes(&self) -> u64 {
        self.min_votes
    }

    pub fn bottom_up(&self, base: SharedId, func: &mut impl FnMut(SharedId, &SharedAccount)) {
        for (name, account) in &self.children {
            account.bottom_up(base.shallow_clone() / name.shallow_clone(), func);
        }
        func(base, self)
    }

    pub fn children(&self) -> &HashMap<UnsharedId<'static>, SharedAccount> {
        &self.children
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SharedSync {
    pub bank: SharedAccount,
    pub proposals: BTreeMap<u64, Proposal>
}


#[derive(Clone, Debug)]
pub struct SharedTracker {
    bank: SharedAccount,
    proposals: BTreeMap<u64, Proposal>
}
impl SharedTracker {
    pub fn init() -> Self {
        Self {
            bank: SharedAccount::new([AccountId::THE_BANK].into(), 1, 1, Default::default()).unwrap(),
            proposals: Default::default()
        }
    }
    pub fn create_or_update(&mut self, id: SharedId, owners: HashSet<AccountId<'static>>, min_difference: u64, min_votes: u64) -> Result<(), crate::Error> {
        if
            min_difference > owners.len() as u64 ||
            min_votes > owners.len() as u64 ||
            min_votes == 0
        {
            return Err(crate::Error::InvalidThreshold);
        }
        let mut parts = id.parts();
        let target: &mut SharedAccount = match parts.next_back() {
            Some(name) => {
                // Look up the shared account's position in the tree
                match self.bank.get_mut(parts).ok_or(crate::Error::InvalidSharedId)?.children.raw_entry_mut().from_key(name.as_ref()) {
                    // If it exists, we edit the values
                    hashbrown::hash_map::RawEntryMut::Occupied(occupied_entry) => {
                        occupied_entry.into_mut()
                    },
                    hashbrown::hash_map::RawEntryMut::Vacant(vacant_entry) => {
                        vacant_entry.insert(name.into_owned(), SharedAccount::new(owners, min_difference, min_votes, Default::default())?);
                        return Ok(())
                    }
                }
            }
            // If this is the root, return the root
            None => &mut self.bank
        };
        target.owners = owners;
        target.min_difference = min_difference;
        target.min_votes = min_votes;
        Ok(())
    }
    pub fn is_owner(&self, id: &SharedId, player: &AccountId) -> Result<bool, crate::Error> {
        // For directly proxied companies
        if player == id {
            return Ok(true)
        }
        self.bank.get(id.parts())
            .ok_or(crate::Error::InvalidSharedId)
            .map(|account| account.owners.contains(player))
    }
    pub fn add_proposal(&mut self, id: u64, target: SharedId, action: Action<'static>) -> Result<(), crate::Error> {
        if self.bank.get(target.parts()).is_none() {
            return Err(crate::Error::InvalidSharedId)
        }
        self.proposals.insert(id, Proposal { action, target: target.into_owned(), agree: Default::default(), disagree: Default::default()});
        Ok(())
    }
    pub fn vote(&mut self, id: u64, player: AccountId, agree: bool) -> Result<Option<Action<'static>>, crate::Error> {
        // Look up the proposal
        let std::collections::btree_map::Entry::Occupied(mut proposal) = self.proposals.entry(id)
        else { return Err(crate::Error::InvalidId { id }) };
        // Find the relevant account
        let target = self.bank.get(proposal.get().target.parts()).expect("Inconsistent proposal");
        // Check that this player actually can vote
        if !target.owners().contains(&player) {
            return Err(crate::Error::UnauthorisedShared)
        }
        // Try to remove the player from the side they are not on (it doesn't matter if they didn't vote that way anyway)
        if agree {
            proposal.get_mut().disagree.remove(player.as_ref());
            proposal.get_mut().agree.insert(player.into_owned());
        }
        else {
            proposal.get_mut().agree.remove(player.as_ref());
            proposal.get_mut().disagree.insert(player.into_owned());
        }
        // Check to see if we've reached threshold
        //
        // It may seem counter-intuitive that a "disagree" vote could trigger a pass,
        // but this is less silly than vote order mattering more than it already does
        let n_agree = proposal.get().agree.len() as u64;
        let n_disagree = proposal.get().disagree.len() as u64;
        if n_agree + n_disagree >= target.min_votes() {
            // Check to see if we have more agrees than disagrees...
            if let Some(difference) = n_agree.checked_sub(n_disagree) {
                // ... and specifically at least min_difference more ...
                if difference >= target.min_difference() {
                    // ... then we can perform the action, and remove it from our list
                    //
                    // Note that we want to remove it even if the action fails, as otherwise there is no good way of retriggering it
                    //
                    // The returned action was also checked to belong to target when it was passed here by the Propose action,
                    // and the actual authorisations will be checked on apply
                    let Proposal { action, .. } = proposal.remove();
                    return Ok(Some(action))
                }
            }
        }
        Ok(None)
    }
    pub fn wind_up(&mut self, id: SharedId, mut clean_one: impl FnMut(&SharedId)) -> Result<(), crate::Error> {
        // Get the parent, and remove the child
        let mut parent = id.parts();
        let Some(name) = parent.next_back()
        // You can't wind up the bank, it has cyclic parent and would cause terrible issues!
        else { return Err(crate::Error::InvalidSharedId) };
        let parent = self.bank.get_mut(parent).ok_or(crate::Error::InvalidSharedId)?;
        let target = parent.children.remove(name.as_ref()).ok_or(crate::Error::InvalidSharedId)?;
        // Grab a list of all the accounts being destroyed
        let mut to_remove = std::collections::HashSet::new();
        target.bottom_up(id, &mut |i, _| {
            clean_one(&i);
            to_remove.insert(i.into_owned());
        });
        // Remove the proposals
        self.proposals.retain(|_, proposal| !to_remove.contains(&proposal.target));
        Ok(())
    }
    pub fn contains(&self, id: &SharedId) -> bool {
        self.bank.get(id.parts()).is_some()
    }
    pub fn the_bank(&self) -> &SharedAccount {
        &self.bank
    }
}

impl From<&SharedTracker> for SharedSync {
    fn from(value: &SharedTracker) -> Self {
        SharedSync {
            bank: value.bank.clone(),
            proposals: value.proposals.clone(),
        }
    }
}
impl TryFrom<SharedSync> for SharedTracker {
    type Error = crate::Error;
    fn try_from(SharedSync { bank, proposals }: SharedSync) -> Result<Self, Self::Error> {
        for proposal in proposals.values() {
            if bank.get(proposal.target.parts()).is_none() {
                return Err(crate::Error::InvalidFastSync)
            }
        }
        Ok(SharedTracker {
            bank,
            proposals
        })
    }
}
