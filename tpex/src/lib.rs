use std::{collections::HashSet, ops::{Add, AddAssign}, pin::pin};

use const_format::concatcp;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

// We use a base coins, which represent 1/1000 of a diamond
use serde::{Deserialize, Serialize};

use auth::AuthSync;
use balance::BalanceSync;
use order::OrderSync;
use withdrawal::WithdrawalSync;
use crate::shared_account::SharedSync;

use self::{order::PendingOrder, withdrawal::PendingWithdrawal};

pub mod balance;
pub mod order;
pub mod withdrawal;
pub mod coins;
pub mod auth;
pub mod shared_account;
pub mod etp;
mod tests;

pub use coins::Coins;
pub use shared_account::SharedId;
pub use etp::ETPId;

pub use shared_account::SHARED_ACCOUNT_DELIM;
pub use etp::ETP_DELIM;

/// Checks whether `x` is a safe name (i.e. free from annoying bs that could hack us)
///
/// This will reject a lot of valid IDs, so it should only be used for something that cannot be decomposed further
/// (i.e. for parts of an SharedId, not for a PlayerId)
pub fn is_safe_name(x: &str) -> bool {
    x.chars().all(|i| i.is_ascii_alphanumeric() || i == '_' || i == '-')
}

pub const DIAMOND_NAME: &str = "diamond";
pub const DIAMOND_RAW_COINS: Coins = Coins::from_coins(1000);

const INITIAL_BANK_RATES: BankRates = BankRates {
    buy_order_ppm:      0_0000,
    sell_order_ppm:     0_0000,
    coins_sell_ppm:    5_0000,
    coins_buy_ppm:   5_0000,
};

#[derive(PartialEq, PartialOrd, Eq, Ord, Default, Debug, Clone, Hash)]
pub struct PlayerId(String);
impl PlayerId {
    /// Creates a player id, assuming that the given id is valid, correct, and authorized.
    pub fn assume_username_correct(s: String) -> PlayerId { PlayerId(s) }
    /// Gets the internal name of the user
    pub fn get_raw_name(&self) -> &String { &self.0 }
    pub fn the_bank() -> PlayerId { PlayerId(SHARED_ACCOUNT_DELIM.to_string()) }
    pub fn is_bank(&self) -> bool { self.0 == concatcp!(SHARED_ACCOUNT_DELIM) }
    pub fn is_unshared(&self) -> bool { !self.0.starts_with(SHARED_ACCOUNT_DELIM) }
}
impl<'de> Deserialize<'de> for PlayerId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de> {
        String::deserialize(deserializer).map(PlayerId)
    }
}
impl Serialize for PlayerId {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer {
        String::serialize(&self.0, serializer)
    }
}
impl core::fmt::Display for PlayerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

pub type AssetId = String;

