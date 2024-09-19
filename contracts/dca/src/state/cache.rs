use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coin, SubMsg, Uint128};
use cw_storage_plus::{Item, Map};
use std::collections::VecDeque;

pub const BOUNTY_ID_CACHE: Item<Uint128> = Item::new("bounty_id_cache_v1");

// #[cw_serde]
// pub struct SwapCache {
  //  pub swap_denom_balance: Coin,
  //  pub receive_denom_balance: Coin,
// }

// pub const SWAP_CACHE: Item<SwapCache> = Item::new("swap_cache_v1");

#[cw_serde]
pub struct PostExecutionActionCacheEntry {
    pub msg: SubMsg,
    pub funds: Vec<Coin>,
}

pub const POST_EXECUTION_ACTION_CACHE: Map<u128, VecDeque<PostExecutionActionCacheEntry>> =
    Map::new("post_execution_action_cache_v1");
