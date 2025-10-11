//! The various Clone on Write ID types
//!
//! These use the type system to enforce correctness of IDs throughout the code, and prevent you mixing up different types of IDs
//!
//! Try to avoid using `.clone()` on these types, which defaults to a deep copy of the string.
//! Instead, make your decision explicit with `deep_clone` or `shallow_clone`

use std::{borrow::{Borrow, Cow}, fmt::{Debug, Display}, hash::{BuildHasher, Hash, Hasher}, ops::{Deref, Div, DivAssign}, str::FromStr};

use const_format::concatcp;
use serde::{Deserialize, Serialize};
use serde::de::Error;

use crate::{is_safe_name, ETP_DELIM, SHARED_ACCOUNT_DELIM};

#[derive(Debug, Clone)]
pub struct IdParseError<'a>(pub Cow<'a, str>);
impl<'a> From<Cow<'a, str>> for IdParseError<'a> {
    fn from(value: Cow<'a, str>) -> Self {
        Self(value)
    }
}
impl<'a> From<IdParseError<'a>> for Cow<'a, str> {
    fn from(value: IdParseError<'a>) -> Self {
        value.0
    }
}
impl<'a> Deref for IdParseError<'a> {
    type Target = Cow<'a, str>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Display for IdParseError<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to parse the following as an ID {:?}", self.0)
    }
}
impl std::error::Error for IdParseError<'_> {}