#[derive(PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum ActionLevel {
    Normal,
    Banker
}
#[derive(PartialEq, Eq, Debug)]
pub struct ActionPermissions {
    pub level: ActionLevel,
    pub player: PlayerId
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum Action {
    /// Deleted transaction, for when someone does a bad
    Deleted {
        /// The reason it was deleted
        reason: String,
        /// Who deleted it
        banker: PlayerId
    },
    /// Player deposited assets
    Deposit {
        /// The player who should be credited with these assets
        player: PlayerId,
        /// The asset to deposit
        asset: AssetId,
        /// The number of that asset to deposit
        count: u64,
        /// The banker who performed the deposit
        banker: PlayerId,
    },
    /// Used to correct typos
    Undeposit {
        /// The player who should have the items taken away
        player: PlayerId,
        /// The asset to remove
        asset: AssetId,
        /// The amount of those items to be removed
        count: u64,
        /// The banker who submitted this action
        banker: PlayerId
    },
    /// Player asked to withdraw assets
    RequestWithdrawal {
        /// The player who requested the withdrawal
        player: PlayerId,
        /// The assets to withdraw
        assets: std::collections::HashMap<AssetId,u64>
    },
    /// A banker has agreed to take out assets imminently
    CompleteWithdrawal {
        /// The ID of the corresponding RequestWithdrawal transaction
        target: u64,
        /// The banker who confirmed it
        banker: PlayerId,
    },
    /// A banker has confirmed that these assets have not, and will not, be withdrawn
    CancelWithdrawal {
        /// The ID of the corresponding RequestWithdrawal transaction
        target: u64,
        /// The banker who confirmed it
        banker: PlayerId,
    },
    /// The player got coins for giving diamonds
    BuyCoins {
        /// The player who should be credited with the coins
        player: PlayerId,
        /// The number of diamonds converted
        n_diamonds: u64,
    },
    /// The player got diamonds for giving coins
    SellCoins {
        /// The player who should be credited with the diamonds
        player: PlayerId,
        /// The number of diamonds converted
        n_diamonds: u64,
    },
    /// Player offers to buy assets at a price, and locks money away until cancelled
    ///
    /// Instant matches should favour the buyer
    BuyOrder {
        /// The player who placed the order
        player: PlayerId,
        /// The asset they wish to order
        asset: AssetId,
        /// The number of that asset they wish to order
        count: u64,
        /// The number of coins each individual asset will cost
        coins_per: Coins,
    },
    /// Player offers to sell assets at a price, and locks away assets until cancelled
    ///
    /// Instant matches should favour the seller
    SellOrder {
        /// The player who placed the order
        player: PlayerId,
        /// The asset they wish to order
        asset: AssetId,
        /// The number of that asset they wish to order
        count: u64,
        /// The number of coins each individual asset will cost
        coins_per: Coins,
    },
    /// Updates the list of assets that require prior authorisation from an admin
    UpdateRestricted {
        /// The new list of assets that are restricted
        restricted_assets: HashSet<AssetId>,
    },
    /// Allows a player to place new withdrawal requests up to new_count of an item
    ///
    /// XXX: This can and will nuke existing values, so check those race conditions!
    AuthoriseRestricted {
        /// The player whose authorisation is being adjusted
        authorisee: PlayerId,
        /// The asset that should be authorised
        asset: AssetId,
        /// The new maximum amount this player can withdraw
        new_count: u64
    },
    /// Changes the fees
    UpdateBankRates {
        #[serde(flatten)]
        rates: BankRates,
    },
    /// A transfer of coins from one player to another, no strings attached
    TransferCoins {
        /// The player sending the coins
        payer: PlayerId,
        /// The player receiving the coins
        payee: PlayerId,
        /// The number of coins
        count: Coins
    },
    /// A transfer of items from one player to another, no strings attached
    TransferAsset {
        /// The player sending the asset
        payer: PlayerId,
        /// The player receiving the asset
        payee: PlayerId,
        /// The name of the asset
        asset: AssetId,
        /// The amount of the asset
        count: u64
    },
    /// Cancel the remaining assets and coins in a buy or sell order
    CancelOrder {
        /// The transaction ID of the order to cancel
        target: u64
    },
    /// Creates or updates a shared account
    CreateOrUpdateShared {
        /// The name of the shared account, which will be added onto the parent with a slash
        ///
        /// i.e. If /foo creates an account bar, then it will be called /foo/bar
        ///
        /// Note that the bank name "/" is implicit here
        name: SharedId,
        /// The players who control this account
        owners: Vec<PlayerId>,
        /// The minimum value of (agree - disagree) before a vote passes
        min_difference: u64,
        /// The minimum number of owners who need to vote in order for a proposal to be considered
        min_votes: u64,
    },
    /// Proposes an action for a shared account
    Propose {
        /// The actual action to perform
        action: Box<Action>,
        /// The player proposing the action
        proposer: PlayerId,
        /// The shared account that this proposal applies to
        target: SharedId
    },
    /// Agree to a proposal
    Agree {
        /// The player who agrees
        player: PlayerId,
        /// The ID of the proposal in question
        proposal_id: u64,
    },
    /// Disagree to a proposal
    Disagree {
        /// The player who disagrees
        player: PlayerId,
        /// The ID of the proposal in question
        proposal_id: u64,
    },
    /// Shut down a shared account, cancel all orders, and credit the remaining assets and coins to the parent
    WindUp {
        /// The shared account to wind up
        account: SharedId,
    },
    /// Update the list of shared accounts that are allowed to issue exchange traded products
    ///
    /// The compliance process should be very tight to ensure low default risk
    UpdateETPAuthorised {
        /// The new list of shared accounts
        accounts: HashSet<SharedId>
    },
    /// Issue an exchange traded product to the issuing account
    Issue {
        /// The product to be issued
        product: ETPId,
        /// The amount of that product, capped to a u32 to make it harder (and more obvious) if someone is doing a funny
        count: u32
    },
    /// Removes some amount of a product from the *issuing* account
    ///
    /// This allows redemption by sending the asset to the issuer, who then removes
    Remove {
        /// The product to be removed
        product: ETPId,
        /// The amount of that product
        count: u64
    }
}
impl Action {
    fn adjust_audit(&self, mut audit: Audit) -> Option<Audit> {
        match self {
            Action::Deposit { asset, count, .. } => {
                audit.add_asset(asset.clone(), *count);
                Some(audit)
            },
            Action::Undeposit { asset, count, .. } => {
                audit.sub_asset(asset.clone(), *count);
                Some(audit)
            }
            Action::CompleteWithdrawal{..} => {
                // We don't know what the withdrawal is just from the id
                //
                // TODO: find a way to track this nicely
                None
            },
            Action::BuyCoins { n_diamonds, .. } => {
                audit.sub_asset(DIAMOND_NAME.into(), *n_diamonds);
                audit.add_coins(DIAMOND_RAW_COINS.checked_mul(*n_diamonds).unwrap());
                Some(audit)
            },
            Action::SellCoins { n_diamonds, .. } => {
                audit.add_asset(DIAMOND_NAME.into(), *n_diamonds);
                audit.sub_coins(DIAMOND_RAW_COINS.checked_mul(*n_diamonds).unwrap());
                Some(audit)
            },
            Action::Issue { product, count } => {
                audit.add_asset(product.into(), *count as u64);
                Some(audit)
            },
            Action::Remove { product, count } => {
                audit.sub_asset(product.into(), *count);
                Some(audit)
            },
            Action::Propose { action, .. } => {
                match action.adjust_audit(audit.clone()) {
                    // If the proposal isn't going to change the total amount of stuff even if it goes through,
                    // then we can confidently assert that the total amount of stuff shouldn't change
                    Some(result) if result == audit => Some(audit),
                    // Otherwise, we don't know what effect this will have
                    _ => None
                }
            }
            _ => Some(audit)
        }
    }
}

#[derive(Default, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Audit {
    pub coins: Coins,
    pub assets: std::collections::HashMap<AssetId, u64>
}
impl Audit {
    pub fn add_asset(&mut self, asset: AssetId, count: u64) {
        if count > 0 {
            let entry = self.assets.entry(asset).or_default();
            *entry = entry.checked_add(count).expect("Failed to add asset to audit")
        }
    }
    pub fn sub_asset(&mut self, asset: AssetId, count: u64) {
        if count == 0 {
            return;
        }
        let std::collections::hash_map::Entry::Occupied(mut entry) = self.assets.entry(asset)
        else { panic!("Tried to remove empty asset from audit") };
        match entry.get().checked_sub(count) {
            Some(0) => {entry.remove();  },
            None => panic!("Failed to remove asset from audit"),
            Some(res) => { *entry.get_mut() = res; }
        }
    }
    pub fn add_coins(&mut self, count: Coins) {
        self.coins.checked_add_assign(count).expect("Failed to add coins to audit")
    }
    pub fn sub_coins(&mut self, count: Coins) {
        self.coins.checked_sub_assign(count).expect("Failed to remove coins from audit")
    }
}
impl Add for Audit {
    type Output = Audit;

