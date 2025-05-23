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
// #[unsafe(no_mangle)]
// pub unsafe extern "C" fn tpex_audit_player(state: *mut std::sync::RwLock<State>, player: *const std::ffi::c_char) -> Audit {
//     let state = unsafe { &mut *state };
//     let mut player = unsafe { std::ffi::CStr::from_ptr(player) }.to_str().unwrap();
//     Audit { millicoins: () }
//     state.get_bal(&tpex::PlayerId::evil_constructor(player.to_string())).millicoins()
// }

// #[unsafe(no_mangle)]
// pub unsafe extern "C" fn tpex_audit(state: *mut std::sync::RwLock<State>) -> u64 {
// }

// #[unsafe(no_mangle)]
// pub unsafe extern "C" fn tpex_audit_player(state: *mut std::sync::RwLock<State>, player: *const std::ffi::c_char) -> Audit {
//     let state = unsafe { &mut *state };
//     let mut player = unsafe { std::ffi::CStr::from_ptr(player) }.to_str().unwrap();
//     Audit { millicoins: () }state.get_bal(&tpex::PlayerId::evil_constructor(player.to_string())).millicoins()
// }

// pub struct OrderList {
//     buy_orders: *mut [Order],
//     sell_orders: *mut [Order],
// }
// pub struct Order {
//     millicoins: u64,
//     count: u64
// }

// #[unsafe(no_mangle)]
// pub unsafe extern "C" fn tpex_free_order_list(OrderList: *mut OrderList) {
//     Box::from
// }

// #[unsafe(no_mangle)]
// pub unsafe extern "C" fn tpex_get(state: *mut std::sync::RwLock<State>) -> *mut [Order] {
//     let state = unsafe { &mut *state };
//     state.get_next_id()
// }