pub trait HashMapCowExt<'this, Search, K, V> {
    fn cow_get_or_default(&'this mut self, key: Search) -> (&'this mut K, &'this mut V);//hashbrown::hash_map::RawEntryMut<'a, K, V, S>;
}
macro_rules! common_impl {
    ($type:ident)  => {
        impl<'a> TryFrom<String> for $type<'a> {
            type Error = IdParseError<'a>;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::try_from(Cow::from(value))
            }
        }
        impl<'a> TryFrom<&'a str> for $type<'a> {
            type Error = IdParseError<'a>;
            fn try_from(value: &'a str) -> Result<Self, Self::Error> {
                Self::try_from(Cow::from(value))
            }
        }
        impl<'a> AsRef<str> for $type<'a> {
            fn as_ref(&self) -> &str {
                self.deref().as_ref()
            }
        }
        impl<'a> Deserialize<'a> for $type<'_> {
            fn deserialize<D: serde::Deserializer<'a>>(deserializer: D) -> Result<Self, D::Error> {
                let inner = Cow::deserialize(deserializer)?;
                Self::try_from(inner)
                .map_err(|_inner| D::Error::custom("Invalid AccountId"))
            }
        }
        impl Serialize for $type<'_> {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                self.deref().serialize(serializer)
            }
        }
        impl Display for $type<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                Display::fmt(&self.deref(), f)
            }
        }
        impl Borrow<str> for $type<'_> {
            fn borrow(&self) -> &str {
                self.deref()
            }
        }
        impl<'a> From<&'a $type<'a>> for $type<'a> {
            fn from(x: &'a $type<'a>) -> Self {
                x.shallow_clone()
            }
        }
        impl $type<'_> {
            pub fn deep_clone(&self) -> $type<'static> {
                self.clone().into_owned()
            }
        }
        impl<'a> Hash for $type<'a> {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.deref().hash(state)
            }
        }
        impl<T: AsRef<str>> PartialEq<T> for $type<'_> {
            fn eq(&self, other: &T) -> bool {
                self.deref() == other.as_ref()
            }
        }
        impl Eq for $type<'_> {}
        impl<'this, 'key, V: Default, S: BuildHasher> HashMapCowExt<'this, $type<'_>, $type<'key>, V> for hashbrown::HashMap<$type<'key>, V, S> {
            fn cow_get_or_default(&mut self, search: $type<'_>) -> (&mut $type<'key>, &mut V) {
                self.raw_entry_mut().from_key(search.as_ref()).or_insert_with(move || (search.into_owned(), Default::default()))
            }
        }
        // We don't get to pick the lifetime, so we have to clone
        impl<'a> FromStr for $type<'a> {
            type Err = IdParseError<'a>;
            fn from_str(x: &str) -> Result<Self, Self::Err> {
                x.to_owned().try_into()
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum AccountId<'a> {
    Unshared(UnsharedId<'a>),
    Shared(SharedId<'a>)
}
impl<'a> AccountId<'a> {
    pub const THE_BANK: AccountId<'static> = AccountId::Shared(SharedId::THE_BANK);
    pub fn is_bank(&self) -> bool { self == &Self::THE_BANK }
    pub fn into_owned(self) -> AccountId<'static> {
        match self {
            AccountId::Unshared(single_id) => AccountId::Unshared(single_id.into_owned()),
            AccountId::Shared(shared_id) => AccountId::Shared(shared_id.into_owned()),
        }
    }
    pub fn shallow_clone(&'a self) -> Self {
        match self {
            AccountId::Unshared(single_id) => AccountId::Unshared(single_id.shallow_clone()),
            AccountId::Shared(shared_id) => AccountId::Shared(shared_id.shallow_clone()),
        }
    }
}
impl<'a> Deref for AccountId<'a> {
    type Target = Cow<'a, str>;

    fn deref(&self) -> &Self::Target {
        match self {
            AccountId::Unshared(x) => x.deref(),
            AccountId::Shared(x) => x.deref(),
        }
    }
}
impl<'a> TryFrom<Cow<'a, str>> for AccountId<'a> {
    type Error = IdParseError<'a>;
    fn try_from(value: Cow<'a, str>) -> Result<Self, Self::Error> {
        value.try_into().map(Self::Shared)
        .or_else(|IdParseError(i)| i.try_into().map(Self::Unshared))
    }
}
common_impl!(AccountId);
impl<'a> From<UnsharedId<'a>> for AccountId<'a> {
    fn from(value: UnsharedId<'a>) -> Self {
        Self::Unshared(value)
    }
}
impl<'a> From<SharedId<'a>> for AccountId<'a> {
    fn from(value: SharedId<'a>) -> Self {
        Self::Shared(value)
    }
}
impl<'a> From<&'a UnsharedId<'a>> for AccountId<'a> {
    fn from(value: &'a UnsharedId<'a>) -> Self {
        Self::Unshared(value.shallow_clone())
    }
}
impl<'a> From<&'a SharedId<'a>> for AccountId<'a> {
    fn from(value: &'a SharedId<'a>) -> Self {
        Self::Shared(value.shallow_clone())
    }
}
impl<'a> TryFrom<AccountId<'a>> for UnsharedId<'a> {
    type Error = AccountId<'a>;

    fn try_from(value: AccountId<'a>) -> Result<Self, Self::Error> {
        match value {
            AccountId::Unshared(x) => Ok(x),
            x => Err(x),
        }
    }
}
impl<'a> TryFrom<AccountId<'a>> for SharedId<'a> {
    type Error = AccountId<'a>;

    fn try_from(value: AccountId<'a>) -> Result<Self, Self::Error> {
        match value {
            AccountId::Shared(x) => Ok(x),
            x => Err(x),
        }
    }
}