    fn add(self, rhs: Self) -> Self::Output {
        let mut ret = self;
        ret += rhs;
        ret
    }
}
impl AddAssign for Audit {
    fn add_assign(&mut self, rhs: Self) {
        self.coins.checked_add_assign(rhs.coins).expect("Audit merge failed: coin overflow");
        rhs.assets.into_iter().for_each(|(asset, count)| self.add_asset(asset, count));
    }
}
impl AddAssign<&Audit> for Audit {
    fn add_assign(&mut self, rhs: &Audit) {
        self.coins.checked_add_assign(rhs.coins).expect("Audit merge failed: coin overflow");
        rhs.assets.iter().for_each(|(asset, count)| self.add_asset(asset.clone(), *count));
    }
}

pub trait Auditable {
    // Check internal counters, will be called after every action
    fn soft_audit(&self) -> Audit;
    // Verify internal counters, will be called rarely. Should panic if inconsistencies found
    fn hard_audit(&self) -> Audit;
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct WrappedAction {
    // The id of the action, which should equal the line number of the trades list
    pub id: u64,
    // The time this action was performed
    pub time: chrono::DateTime<chrono::Utc>,
    // The action itself
    pub action: Action,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    OverdrawnAsset {
        asset: AssetId,
        amount_overdrawn: u64
    },
    OverdrawnCoins {
        amount_overdrawn: Coins
    },
    UnauthorisedWithdrawal{
        asset: AssetId,
        // Set to be None if the player has no authorization at all to withdraw this
        amount_overdrawn: Option<u64>
    },
    /// Some 1337 hacker tried an overflow attack >:(
    Overflow,
    InvalidId{id: u64},
    AlreadyDone,
    NotABanker{player: PlayerId},
    CoinStringMangled,
    CoinStringTooPrecise,
    InvalidRates,
    InvalidFastSync,
    InvalidThreshold,
    InvalidSharedId,
    InvalidETPId,
    UnauthorisedShared,
    UnsharedOnly,
    UnauthorisedIssue{account: SharedId},
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::OverdrawnAsset { asset, amount_overdrawn } => {
                write!(f, "Player needs {amount_overdrawn} more {asset} to perform this action.")
            },
            Error::OverdrawnCoins { amount_overdrawn } => {
                write!(f, "Player needs {amount_overdrawn} more to perform this action.")
            },
            Error::UnauthorisedWithdrawal { asset, amount_overdrawn } => {
                match amount_overdrawn {
                    Some(amount_overdrawn) => write!(f, "Player needs authorisation to withdraw {amount_overdrawn} more {asset}."),
                    None => write!(f, "Player is not authorised to withdraw {asset}.")
                }

            },
            Error::Overflow => {
                write!(f, "The request was so messed up it could have overflowed!")
            },
            Error::InvalidId { id } => {
                write!(f, "The action ID {id} was invalid.")
            },
            Error::AlreadyDone => {
                write!(f, "The requested action is redundant.")
            },
            Error::NotABanker { player } => {
                write!(f, "The requested action would require {player} to be a banker, but they are not.")
            },
            Error::CoinStringMangled => {
                write!(f, "The system could not understand the given coin amount.")
            },
            Error::CoinStringTooPrecise => {
                write!(f, "Too much precision was given for the coins: the system can only handle 3 decimal places.")
            },
            Error::InvalidRates => {
                write!(f, "The provided rates were invalid.")
            },
            Error::InvalidFastSync => {
                write!(f, "The provided FastSync struct was corrupted.")
            },
            Error::InvalidThreshold => {
                write!(f, "The provided threshold was either unsatisfiable or zero.")
            },
            Error::InvalidSharedId => {
                write!(f, "The requested action involves a non-existent or unparsable shared account.")
            },
            Error::InvalidETPId => {
                write!(f, "The provided ETP ID was invalid")
            },
            Error::UnauthorisedShared => {
                write!(f, "The requested action requires the player to have access to a shared account that they do not.")
            },
            Error::UnsharedOnly => {
                write!(f, "The requested action can only be performed on an unshared account.")
            },
            Error::UnauthorisedIssue { account } => {
                write!(f, "The account {account} is not authorised to issue ETPs.")
            }
        }

    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct BankRates {
    /// The parts per million fee for each partial completion of a buy order
    buy_order_ppm: u64,
    /// The parts per million fee for each partial completion of a sell order
    sell_order_ppm: u64,
    /// The parts per million fee for converting coins into diamonds
    coins_sell_ppm: u64,
    /// The parts per million fee for converting diamonds into coins
    coins_buy_ppm: u64
}
impl BankRates {
    pub fn check(&self) -> Result<()> {
        if
            // We don't need to limit this, as they just pay a lot, rather than losing money
            // self.buy_order_ppm > 1_000_000 ||
            self.sell_order_ppm > 1_000_000 ||
            self.coins_buy_ppm > 1_000_000
            // We don't need to limit this, as they just pay a lot, rather than losing money
            // self.diamond_buy_ppm > 1_000_000
        {
            Err(Error::InvalidRates)
        }
        else {
            Ok(())
        }
    }
    pub const fn free() -> BankRates {
        BankRates { buy_order_ppm: 0, sell_order_ppm: 0, coins_sell_ppm: 0, coins_buy_ppm: 0 }
    }
}

#[derive(Debug, Clone)]
pub struct State {
    next_id: u64,
    rates: BankRates,

