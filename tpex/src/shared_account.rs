use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Display;
use std::ops::{Div, DivAssign};
use std::str::FromStr;

use const_format::concatcp;
use serde::{Deserialize, Serialize};
use serde::de::Error;

use crate::{is_safe_name, Action, PlayerId};

pub const SHARED_ACCOUNT_DELIM: char = '.';

/// The checked name of a shared acount, with path syntax
///
/// i.e. If /foo creates an account bar, then it will be called /foo/bar
#[repr(transparent)]
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct SharedId(PlayerId);
impl SharedId {
    pub fn the_bank() -> SharedId { PlayerId::the_bank().try_into().unwrap() }
}
impl SharedId {
    pub const fn is_bank(&self) -> bool {
        self.0.0.len() == 1
    }
    pub fn parts(&self) -> impl DoubleEndedIterator<Item = &str> {
        // If this is the bank, there are no parts
        if self.is_bank() {
            None.into_iter().flatten()
        }
        else {
            // Skip the leading slash and split
            Some(self.0.0[1..].split(SHARED_ACCOUNT_DELIM)).into_iter().flatten()
        }
    }
    pub fn take_name(&self) -> Option<(impl DoubleEndedIterator<Item = &str>, &str)> {
        if self.is_bank() {
            return None
        }
        let last_delim_pos = self.0.0.rfind(SHARED_ACCOUNT_DELIM).unwrap();
        if last_delim_pos == 0 {
            Some((None.into_iter().flatten(), &self.0.0[last_delim_pos+1..]))
        }
        else {
            Some((Some(self.0.0[1..last_delim_pos].split(SHARED_ACCOUNT_DELIM)).into_iter().flatten(), &self.0.0[last_delim_pos+1..]))
        }
    }
    pub fn parent(&self) -> Option<SharedId> {
        if self.is_bank() {
            return None;
        }
        let last_delim_pos = self.0.0.rfind(SHARED_ACCOUNT_DELIM).unwrap();
        if last_delim_pos == 0 {
            Some(SharedId::the_bank())
        }
        else {
            Some(SharedId(PlayerId::assume_username_correct(self.0.0[..last_delim_pos].to_string())))
        }
    }
    pub fn try_concat(mut self, name: &str) -> Result<Self, Self> {
        if name.contains(SHARED_ACCOUNT_DELIM) {
            Err(self)
        }
        else {
            self.0.0.reserve(name.len() + 1);
            self.0.0.push(SHARED_ACCOUNT_DELIM);
            self.0.0.push_str(name);
            Ok(self)
        }
    }
    pub fn is_controlled_by(&self, other: &SharedId) -> bool {
        // Check to see if it's just the bank
        if other.0.0.len() == 1 {
            return true;
        }
        // Check that it is prefixed by other
        if !self.0.0.starts_with(&other.0.0) {
            return false;
        }
        // Check that the string is either the same, or that the prefix is terminated by a slash
        match self.0.0.as_bytes().get(other.0.0.len()) {
            None => true,
            Some(val) if *val == SHARED_ACCOUNT_DELIM as u8 => true,
            _ => false
        }
    }
}
impl DivAssign for SharedId {
    // We are using pathing syntax, so this really should be a `+=`
    #[allow(clippy::suspicious_op_assign_impl)]
    fn div_assign(&mut self, rhs: Self) {
        self.0.0 += &rhs.0.0
    }
}
impl Div for SharedId {
    type Output = Self;

    fn div(mut self, rhs: Self) -> Self::Output {
        self /= rhs;
        self
    }
}
impl TryFrom<PlayerId> for SharedId {
    type Error = PlayerId;

    fn try_from(value: PlayerId) -> Result<Self, Self::Error> {
        // It's the bank!
        if value.0 == concatcp!(SHARED_ACCOUNT_DELIM) {
            return Ok(SharedId(value));
        }
        if !value.0.starts_with(SHARED_ACCOUNT_DELIM) || value.0.ends_with(SHARED_ACCOUNT_DELIM) || value.0 == concatcp!(SHARED_ACCOUNT_DELIM, SHARED_ACCOUNT_DELIM) {
            return Err(value);
        }
        let ret = Self(value);
        if !ret.parts().all(is_safe_name) {
            return Err(ret.0);
        }
        Ok(ret)
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
impl FromStr for SharedId {
    type Err = PlayerId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        PlayerId::assume_username_correct(s.to_owned()).try_into()
    }
}
impl TryFrom<String> for SharedId {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        PlayerId::assume_username_correct(value).try_into().map_err(|i: PlayerId| i.0)
    }
}
impl Display for SharedId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Proposal {
    pub target: SharedId,
    pub action: Action,
    pub agree: HashSet<PlayerId>,
    pub disagree: HashSet<PlayerId>,
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

    pub fn bottom_up(&self, base: SharedId, func: &mut impl FnMut(SharedId, &SharedAccount)) {
        for (name, account) in &self.children {
            account.bottom_up(base.clone().try_concat(name).unwrap(), func);
        }
        func(base, self)
    }

    pub fn children(&self) -> &HashMap<String, SharedAccount> {
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
            bank: SharedAccount::new([PlayerId::the_bank()].into(), 1, 1, Default::default()).unwrap(),
            proposals: Default::default()
        }
    }
    pub fn create_or_update(&mut self, id: SharedId, owners: HashSet<PlayerId>, min_difference: u64, min_votes: u64) -> Result<(), crate::Error> {
        if
            min_difference > owners.len() as u64 ||
            min_votes > owners.len() as u64 ||
            min_votes == 0
        {
            return Err(crate::Error::InvalidThreshold);
        }
        let target: &mut SharedAccount = match id.take_name() {
            Some((parent, name)) => {
                // Look up the shared account's position in the tree
                match self.bank.get_mut(parent).ok_or(crate::Error::InvalidSharedId)?.children.entry(name.to_string()) {
                    // If it exists, we edit the values
                    std::collections::hash_map::Entry::Occupied(occupied_entry) => {
                        occupied_entry.into_mut()
                    },
                    std::collections::hash_map::Entry::Vacant(vacant_entry) => {
                        vacant_entry.insert(SharedAccount::new(owners, min_difference, min_votes, Default::default())?);
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
    pub fn is_owner(&self, id: &SharedId, player: &PlayerId) -> Result<bool, crate::Error> {
        // For directly proxied companies
        if player == &id.0 {
            return Ok(true)
        }
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
    pub fn wind_up(&mut self, id: SharedId, mut clean_one: impl FnMut(&SharedId)) -> Result<(), crate::Error> {
        // Get the parent, and remove the child
        let Some((parent, name)) = id.take_name()
        // You can't wind up the bank, it has cyclic parent and would cause terrible issues!
        else { return Err(crate::Error::InvalidSharedId) };
        let parent = self.bank.get_mut(parent).ok_or(crate::Error::InvalidSharedId)?;
        let target = parent.children.remove(name).ok_or(crate::Error::InvalidSharedId)?;
        // Grab a list of all the accounts being destroyed
        let mut to_remove = std::collections::HashSet::new();
        target.bottom_up(id, &mut |i, _| {
            clean_one(&i);
            to_remove.insert(i);
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