#[derive(Clone, Debug)]
pub struct UnsharedId<'a>(Cow<'a, str>);
impl<'a> UnsharedId<'a> {
    /// Creates a SingleId without validating it
    ///
    /// # Safety
    ///
    /// The given string should pass is_safe_name
    pub unsafe fn unvalidated(x: Cow<'a, str>) -> Self { Self(x) }
    pub fn into_owned(self) -> UnsharedId<'static> { UnsharedId(Cow::Owned(self.0.into_owned())) }
    pub fn shallow_clone(&'a self) -> Self { Self(Cow::Borrowed(&self.0)) }
}
impl<'a> TryFrom<Cow<'a, str>> for UnsharedId<'a> {
    type Error = IdParseError<'a>;

    fn try_from(value: Cow<'a, str>) -> Result<Self, Self::Error> {
        if is_safe_name(value.as_ref()) {
            Ok(Self(value))
        }
        else {
            Err(value.into())
        }
    }
}
impl<'a> Deref for UnsharedId<'a> {
    type Target = Cow<'a, str>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
common_impl!(UnsharedId);

/// The checked name of a shared acount, with path syntax
///
/// i.e. If .foo creates an account bar, then it will be called .foo.bar
#[repr(transparent)]
#[derive(Clone, Debug)]
pub struct SharedId<'a>(Cow<'a, str>);
impl<'a> SharedId<'a> {
    pub const THE_BANK: SharedId<'static> = SharedId(Cow::Borrowed(concatcp!(SHARED_ACCOUNT_DELIM)));
    /// Creates a SharedId without validating it
    ///
    /// # Safety
    ///
    /// The given string, when split along SHARED_ACCOUNT_DELIM, should have parts that are valid PlayerIds
    pub unsafe fn unvalidated(x: Cow<'a, str>) -> Self { Self(x) }
    pub fn into_owned(self) -> SharedId<'static> { SharedId(Cow::Owned(self.0.into_owned())) }
    pub fn shallow_clone(&'a self) -> Self { Self(Cow::Borrowed(&self.0)) }
    pub fn is_bank(&self) -> bool { self.len() == 1 }
    pub fn parts<'this>(&'this self) -> impl DoubleEndedIterator<Item = UnsharedId<'this>> {
        // If this is the bank, there are no parts
        if self.is_bank() {
            None.into_iter().flatten()
        }
        else {
            // Skip the leading slash and split
            Some(self.0[1..].split(SHARED_ACCOUNT_DELIM)).into_iter().flatten()

        }
        // SAFETY: we've already validated the path, so these ids are fine
        .map(|i| unsafe{UnsharedId::unvalidated(i.into())})
    }
    // Can I not just do this with parts?

    // pub fn take_name(&self) -> Option<(impl DoubleEndedIterator<Item = SingleId>, &SingleId)> {
    //     if self.is_bank() {
    //         return None
    //     }
    //     let last_delim_pos = self.0.rfind(SHARED_ACCOUNT_DELIM).unwrap();
    //     if last_delim_pos == 0 {
    //         Some((None.into_iter().flatten(), &self.0[last_delim_pos+1..]))
    //     }
    //     else {
    //         Some((Some(self.0[1..last_delim_pos].split(SHARED_ACCOUNT_DELIM)).into_iter().flatten(), &self.0[last_delim_pos+1..]))
    //     }
    // }
    pub fn parent<'b>(&'b self) -> Option<SharedId<'b>> {
        if self.is_bank() {
            return None;
        }
        let last_delim_pos = self.0.rfind(SHARED_ACCOUNT_DELIM).unwrap();
        if last_delim_pos == 0 {
            Some(SharedId::THE_BANK)
        }
        else {
            Some(SharedId(std::borrow::Cow::Borrowed(&self.0[..last_delim_pos])))
        }
    }
    pub fn push(&mut self, child: UnsharedId) {
        let raw_str = self.0.to_mut();
        raw_str.reserve(1 + child.len());
        raw_str.push(SHARED_ACCOUNT_DELIM);
        *raw_str += child.as_ref();
    }
    pub fn is_controlled_by(&self, other: &SharedId) -> bool {
        // Check to see if it's just the bank
        if other.0.len() == 1 {
            return true;
        }
        // Check that it is prefixed by other
        if !self.0.starts_with(other.0.as_ref()) {
            return false;
        }
        // Check that the string is either the same, or that the prefix is terminated by a slash
        match self.0.as_bytes().get(other.0.len()) {
            None => true,
            Some(val) if *val == SHARED_ACCOUNT_DELIM as u8 => true,
            _ => false
        }
    }
}
impl<'a, 'b> DivAssign<SharedId<'a>> for SharedId<'b> {
    // We are using pathing syntax, so this really should be a `+=`
    #[allow(clippy::suspicious_op_assign_impl)]
    fn div_assign(&mut self, rhs: SharedId<'a>) {
        *self.0.to_mut() += &rhs.0
    }
}
impl<'a, 'b> DivAssign<UnsharedId<'a>> for SharedId<'b> {
    // We are using pathing syntax, so this really should be a `+=`
    #[allow(clippy::suspicious_op_assign_impl)]
    fn div_assign(&mut self, rhs: UnsharedId<'a>) {
        self.push(rhs);
    }
}
impl<'a, 'b> Div<SharedId<'a>> for SharedId<'b> {
    type Output = SharedId<'static>;