    auth: auth::AuthTracker,
    balance: balance::BalanceTracker,
    order: order::OrderTracker,
    withdrawal: withdrawal::WithdrawalTracker,
    shared_account: shared_account::SharedTracker
}
impl Default for State {
    fn default() -> State {
        State {
            // Start on ID 1 for nice mapping to line numbers
            next_id: 1,
            rates: INITIAL_BANK_RATES,
            auth: Default::default(),
            balance: Default::default(),
            order: Default::default(),
            withdrawal: Default::default(),
            shared_account: shared_account::SharedTracker::init()
        }
    }
}
impl State {
    /// Create a new empty state
    pub fn new() -> State { Self::default() }
    /// Get the next line
    pub fn get_next_id(&self) -> u64 { self.next_id }
    /// Get a player's balance
    pub fn get_bal(&self, player: &PlayerId) -> Coins { self.balance.get_bal(player) }
    /// Get all balances
    pub fn get_bals(&self) -> std::collections::HashMap<PlayerId, Coins> { self.balance.get_bals() }
    /// Get a player's assets
    pub fn get_assets(&self, player: &PlayerId) -> std::collections::HashMap<AssetId, u64> { self.balance.get_assets(player) }
    /// Get all players' assets
    pub fn get_all_assets(&self) -> &std::collections::HashMap<PlayerId, std::collections::HashMap<AssetId, u64>> { self.balance.get_all_assets() }
    /// List all withdrawals
    pub fn get_withdrawals(&self) -> std::collections::BTreeMap<u64, PendingWithdrawal> { self.withdrawal.get_withdrawals() }
    /// List all withdrawals
    pub fn get_withdrawal(&self, id: u64) -> Result<PendingWithdrawal> { self.withdrawal.get_withdrawal(id) }
    /// Get the withdrawal the bankers should examine next
    pub fn get_next_withdrawal(&self) -> Option<PendingWithdrawal> { self.withdrawal.get_next_withdrawal() }
    /// List all orders
    pub fn get_orders(&self) -> std::collections::BTreeMap<u64, PendingOrder> { self.order.get_all() }
    /// List all orders
    pub fn get_orders_filter<'a>(&'a self, filter: impl Fn(&PendingOrder) -> bool + 'a) -> impl Iterator<Item=PendingOrder> + 'a { self.order.get_orders_filter(filter) }
    /// Get a specific order
    pub fn get_order(&self, id: u64) -> Result<PendingOrder> { self.order.get_order(id) }
    /// Prices for an asset, returns (price, amount) in (buy, sell)
    pub fn get_prices(&self, asset: &AssetId) -> (std::collections::BTreeMap<Coins, u64>, std::collections::BTreeMap<Coins, u64>) { self.order.get_prices(asset) }
    /// Returns true if the given item is currently restricted
    pub fn is_restricted(&self, asset: &AssetId) -> bool { self.auth.is_restricted(asset) }
    /// Lists all restricted items
    pub fn get_restricted(&self) -> impl IntoIterator<Item = &AssetId> { self.auth.get_restricted() }
    /// Gets a list of all bankers
    pub fn get_bankers(&self) -> &HashSet<PlayerId> { self.shared_account.the_bank().owners() }
    /// Returns true if the given player is an banker
    pub fn is_banker(&self, player: &PlayerId) -> bool { player.is_bank() || self.shared_account.the_bank().owners().contains(player) }
    /// Get the required permissions for a given action
    pub fn perms(&self, action: &Action) -> Result<ActionPermissions> {
        match action {
            Action::AuthoriseRestricted { .. } |
            Action::UpdateBankRates { .. } |
            Action::UpdateRestricted { .. } |
            Action::UpdateETPAuthorised { .. }
                => Ok(ActionPermissions { level: ActionLevel::Banker, player: PlayerId::the_bank() }),

            Action::Deleted { banker, .. } |
            Action::Deposit { banker, .. } |
            Action::CompleteWithdrawal { banker, .. } |
            Action::CancelWithdrawal { banker, .. } |
            Action::Undeposit { banker, .. }
                => Ok(ActionPermissions{level: ActionLevel::Banker, player: banker.clone()}),

            Action::BuyCoins { player, .. } |
            Action::BuyOrder { player, .. } |
            Action::SellCoins { player, .. } |
            Action::SellOrder { player, .. } |
            Action::TransferAsset { payer: player, .. } |
            Action::TransferCoins { payer: player, .. } |
            Action::RequestWithdrawal { player, .. } |
            Action::Agree { player, .. } |
            Action::Disagree { player, .. }
                => Ok(ActionPermissions{level: ActionLevel::Normal, player: player.clone()}),

            Action::CancelOrder { target } =>
                Ok(ActionPermissions{level: ActionLevel::Normal, player: self.order.get_order(*target)?.player.clone()}),

            Action::Propose { proposer, action, .. } => {
                let perms = self.perms(action)?;
                if perms.level != ActionLevel::Normal {
                    return Err(Error::NotABanker { player: perms.player });
                }
                Ok(ActionPermissions{player: proposer.clone(), level: perms.level})
            },

            Action::WindUp { account, .. } =>
                // Only the parent can wind up a company, to prevent easy default
                Ok(ActionPermissions{level: ActionLevel::Normal, player: account.parent().ok_or(Error::UnauthorisedShared)?.into()}),
            Action::CreateOrUpdateShared { name, .. } =>
                Ok(ActionPermissions{level: ActionLevel::Normal, player: name.clone().into()}),

            // Managing products directly can only be done by the issuer
            Action::Issue { product, .. } |
            Action::Remove { product, .. } =>
                Ok(ActionPermissions { level: ActionLevel::Normal, player: product.issuer().clone().into() })
        }
    }
    /// Nice macro for checking whether a player is a banker
    fn check_banker(&self, player: &PlayerId) -> Result<()> {
        if self.is_banker(player) {
            Ok(())
        }
        else {
            Err(Error::NotABanker { player: player.clone() })
        }
    }
    // Atomic (but not parallelisable!).
    // This means the function will change significant things (i.e. more than just creating empty lists) IF AND ONLY IF it fully succeeds.
    // As such, we don't have to worry about giving it bad actions
    fn apply_inner(&mut self, id: u64, action: Action) -> Result<()> {
        // Blanket check perms
        //
        // TODO: optimise
        if let ActionPermissions { level: ActionLevel::Banker, player } = self.perms(&action)?
            && !self.is_banker(&player) {
                return Err(Error::NotABanker { player });
            }

        match action {
            Action::Deleted{..} => Ok(()),
            Action::Deposit { player, asset, count, banker } => {
                self.check_banker(&banker)?;
                self.balance.commit_asset_add(&player, &asset, count);
                self.auth.increase_authorisation(player, asset, count).expect("Authorisation overflow");

                Ok(())
            },
            Action::Undeposit { player, asset, count, banker } => {
                self.check_banker(&banker)?;
                self.balance.commit_asset_removal(&player, &asset, count)
            },
            Action::RequestWithdrawal { player, assets} => {
                // Shared accounts cannot directly withdraw
                if !player.is_unshared() {
                    return Err(Error::UnsharedOnly)
                }
                // There's no good way of doing this without two passes, so we check then commit
                //
                // BTreeMap ensures that the same asset cannot occur twice, so we don't have to worry about double spending
                for (asset, count) in assets.iter() {
                    // Check to see if they can afford it
                    self.balance.check_asset_removal(&player, asset, *count)?;
                    // Check to see if they are allowed it
                    self.auth.check_withdrawal_authorized(&player, asset, *count)?;
                    // Check to make sure they're not trying to withdraw ETPs, because that makes no sense
                    if ETPId::is_etp(asset) {
                        return Err(Error::UnauthorisedWithdrawal { asset: asset.clone(), amount_overdrawn: None })
                    }
                }

                // Now take the assets, as we've confirmed they can afford it
                for (asset, count) in assets.iter() {
                    // Remove assets
                    self.balance.commit_asset_removal(&player, asset, *count).expect("Assets disappeared after check");
                    // Remove allowance if restricted
                    self.auth.commit_withdrawal_authorized(&player, asset, *count).expect("Auth disappeared after check");
                }

                // Register the withdrawal. This cannot fail, so we don't have to worry about atomicity
                self.withdrawal.track(id, player, assets);
                Ok(())
            },
            Action::CancelWithdrawal { target, banker } => {
                self.check_banker(&banker)?;
                // Stop tracking the withdrawal
                let withdrawal = self.withdrawal.finalise(target)?;
                // Credit the account and reauthorise the assets
                for (asset, count) in withdrawal.assets {
                    self.balance.commit_asset_add(&withdrawal.player, &asset, count);
                    self.auth.increase_authorisation(withdrawal.player.clone(), asset.clone(), count).expect("Authorisation overflow in cancelled order");
                }
                Ok(())
            }
            Action::SellOrder { player, asset, count, coins_per } => {
                if count == 0 {
                    return Err(Error::AlreadyDone)
                }
                // Check and take their assets first
                self.balance.commit_asset_removal(&player, &asset, count)?;
                // Do the matching and listing
                let res = self.order.handle_sell(id, &player, &asset, count, coins_per, self.rates.sell_order_ppm);
                // Transfer the assets
                for (buyer, count) in res.assets_instant_matched {
                    self.balance.commit_asset_add(&buyer, &asset, count);
                }
                // Transfer the money
                self.balance.commit_coin_add(&player, res.coins_instant_earned);
                // Pay the bank
                self.balance.commit_coin_add(&PlayerId::the_bank(), res.instant_bank_fee);

                Ok(())
            },
            Action::BuyOrder { player, asset, count, coins_per } => {
                if count == 0 || coins_per.is_zero() {
                    return Err(Error::AlreadyDone)
                }
                // Check their money first
                let mut max_cost = coins_per.checked_mul(count)?;
                max_cost.checked_add_assign(max_cost.fee_ppm(self.rates.buy_order_ppm)?)?;
                self.balance.check_coin_removal(&player, max_cost)?;

                // Do the matching and listing
                let res = self.order.handle_buy(id, &player, &asset, count, coins_per, self.rates.buy_order_ppm);
                // Transfer the money
                self.balance.commit_coin_removal(&player, res.cost).expect("Somehow used more money in buy order than expected");
                // Pay the sellers
                for (seller, coins) in res.sellers {
                    self.balance.commit_coin_add(&seller, coins)
                }
                // Transfer the assets
                if res.assets_instant_matched > 0 {
                    self.balance.commit_asset_add(&player, &asset, res.assets_instant_matched);
                }
                // Pay the bank
                self.balance.commit_coin_add(&PlayerId::the_bank(), res.instant_bank_fee);

                Ok(())
            },
            Action::CompleteWithdrawal { target, banker } => {
                self.check_banker(&banker)?;
                // Try to take out the pending transaction
                self.withdrawal.finalise(target)?;
                Ok(())
            },
            Action::CancelOrder { target } => {
                match self.order.cancel(target)? {
                    order::CancelResult::BuyOrder { player, refund_coins } => {
                        self.balance.commit_coin_add(&player, refund_coins);
                    },
                    order::CancelResult::SellOrder { player, refunded_asset, refund_count } => {
                        self.balance.commit_asset_add(&player, &refunded_asset, refund_count);
                    }
                }
                Ok(())
            },
            Action::BuyCoins { player, n_diamonds } => {
                // Check and take diamonds from payer...
                self.balance.commit_asset_removal(&player,&DIAMOND_NAME.to_owned(), n_diamonds)?;
                // ... and give them the coins
                let n_coins = DIAMOND_RAW_COINS.checked_mul(n_diamonds).expect("BuyCoins overflow");
                let fee = n_coins.fee_ppm(self.rates.coins_buy_ppm).expect("BuyCoins fee overflow"); // This panic stops inconsistencies
                self.balance.commit_coin_add(&PlayerId::the_bank(), fee);
                self.balance.commit_coin_add(&player, n_coins.checked_sub(fee).unwrap()); // This panic stops inconsistencies
                Ok(())
            },
            Action::SellCoins { player, n_diamonds } => {
                // Check and take coins from payer...
                let n_coins = DIAMOND_RAW_COINS.checked_mul(n_diamonds)?;
                let fee = n_coins.fee_ppm(self.rates.coins_sell_ppm)?; // This panic stops inconsistencies
                self.balance.commit_coin_removal(&player, n_coins.checked_add(fee)?)?;
                self.balance.commit_coin_add(&PlayerId::the_bank(), fee);
                // ... and give them the diamonds
                self.balance.commit_asset_add(&player, &DIAMOND_NAME.to_owned(), n_diamonds);
                Ok(())
            },
            Action::UpdateRestricted { restricted_assets} => {
                // A list of the assets that just became restricted
                let newly_restricted = restricted_assets.difference(self.auth.get_restricted()).cloned().collect::<Vec<_>>();
                self.auth.update_restricted(restricted_assets);
                // Authorise everyone who is already holding the item to withdraw what they have
                for asset in newly_restricted {
                    for (player, their_assets) in self.balance.get_all_assets() {
                        let Some(count) = their_assets.get(&asset).copied() else { continue; };
                        self.auth.set_authorisation(player.clone(), asset.clone(), count);
                    }
                }
                Ok(())
            },
            Action::AuthoriseRestricted { authorisee, asset, new_count } => {
                // We don't need to check that it is authorisable, so that we can do pre-authorisation)
                self.auth.set_authorisation(authorisee, asset, new_count);
                Ok(())
            },
            Action::UpdateBankRates { rates } => {
                // Check they're consistent
                rates.check()?;
                // Set the rates
                self.rates = rates;
                Ok(())
            },
            Action::TransferCoins { payer, payee, count } => {
                // Check and take money from payer...
                self.balance.commit_coin_removal(&payer, count)?;
                // ... and give it to payee
                self.balance.commit_coin_add(&payee, count);
                Ok(())
            },
            Action::TransferAsset { payer, payee, asset, count } => {
                // Check and take assets from payer...
                self.balance.commit_asset_removal(&payer, &asset, count)?;
                // ... and give it to payee
                self.balance.commit_asset_add(&payee,  &asset, count);
                Ok(())
            },
            Action::CreateOrUpdateShared { name, owners, min_difference, min_votes  } => {
                self.shared_account.create_or_update(name, owners.into_iter().collect(), min_difference, min_votes)
            },
            Action::Propose { action, proposer, target } => {
                let expected_target: SharedId = self.perms(action.as_ref())?.player.try_into().map_err(|_| Error::InvalidSharedId)?;
                // Make sure that the target is owned by the player
                if !self.shared_account.is_owner(&target, &proposer)? {
                    return Err(Error::UnauthorisedShared)
                }
                // If this action applies to a different target, then the only way this is OK is if the expected_target is controlled by target
                if expected_target != target {
                    // We have to proceed even if this account doesn't exist, because of the CreateOrUpdate command
                    //
                    // if !self.shared_account.contains(&expected_target) {
                    //     return Err(Error::InvalidSharedId)
                    // }

                    // If the expected target is not controlled by the target, this is unauthorised
                    if !expected_target.is_controlled_by(&target) {
                        return Err(Error::UnauthorisedShared);
                    }
                    // Otherwise, this is definitely authorised, and we can continue
                }
                self.shared_account.add_proposal(id, target, *action)?;
                // The player agrees to their own proposal.
                if let Some(action) = self.shared_account.vote(id, proposer, true)? {
                    // We then process it if it immediately passes
                    self.apply_inner(id, action)?
                }
                Ok(())

            },
            Action::Disagree { player, proposal_id } => {
                if let Some(action) = self.shared_account.vote(proposal_id, player, false)? {
                    self.apply_inner(id, action)?
                }
                Ok(())
            },
            Action::Agree { player, proposal_id } => {
                if let Some(action) = self.shared_account.vote(proposal_id, player, true)? {
                    self.apply_inner(id, action)?
                }
                Ok(())
            },
            Action::WindUp { account } => {
                let parent = account.parent().ok_or(Error::InvalidSharedId)?;
                self.shared_account.wind_up(account, |account| {
                    // Move all the assets to the parent
                    let assets = self.balance.get_assets(account.as_ref());
                    for (asset, count) in assets {
                        self.balance.commit_asset_removal(account.as_ref(), &asset, count).expect("Failed to remove assets in windup");
                        self.balance.commit_asset_add(parent.as_ref(), &asset, count);
                    }
                    // Move all the coins to the parent
                    let coins = self.balance.get_bal(account.as_ref());
                    self.balance.commit_coin_removal(account.as_ref(), coins).expect("Failed to remove coins in windup");
                    self.balance.commit_coin_add(parent.as_ref(), coins);
                })
            },
            Action::UpdateETPAuthorised { accounts } => {
                self.auth.update_etp_authorised(accounts);
                Ok(())
            },
            Action::Issue { product, count } => {
                if !self.auth.is_etp_authorised(product.issuer()) {
                    return Err(Error::UnauthorisedIssue{account: product.issuer().clone()})
                }
                self.balance.commit_asset_add(product.issuer().as_ref(), &(&product).into(), count as u64);
                Ok(())
            },
            Action::Remove { product, count } => {
                // We don't check to see if they are currently allowed to issue, because they are only removing owned assets that they issued
                self.balance.commit_asset_removal(product.issuer().as_ref(), &(&product).into(), count)
            },
        }
    }
    /// Load in the transactions from a trade file. Because of numbering, we must do this first; we cannot append
    pub async fn replay(&mut self, trade_file: &mut (impl tokio::io::AsyncBufRead + std::marker::Unpin), hard_audit: bool) -> Result<()> {
        let mut trade_file_lines = trade_file.lines();
        macro_rules! do_audit {
            () => {
                if hard_audit { self.hard_audit() } else { self.soft_audit() }
            };
        }
        let mut last_audit = do_audit!();
        while let Some(line) = trade_file_lines.next_line().await.expect("Could not read line from trade list") {
            let wrapped_action: WrappedAction = serde_json::from_str(&line).expect("Corrupted trade file");
            if wrapped_action.id != self.next_id {
                panic!("Trade file ID mismatch: action {} found on line {}: {}", wrapped_action.id, self.next_id, line);
            }
            self.apply_inner(self.next_id, wrapped_action.action.clone())?;
            if let Some(new_audit) = wrapped_action.action.adjust_audit(last_audit) {
                let post = do_audit!();
                if new_audit != post {
                    panic!("Failed audit on {line}: expected {new_audit:?} vs actual {post:?}");
                }
                last_audit = new_audit;
            }
            else {
                // The state has changed, adjust the audit
                last_audit = do_audit!();
            }
            self.next_id += 1;
        }
        Ok(())
    }
    /// Atomically try to apply an action with a give time, and if successful, write to given stream
    pub async fn apply_with_time(&mut self, action: Action, time: chrono::DateTime<chrono::Utc>, out: impl tokio::io::AsyncWrite) -> Result<u64> {
        let id = self.next_id;
        let wrapped_action = WrappedAction {
            id,
            time: time.to_utc(),
            action: action.clone(),
        };
        let mut line = serde_json::to_string(&wrapped_action).expect("Cannot serialise action");
        let pre = self.hard_audit();
        self.apply_inner(self.next_id, wrapped_action.action)?;
        // We can soft audit, as the last one was checked as required
        if let Some(expected) = action.adjust_audit(pre) {
            let post = self.soft_audit();
            if expected != post {
                panic!("Failed audit on {line}: expected {expected:?} vs actual {post:?}");
            }
        }
        line.push('\n');
        self.next_id += 1;
        let mut out = pin!(out);
        out.write_all(line.as_bytes()).await.expect("Could not write to log, must immediately stop!");
        out.flush().await.expect("Could not flush to log, must immediately stop!");
        Ok(id)
    }
    /// Atomically try to apply an action, and if successful, write to given stream
    pub fn apply(&mut self, action: Action, out: impl tokio::io::AsyncWrite) -> impl Future<Output=Result<u64>> {
        self.apply_with_time(action, chrono::Utc::now(), out)
    }
    /// Atomically try to apply an action, and if successful, write to given stream
    pub async fn apply_wrapped(&mut self, wrapped_action: WrappedAction, out: impl tokio::io::AsyncWrite) -> Result<u64> {
        if wrapped_action.id != self.next_id {
            return Err(Error::InvalidId { id: wrapped_action.id })
        }
        self.apply_with_time(wrapped_action.action, wrapped_action.time, out).await
    }
}
impl Auditable for State {
    fn soft_audit(&self) -> Audit {
        self.balance.soft_audit() + self.order.soft_audit() + self.withdrawal.soft_audit()
    }

