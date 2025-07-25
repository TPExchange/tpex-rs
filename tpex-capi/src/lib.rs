#![allow(clippy::missing_safety_doc)]

use std::{pin::pin, str::FromStr, task::Context};

use tpex::{Auditable, State};

#[unsafe(no_mangle)]
pub extern "C" fn tpex_new() -> *mut std::sync::RwLock<State> {
    Box::into_raw(Box::new(std::sync::RwLock::new(State::new())))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tpex_free(state: *mut std::sync::RwLock<State>) {
    drop(unsafe { Box::from_raw(state) })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tpex_replay(state: *mut std::sync::RwLock<State>, updates: *const std::ffi::c_char, hard_audit: bool) -> bool {
    let mut state = unsafe { &mut *state }.write().unwrap();
    let mut updates = unsafe { std::ffi::CStr::from_ptr(updates) }.to_bytes();
    let future = state.replay(&mut updates, hard_audit);

    let mut ctx = Context::from_waker(std::task::Waker::noop());
    let std::task::Poll::Ready(result) = pin!(future).poll(&mut ctx)
    else { panic!("Somehow blocked on empty context"); };

    result.is_ok()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tpex_get_next_id(state: *mut std::sync::RwLock<State>) -> u64 {
    let state = unsafe { &mut *state }.read().unwrap();
    state.get_next_id()
}

#[repr(C)]
pub struct Audit {
    pub millicoins: u64,
    pub item_names: *mut *mut std::ffi::c_char,
    pub item_amounts: *mut u64,
    pub item_count: usize
}
impl From<tpex::Audit> for Audit {
    fn from(value: tpex::Audit) -> Self {
        let (mut names, mut amounts) : (Vec<*mut std::ffi::c_char>, Vec<u64>) =
            value.assets.into_iter()
            .map(|(name, amount)| (std::ffi::CString::new(name.as_bytes()).unwrap().into_raw(), amount))
            .unzip();
        let item_count = names.len();
        let names_ptr = names.as_mut_ptr();
        std::mem::forget(names);
        let amounts_ptr = amounts.as_mut_ptr();
        std::mem::forget(amounts);
        Audit {
            millicoins: value.coins.millicoins(),
            item_names: names_ptr,
            item_amounts: amounts_ptr,
            item_count
        }
    }
}
impl Drop for Audit {
    fn drop(&mut self) {
        let names: Vec<_> = unsafe { Vec::from_raw_parts(self.item_names, self.item_count, self.item_count) };
        let amounts = unsafe { Vec::from_raw_parts(self.item_amounts, self.item_count, self.item_count) };
        for i in names.into_iter() {
            drop(unsafe{std::ffi::CString::from_raw(i)})
        }
        drop(amounts);
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tpex_free_audit(audit: *mut Audit) {
    drop(unsafe { Box::from_raw(audit) })
}

#[repr(C)]
#[derive(PartialEq, PartialOrd, Eq, Ord)]
pub struct Level {
    price_millicoins: u64,
    count: u64
}

#[repr(C)]
pub struct PriceLevels {
    pub buy_levels: *mut Level,
    pub buy_count: usize,
    pub sell_levels: *mut Level,
    pub sell_count: usize,
}
impl PriceLevels {
    fn new<Buy: IntoIterator<Item=(tpex::Coins, u64)>, Sell: IntoIterator<Item=(tpex::Coins, u64)>>(buy: Buy, sell: Sell) -> Self {
        let mut buy = buy.into_iter()
            .map(|(price, count)| Level{price_millicoins: price.millicoins(), count})
            .collect::<Vec<_>>();
        let mut sell: Vec<Level> = sell.into_iter()
            .map(|(price, count)| Level{price_millicoins: price.millicoins(), count})
            .collect::<Vec<_>>();
        // Cheapest sell price first
        sell.sort_unstable();
        // Most expensive buy price first
        buy.sort_unstable_by(|x,y| y.cmp(x) );
        let ret = PriceLevels {
            buy_count: buy.len(),
            buy_levels: buy.as_mut_ptr(),
            sell_count: sell.len(),
            sell_levels: sell.as_mut_ptr(),
        };
        std::mem::forget(buy);
        std::mem::forget(sell);
        ret
    }
}
impl Drop for PriceLevels {
    fn drop(&mut self) {
        let buy: Vec<_> = unsafe { Vec::from_raw_parts(self.buy_levels, self.buy_count, self.buy_count) };
        let sell: Vec<_> = unsafe { Vec::from_raw_parts(self.sell_levels, self.sell_count, self.sell_count) };
        drop(buy);
        drop(sell);
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tpex_free_price_levels(price_levels: *mut PriceLevels) {
    drop(unsafe { Box::from_raw(price_levels) })
}

#[repr(C)]
pub struct Order {
    pub id: u64,
    pub millicoins_per: u64,
    pub player: *mut std::ffi::c_char,
    pub amount_remaining: u64,
    pub asset: *mut std::ffi::c_char,
    pub is_sell: bool,
    pub fee_ppm: u64,
}
impl From<&tpex::order::PendingOrder> for Order {
    fn from(value: &tpex::order::PendingOrder) -> Self {
        Order {
            id: value.id,
            millicoins_per: value.coins_per.millicoins(),
            player: std::ffi::CString::from_str(value.player.get_raw_name()).expect("Null in username").into_raw(),
            amount_remaining: value.amount_remaining,
            asset: std::ffi::CString::from_str(&value.asset).expect("Null in asset name").into_raw(),
            is_sell: match value.order_type { tpex::order::OrderType::Buy => false, tpex::order::OrderType::Sell => true },
            fee_ppm: value.fee_ppm,
        }
    }
}
impl Drop for Order {
    fn drop(&mut self) {
        drop(unsafe { std::ffi::CString::from_raw(self.player) });
        drop(unsafe { std::ffi::CString::from_raw(self.asset) });
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tpex_free_order(order: *mut Order) {
    drop(unsafe { Box::from_raw(order) })
}

#[repr(C)]
pub struct OrderList {
    pub buy: *mut Order,
    pub buy_count: usize,
    pub sell: *mut Order,
    pub sell_count: usize
}
impl OrderList {
    fn new(mut buy: Vec<Order>, mut sell: Vec<Order>) -> Self {
        let ret = OrderList {
            buy_count: buy.len(),
            buy: buy.as_mut_ptr(),
            sell_count: sell.len(),
            sell: sell.as_mut_ptr(),
        };
        std::mem::forget(buy);
        std::mem::forget(sell);
        ret
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tpex_free_order_list(order_list: *mut OrderList) {
    drop(unsafe { Box::from_raw(order_list) })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tpex_audit(state: *mut std::sync::RwLock<State>) -> *mut Audit {
    let state = unsafe { &mut *state }.read().unwrap();
    Box::into_raw(Box::new(state.soft_audit().into()))
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tpex_audit_player(state: *mut std::sync::RwLock<State>, player: *const std::ffi::c_char) -> *mut Audit {
    let state = unsafe { &mut *state }.read().unwrap();
    let Ok(player) = unsafe { std::ffi::CStr::from_ptr(player) }.to_str().map(ToOwned::to_owned).map(tpex::PlayerId::assume_username_correct)
    else { return std::ptr::null_mut() };
    Box::into_raw(Box::new(tpex::Audit {
        coins: state.get_bal(&player),
        assets: state.get_assets(&player),
    }.into()))
}
#[unsafe(no_mangle)]
pub extern "C" fn tpex_prettify_millicoins(millicoins: u64) -> *mut std::ffi::c_char {
    std::ffi::CString::new(tpex::Coins::from_millicoins(millicoins).to_string()).unwrap().into_raw()
}
#[unsafe(no_mangle)]
pub static INVALID_COINS: u64 = u64::MAX;
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tpex_parse_millicoins(millicoins: *const std::ffi::c_char) -> u64 {
    let Ok(millicoins_safe) = unsafe { std::ffi::CStr::from_ptr(millicoins) }.to_str() else { return INVALID_COINS };
    tpex::Coins::from_str(millicoins_safe).map(|x| x.millicoins()).unwrap_or(INVALID_COINS)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tpex_get_prices(state: *mut std::sync::RwLock<State>, asset: *const std::ffi::c_char) -> *mut PriceLevels {
    let state = unsafe { &mut *state }.read().unwrap();
    let Ok(asset) = unsafe { std::ffi::CStr::from_ptr(asset) }.to_str().map(ToOwned::to_owned)
    else { return std::ptr::null_mut() };
    let (buy, sell) = state.get_prices(&asset);
    Box::into_raw(Box::new(PriceLevels::new(buy, sell)))
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tpex_get_orders(state: *mut std::sync::RwLock<State>, player: *const std::ffi::c_char) -> *mut OrderList {
    let state = unsafe { &mut *state }.read().unwrap();
    let Ok(player) = unsafe { std::ffi::CStr::from_ptr(player) }.to_str().map(ToOwned::to_owned).map(tpex::PlayerId::assume_username_correct)
    else { return std::ptr::null_mut() };
    let (buy, sell) =
        state.get_orders_filter(|i| i.player == player)
        .map(|i| Order::from(&i))
        .partition(|i| i.is_sell );

    Box::into_raw(Box::new(OrderList::new(buy, sell)))
}
