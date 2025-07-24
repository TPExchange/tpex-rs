use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde::de::Error;

use crate::{Action, PlayerId};

#[repr(transparent)]
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct SharedId(PlayerId);
impl SharedId {
    pub fn the_bank() -> SharedId { PlayerId::the_bank().try_into().unwrap() }
}
impl SharedId {
    pub fn parts(&self) -> impl DoubleEndedIterator<Item = &str> {
        // Skip the leading slash and split
        self.0.0[1..].split('/')
    }
    pub fn take_name(&self) -> (impl DoubleEndedIterator<Item = &str>, &str) {
        let last_delim_pos = self.0.0.rfind('/').unwrap();
        if last_delim_pos == 0 {
            (None.into_iter().flatten(), &self.0.0[last_delim_pos+1..])
        }
        else {
            (Some(self.0.0[1..last_delim_pos].split('/')).into_iter().flatten(), &self.0.0[last_delim_pos+1..])
        }
    }
    pub fn parent(&self) -> Option<SharedId> {
        let last_delim_pos = self.0.0.rfind('/').unwrap();
        if last_delim_pos == 0 {
            None
        }
        else {
            Some(SharedId(PlayerId::assume_username_correct(self.0.0[..last_delim_pos].to_string())))
        }
    }
    pub fn is_controlled_by(&self, other: &SharedId) -> bool {
        // Check that it is prefixed by other
        if !self.0.0.starts_with(&other.0.0) {
            return false;
        }
        match self.0.0.as_bytes().get(other.0.0.len()) {
            // If we are equal to other, then we are done
            None |
            // Otherwise, check to make sure that it's not just something that begins with the same string
            Some(b'/') => true,
            _ => false
        }
    }
}
impl TryFrom<PlayerId> for SharedId {
    type Error = PlayerId;

    fn try_from(value: PlayerId) -> Result<Self, Self::Error> {
        if value.0.ends_with('/') && value.0.len() != 1 {
            Err(value)
        }
        else if value.0.starts_with('/') {
            Ok(Self(value))
        }
        else {
            Err(value)
        }
    }
}
impl From<SharedId> for PlayerId {
    fn from(val: SharedId) -> Self {
        val.0
    }
}
impl AsRef<PlayerId> for SharedId {
    fn as_ref(&self) -> &PlayerId {
        &self.0
    }
}
impl Serialize for SharedId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}
impl<'a> Deserialize<'a> for SharedId {
    fn deserialize<D: serde::Deserializer<'a>>(deserializer: D) -> Result<Self, D::Error> {
        let inner = PlayerId::deserialize(deserializer)?;
        Self::try_from(inner)
        .map_err(|_inner| D::Error::custom("Expected leading slash for SharedId"))
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
struct Proposal {
    target: SharedId,
    action: Action,
    agree: HashSet<PlayerId>,
    disagree: HashSet<PlayerId>,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SharedAccount {
    /// The players who own the shared account
    owners: HashSet<PlayerId>,
    /// The minimum value of (agree - disagree) before a vote passes
    min_difference: u64,
    /// The minimum number of owners who need to vote in order for a proposal to be considered
    min_votes: u64,
    /// The accounts owned by this shared account
    children: HashMap<String, SharedAccount>,
}
impl SharedAccount {
    pub fn new(owners: HashSet<PlayerId>, min_difference: u64, min_votes: u64, children: HashMap<String, SharedAccount>) -> Result<Self, crate::Error> {
        // If consensus is trivial or impossible, this clearly was an error
        if
            min_difference > owners.len() as u64 || min_difference == 0 ||
            min_votes      > owners.len() as u64 || min_votes      == 0
        {
            Err(crate::Error::InvalidThreshold)
        }
        else {
            Ok(SharedAccount { owners, min_difference, min_votes, children })
        }
    }

    /// The players who own the shared account
    pub fn owners(&self) -> &HashSet<PlayerId> {
        &self.owners
    }

    fn get<'a, 'b>(&'b self, name: impl IntoIterator<Item=&'a str>) -> Option<&'b SharedAccount> {
        let mut iter = name.into_iter();
        match iter.next() {
            Some(next) => { self.children.get(next).and_then(|i| i.get(iter)) },
            None => Some(self)
        }
    }