    fn hard_audit(&self) -> Audit {
        self.balance.hard_audit() + self.order.hard_audit() + self.withdrawal.hard_audit()
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ItemisedAudit {
    pub balance: Audit,
    pub order: Audit,
    pub withdrawal: Audit,
}
impl State {
    pub fn itemised_audit(&self) -> ItemisedAudit {
        ItemisedAudit {
            balance: self.balance.soft_audit(),
            order: self.order.soft_audit(),
            withdrawal: self.withdrawal.soft_audit(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct StateSync {
    pub current_id: u64,
    pub balance: BalanceSync,
    pub rates: BankRates,
    pub auth: AuthSync,
    pub order: OrderSync,
    pub withdrawal: WithdrawalSync,
    pub shared_account: SharedSync
}
impl From<&State> for StateSync {
    fn from(value: &State) -> Self {
        Self {
            current_id: value.next_id.checked_sub(1).unwrap(),
            balance: (&value.balance).into(),
            rates: value.rates.clone(),
            auth: (&value.auth).into(),
            order: (&value.order).into(),
            withdrawal: (&value.withdrawal).into(),
            shared_account: (&value.shared_account).into()
        }
    }
}
impl TryFrom<StateSync> for State {
    type Error = Error;
    fn try_from(value: StateSync) -> Result<Self> {
        Ok(Self {
            next_id: value.current_id.checked_add(1).ok_or(Error::Overflow)?,
            rates: value.rates,
            balance: value.balance.try_into()?,
            order: value.order.try_into()?,
            withdrawal: value.withdrawal.try_into()?,
            auth: value.auth.try_into()?,
            shared_account: value.shared_account.try_into()?,
        })
    }
}
