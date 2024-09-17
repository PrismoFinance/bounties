use crate::{msg::BountyResponse, state::bounties::get_bounty as fetch_bounty};
use cosmwasm_std::{Deps, StdResult, Uint128};

pub fn get_bounty_handler(deps: Deps, bounty_id: Uint128) -> StdResult<BountyResponse> {
    let bounty = fetch_bounty(deps.storage, bounty_id)?;

    Ok(BountyResponse { bounty })
}