    fn div(self, rhs: SharedId<'a>) -> Self::Output {
        let mut ret = self.into_owned();
        ret /= rhs;
        ret
    }
}
impl<'a, 'b> Div<UnsharedId<'a>> for SharedId<'b> {
    type Output = SharedId<'static>;

    fn div(self, rhs: UnsharedId<'a>) -> Self::Output {
        let mut ret = self.into_owned();
        ret /= rhs;
        ret
    }
}
impl<'a> TryFrom<Cow<'a, str>> for SharedId<'a> {
    type Error = IdParseError<'a>;

    fn try_from(value: Cow<'a, str>) -> Result<Self, Self::Error> {
        if !value.starts_with(SHARED_ACCOUNT_DELIM) {
            return Err(value.into());
        }
        let ret = Self(value);
        // It's the bank!
        if ret == Self::THE_BANK {
            return Ok(Self::THE_BANK);
        }
        if !ret.parts().all(is_safe_name) {
            return Err(ret.0.into());
        }
        Ok(ret)
    }
}
impl<'a> Deref for SharedId<'a> {
    type Target = Cow<'a, str>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
common_impl!(SharedId);


#[derive(Clone, Debug)]
pub enum AssetId<'a> {
    Item(ItemId<'a>),
    ETP(ETPId<'a>)
}
impl<'a> AssetId<'a> {
    pub const DIAMOND: AssetId<'a> = AssetId::Item(ItemId::DIAMOND);

    pub fn into_owned(self) -> AssetId<'static> {
        match self {
            AssetId::Item(item_id) => AssetId::Item(item_id.into_owned()),
            AssetId::ETP(etpid) => AssetId::ETP(etpid.into_owned()),
        }
    }

    pub fn shallow_clone(&'a self) -> Self {
        match self {
            AssetId::Item(item_id) => AssetId::Item(item_id.shallow_clone()),
            AssetId::ETP(etpid) => AssetId::ETP(etpid.shallow_clone()),
        }
    }
}
impl<'a> Deref for AssetId<'a> {
    type Target = Cow<'a, str>;