    fn get_mut<'a, 'b>(&'b mut self, name: impl IntoIterator<Item=&'a str>) -> Option<&'b mut SharedAccount> {
        let mut iter = name.into_iter();
        match iter.next() {
            Some(next) => { self.children.get_mut(next).and_then(|i| i.get_mut(iter)) },
            None => Some(self)
        }
    }

    pub fn min_difference(&self) -> u64 {
        self.min_difference
    }

    pub fn min_votes(&self) -> u64 {
        self.min_votes
    }
}

// #[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
// struct SharedAccountSync {
//     /// The players who own the shared account
//     pub owners: HashSet<PlayerId>,
//     /// The minimum number of owners who need to agree in order to act on the account
//     ///
//     /// There is no separate number for management, because someone could just dump all the money out of the account as is
//     pub threshold: usize,
// }
// impl TryInto<SharedAccount> for SharedAccountSync {
//     type Error = Error;

//     fn try_into(self) -> Result<SharedAccount, Self::Error> {
//         SharedAccount::new(self.owners, self.threshold)
//     }
// }
// impl From<SharedAccount> for SharedAccountSync {
//     fn from(val: SharedAccount) -> Self {
//         SharedAccountSync { owners: val.owners, threshold: val.threshold }
//     }
// }

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SharedSync {
    bank: SharedAccount,
    proposals: BTreeMap<u64, Proposal>
}


#[derive(Clone, Debug)]
pub struct SharedTracker {
    bank: SharedAccount,
    proposals: BTreeMap<u64, Proposal>
}
impl SharedTracker {
    pub fn init() -> Self {
        Self {
            bank: SharedAccount::new([PlayerId::the_bank()].into(), 1, 1, Default::default()).unwrap(),
            proposals: Default::default()
        }
    }
    pub fn create_or_update(&mut self, id: SharedId, owners: HashSet<PlayerId>, min_difference: u64, min_votes: u64) -> Result<(), crate::Error> {
        let (parent, name) = id.take_name();
        // Look up the shared account's position in the tree
        match self.bank.get_mut(parent).ok_or(crate::Error::InvalidSharedId)?.children.entry(name.to_string()) {
            // If it exists, we edit the values
            std::collections::hash_map::Entry::Occupied(mut occupied_entry) => {
                let occupied_entry = occupied_entry.get_mut();
                occupied_entry.owners = owners;
                occupied_entry.min_difference = min_difference;
                occupied_entry.min_votes = min_votes;
            },
            std::collections::hash_map::Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(SharedAccount::new(owners, min_difference, min_votes, Default::default())?);
            }
        }
        Ok(())
    }
    pub fn is_owner(&self, id: &SharedId, player: &PlayerId) -> Result<bool, crate::Error> {
        self.bank.get(id.parts())
            .ok_or(crate::Error::InvalidSharedId)
            .map(|account| account.owners.contains(player))
    }
    pub fn add_proposal(&mut self, id: u64, target: SharedId, action: Action) -> Result<(), crate::Error> {
        if self.bank.get(target.parts()).is_none() {
            return Err(crate::Error::InvalidSharedId)
        }
        self.proposals.insert(id, Proposal { action, target, agree: Default::default(), disagree: Default::default()});
        Ok(())
    }
    pub fn vote(&mut self, id: u64, player: PlayerId, agree: bool) -> Result<Option<Action>, crate::Error> {
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
            proposal.get_mut().disagree.remove(&player);
            proposal.get_mut().agree.insert(player);
        }
        else {
            proposal.get_mut().agree.remove(&player);
            proposal.get_mut().disagree.insert(player);
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
    pub fn close(&mut self, id: &SharedId) -> Result<(), crate::Error> {
        // You can't wind up the bank, it has no parent and would cause terrible issues!
        if id == &SharedId::the_bank() {
            return Err(crate::Error::InvalidSharedId)
        }
        // Get the parent, and remove the child
        let (parent, name) = id.take_name();
        let parent = self.bank.get_mut(parent).ok_or(crate::Error::InvalidSharedId)?;
        let _target = parent.children.remove(name).ok_or(crate::Error::InvalidSharedId)?;
        // Remove the proposals
        self.proposals.retain(|_, proposal| &proposal.target != id);
        Ok(())
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