    fn deref(&self) -> &Self::Target {
        match self {
            AssetId::Item(item_id) => item_id.deref(),
            AssetId::ETP(etpid) => etpid.deref(),
        }
    }
}
impl<'a> TryFrom<Cow<'a, str>> for AssetId<'a> {
    type Error = IdParseError<'a>;
    fn try_from(value: Cow<'a, str>) -> Result<Self, Self::Error> {
        value.try_into().map(Self::ETP)
        .or_else(|IdParseError(i)| i.try_into().map(Self::Item))
    }
}
common_impl!(AssetId);
impl<'a> From<ItemId<'a>> for AssetId<'a> {
    fn from(value: ItemId<'a>) -> Self {
        Self::Item(value)
    }
}
impl<'a> From<ETPId<'a>> for AssetId<'a> {
    fn from(value: ETPId<'a>) -> Self {
        Self::ETP(value)
    }
}
impl<'a> From<&'a ItemId<'a>> for AssetId<'a> {
    fn from(value: &'a ItemId<'a>) -> Self {
        Self::Item(value.shallow_clone())
    }
}
impl<'a> From<&'a ETPId<'a>> for AssetId<'a> {
    fn from(value: &'a ETPId<'a>) -> Self {
        Self::ETP(value.shallow_clone())
    }
}
impl<'a> TryFrom<AssetId<'a>> for ItemId<'a> {
    type Error = AssetId<'a>;

    fn try_from(value: AssetId<'a>) -> Result<Self, Self::Error> {
        match value {
            AssetId::Item(x) => Ok(x),
            x => Err(x),
        }
    }
}
impl<'a> TryFrom<AssetId<'a>> for ETPId<'a> {
    type Error = AssetId<'a>;

    fn try_from(value: AssetId<'a>) -> Result<Self, Self::Error> {
        match value {
            AssetId::ETP(x) => Ok(x),
            x => Err(x),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ItemId<'a>(Cow<'a, str>);
impl<'a> ItemId<'a> {
    pub const DIAMOND: ItemId<'static> = ItemId(Cow::Borrowed("diamond"));
    /// Creates an ItemId without validating it
    ///
    /// # Safety
    ///
    /// The given string should pass is_safe_name
    pub unsafe fn unvalidated(x: Cow<'a, str>) -> Self { Self(x) }
    pub fn into_owned(self) -> ItemId<'static> { ItemId(Cow::Owned(self.0.into_owned())) }
    pub fn shallow_clone(&'a self) -> Self { Self(Cow::Borrowed(&self.0)) }
}
impl<'a> Deref for ItemId<'a> {
    type Target = Cow<'a, str>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<'a> TryFrom<Cow<'a, str>> for ItemId<'a> {
    type Error = IdParseError<'a>;

    fn try_from(value: Cow<'a, str>) -> Result<Self, Self::Error> {
        if is_safe_name(value.as_ref()) {
            Ok(Self(value))
        }
        else {
            Err(value.into())
        }
    }
}
common_impl!(ItemId);

#[derive(Clone, Debug)]
pub struct ETPId<'a> {
    base: Cow<'a, str>,
    split_offset: usize
}
impl<'a> ETPId<'a> {
    pub fn create<'b, 'c>(issuer: SharedId<'b>, name: ItemId<'c>) -> ETPId<'static> {
        let split_offset = issuer.len();
        let mut raw_str = issuer.0.into_owned();
        raw_str.reserve(split_offset + 1);
        raw_str.push(ETP_DELIM);
        raw_str.push_str(&name);
        // The issuer and name are validated before being passed in (or it's not our mess up :p)
        ETPId { base: Cow::Owned(raw_str), split_offset }
    }
    /// Creates an ETPId without validating it
    ///
    /// # Safety
    ///
    /// The split offset should be inside the length of base, and the two parts should be respectively a valid SharedId and ItemId
    pub unsafe fn unvalidated(base: Cow<'a, str>, split_offset: usize) -> Self { Self{base, split_offset} }
    pub fn into_owned(self) -> ETPId<'static> {
        ETPId {
            base: Cow::Owned(self.base.into_owned()),
            split_offset: self.split_offset
        }
    }
    pub fn shallow_clone(&'a self) -> Self {
        ETPId {
            base: Cow::Borrowed(&self.base),
            split_offset: self.split_offset
        }
    }

    pub fn issuer(&'_ self) -> SharedId<'_> {
        // SAFETY: We've already checked, so no need to worry about validation
        unsafe { SharedId::unvalidated(Cow::Borrowed(&self.base[..self.split_offset])) }
    }
    pub fn name(&'_ self) -> ItemId<'_> {
        // SAFETY: We've already checked, so no need to worry about validation
        unsafe { ItemId::unvalidated(Cow::Borrowed(&self.base[self.split_offset + 1 ..])) }
    }
}
impl<'a> Deref for ETPId<'a> {
    type Target = Cow<'a, str>;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}
impl<'a> TryFrom<Cow<'a, str>> for ETPId<'a> {
    type Error = IdParseError<'a>;

    fn try_from(base: Cow<'a, str>) -> Result<Self, Self::Error> {
        if !base.starts_with(SHARED_ACCOUNT_DELIM) {
            return Err(base.into());
        }
        let Some(split_offset) = base.find(ETP_DELIM) else { return Err(base.into()) };
        // Validate the components
        if SharedId::try_from(Cow::Borrowed(&base[..split_offset])).is_err() || ItemId::try_from(Cow::Borrowed(&base[split_offset + 1 ..])).is_err() {
            return Err(base.into());
        }
        Ok(ETPId { base, split_offset })
    }
}
common_impl!(ETPId);

#[cfg(test)]
mod tests {
    use crate::ids::*;

    #[test]
    fn fuzz_item_id() {
        assert!(ItemId::try_from("").is_err(), "Empty ItemId got through");
        assert!(ItemId::try_from(format!("{ETP_DELIM}foobar")).is_err(), "ItemId with ETP delim got through");
        assert!(ItemId::try_from(format!("foo{ETP_DELIM}var")).is_err(), "ItemId with ETP delim in middle got through");
        assert!(ItemId::try_from("test-item_good").is_ok(), "Valid ItemId got blocked");
        assert!(ItemId::try_from(ItemId::DIAMOND.as_ref()).is_ok(), "Valid ItemId got blocked");
    }

    #[test]
    fn fuzz_shared_id() {
        assert!(SharedId::THE_BANK.parts().last().is_none());
        assert_eq!(SharedId::THE_BANK.parts().collect::<Vec<UnsharedId>>(), Vec::<UnsharedId>::new(), "Bank had parts");

        SharedId::try_from("foo").expect_err("Invalid SharedId got through");
        SharedId::try_from("foo.").expect_err("Invalid SharedId got through");
        SharedId::try_from(".foo.").expect_err("Invalid SharedId got through");

        let single = SharedId::try_from(".foo").expect("Could not parse valid SharedId");
        let mut parent = single.parts();
        let name = parent.next_back().expect("Single name didn't have a name");
        assert_eq!(parent.collect::<Vec<UnsharedId>>(), Vec::<UnsharedId>::new(), "Somehow had parent in single name");
        assert_eq!(name.as_ref(), "foo");
        assert_eq!(single.parent(), Some(SharedId::THE_BANK));

        let multi = SharedId::try_from(".foo.bar").expect("Could not parse valid SharedId");
        let mut parent = multi.parts();
        let name = parent.next_back().expect("Single name didn't have a name");
        assert_eq!(parent.collect::<Vec<_>>(), vec![UnsharedId::try_from("foo").unwrap()]);
        assert_eq!(name.as_ref(), "bar");
    }


    #[test]
    fn fuzz_etp() {
        let shared_name: SharedId = ".foo".try_into().expect("Could not parse name");
        assert_eq!(ETPId::create(shared_name.shallow_clone(), "foobar".try_into().unwrap()).deref(), ".foo:foobar");
        assert_eq!(ETPId::try_from(".foo:foobar").unwrap(), ETPId::create(shared_name.shallow_clone(), "foobar".try_into().unwrap()));
        assert_eq!(ETPId::try_from(".foo:foobar").unwrap().deref(), ".foo:foobar");
        assert_eq!(ETPId::create(SharedId::THE_BANK, "foobar".try_into().unwrap()).deref(), ".:foobar");
        assert_eq!(ETPId::try_from(".:foobar").unwrap(), ETPId::create(SharedId::THE_BANK, "foobar".try_into().unwrap()));
        assert_eq!(ETPId::try_from(".:foobar").unwrap().deref(), ".:foobar");
    }
}
