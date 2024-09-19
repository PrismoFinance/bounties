use crate::constants::{
    AFTER_DELEGATION_REPLY_ID, AFTER_FAILED_AUTOMATION_REPLY_ID, AFTER_LIMIT_ORDER_PLACED_REPLY_ID,
    AFTER_SWAP_REPLY_ID, FAIL_SILENTLY_REPLY_ID,
};
use crate::error::ContractError;
use crate::handlers::cancel_bounty::cancel_bounty_handler;
use crate::handlers::create_bounty::{create_bounty_handler, save_price_trigger};
use crate::handlers::deposit::deposit_handler;
use crate::handlers::disburse_escrow::disburse_escrow_handler;
use crate::handlers::disburse_funds::disburse_funds_handler;
use crate::handlers::execute_trigger::execute_trigger_handler;
use crate::handlers::get_config::get_config_handler;
use crate::handlers::get_disburse_escrow_tasks::get_disburse_escrow_tasks_handler;
use crate::handlers::get_events::get_events_handler;
use crate::handlers::get_events_by_resource_id::get_events_by_resource_id_handler;
use crate::handlers::get_pairs::get_pairs_handler;
use crate::handlers::get_time_trigger_ids::get_time_trigger_ids_handler;
use crate::handlers::get_trigger_id_by_fin_limit_order_idx::get_trigger_id_by_fin_limit_order_idx_handler;
use crate::handlers::get_bounty::get_bounty_handler;
use crate::handlers::get_bounty_performance::get_bounty_performance_handler;
use crate::handlers::get_bounties::get_bounties_handler;
use crate::handlers::get_bounties_by_address::get_bounties_by_address_handler;
use crate::handlers::handle_failed_automation::handle_failed_automation_handler;
use crate::handlers::instantiate::instantiate_handler;
use crate::handlers::migrate::migrate_handler;
use crate::handlers::update_config::update_config_handler;
use crate::handlers::update_swap_adjustment_handler::update_swap_adjustment_handler;
use crate::handlers::update_bounty::update_bounty_handler;
use crate::handlers::z_delegate::{log_delegation_result, z_delegate_handler};
use crate::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};
use cosmwasm_std::from_json;
#[cfg(not(feature = "library"))]
use cosmwasm_std::{
    entry_point, to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response,
    StdResult,
};
use shared::cw20::from_cw20_receive_msg;

pub const CONTRACT_NAME: &str = "crates.io:calc-dca";
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[entry_point]
pub fn migrate(deps: DepsMut, _: Env, msg: MigrateMsg) -> Result<Response, ContractError> {
    migrate_handler(deps, msg)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _: Env,
    _: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    instantiate_handler(deps, msg)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::CreateBounty {
            owner,
            label,
            bounty_description,
            destinations,
            target_denom,
            route,
            slippage_tolerance,
            minimum_receive_amount,
           // swap_amount,
            time_interval,
            target_start_time_utc_seconds,
            target_receive_amount,
          //  performance_assessment_strategy,
          //  swap_adjustment_strategy,
        } => create_bounty_handler(
            deps,
            env,
            &info,
            owner.unwrap_or_else(|| info.sender.clone()),
            label,
            bounty_description,
            destinations.unwrap_or_default(),
            target_denom,
            route,
            slippage_tolerance,
            minimum_receive_amount,
           // swap_amount,
            time_interval,
            target_start_time_utc_seconds,
            target_receive_amount,
           // performance_assessment_strategy,
           // swap_adjustment_strategy,
        ),
        ExecuteMsg::UpdateBounty {
            vault_id,
            label,
            bounty_description,
            destinations,
            slippage_tolerance,
            minimum_receive_amount,
            time_interval,
           // swap_adjustment_strategy,
           // swap_amount,
        } => update_bounty_handler(
            deps,
            env,
            info,
            bounty_id,
            label,
            bounty_description,
            destinations,
            slippage_tolerance,
            minimum_receive_amount,
            time_interval,
            // swap_adjustment_strategy,
            // swap_amount,
        ),
        ExecuteMsg::CancelBounty { bounty_id } => cancel_bounty_handler(deps, env, info, bounty_id),
        ExecuteMsg::ExecuteTrigger { trigger_id, route } => {
            execute_trigger_handler(deps, env, trigger_id, route)
        }
        ExecuteMsg::Deposit { address, bounty_id } => {
            deposit_handler(deps, env, info, address, bounty_id)
        }
        ExecuteMsg::UpdateConfig {
            executors,
            fee_collectors,
           // default_swap_fee_percent,
           // weighted_scale_swap_fee_percent,
            automation_fee_percent,
            default_page_limit,
            paused,
            risk_weighted_average_escrow_level,
            // twap_period,
            default_slippage_tolerance,
            exchange_contract_address,
        } => update_config_handler(
            deps,
            info,
            executors,
            fee_collectors,
           // default_swap_fee_percent,
           // weighted_scale_swap_fee_percent,
            automation_fee_percent,
            default_page_limit,
            paused,
            risk_weighted_average_escrow_level,
           // twap_period,
            default_slippage_tolerance,
            exchange_contract_address,
        ),
        // ExecuteMsg::UpdateSwapAdjustment { strategy, value } => {
           // update_swap_adjustment_handler(deps, env, info, strategy, value)
       // }
        ExecuteMsg::DisburseEscrow { bounty_id } => {
            disburse_escrow_handler(deps, env, info, bounty_id)
        }
        ExecuteMsg::ZDelegate {
            delegator_address,
            validator_address,
        } => z_delegate_handler(
            deps.as_ref(),
            env,
            info,
            delegator_address,
            validator_address,
        ),
        ExecuteMsg::Receive(receive_msg) => {
            let info = from_cw20_receive_msg(&deps.as_ref(), info, receive_msg.clone())?;
            let msg = from_json(receive_msg.msg)?;
            match msg {
                ExecuteMsg::Receive(_) => {
                    Err(ContractError::Std(cosmwasm_std::StdError::GenericErr {
                        msg: "nested receive not allowed".to_string(),
                    }))
                }
                _ => execute(deps, env, info, msg),
            }
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, reply: Reply) -> Result<Response, ContractError> {
    match reply.id {
        AFTER_LIMIT_ORDER_PLACED_REPLY_ID => save_price_trigger(deps, reply),
        // AFTER_SWAP_REPLY_ID => disburse_funds_handler(deps, &env, reply),
        AFTER_FAILED_AUTOMATION_REPLY_ID => handle_failed_automation_handler(deps, env, reply),
        AFTER_DELEGATION_REPLY_ID => log_delegation_result(reply),
        FAIL_SILENTLY_REPLY_ID => Ok(Response::new()),
        id => Err(ContractError::CustomError {
            val: format!("unhandled DCA contract reply id: {}", id),
        }),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetPairs { limit, start_after } => {
            to_json_binary(&get_pairs_handler(deps, limit, start_after)?)
        }
        QueryMsg::GetTimeTriggerIds { limit } => {
            to_json_binary(&get_time_trigger_ids_handler(deps, env, limit)?)
        }
        QueryMsg::GetTriggerIdByFinLimitOrderIdx { order_idx } => to_json_binary(
            &get_trigger_id_by_fin_limit_order_idx_handler(deps, order_idx)?,
        ),
        QueryMsg::GetBounties {
            start_after,
            limit,
            reverse,
        } => to_json_binary(&get_bounties_handler(deps, start_after, limit, reverse)?),
        QueryMsg::GetBountiesByAddress {
            address,
            status,
            start_after,
            limit,
        } => to_json_binary(&get_bounties_by_address_handler(
            deps,
            address,
            status,
            start_after,
            limit,
        )?),
        QueryMsg::GetBounty { bounty_id } => to_json_binary(&get_bounty_handler(deps, bounty_id)?),
        QueryMsg::GetEventsByResourceId {
            resource_id,
            start_after,
            limit,
            reverse,
        } => to_json_binary(&get_events_by_resource_id_handler(
            deps,
            resource_id,
            start_after,
            limit,
            reverse,
        )?),
        QueryMsg::GetEvents {
            start_after,
            limit,
            reverse,
        } => to_json_binary(&get_events_handler(deps, start_after, limit, reverse)?),
        QueryMsg::GetConfig {} => to_json_binary(&get_config_handler(deps)?),
        QueryMsg::GetVaultPerformance { vault_id } => {
            to_json_binary(&get_bounty_performance_handler(deps, bounty_id)?)
        }
        QueryMsg::GetDisburseEscrowTasks { limit } => {
            to_json_binary(&get_disburse_escrow_tasks_handler(deps, env, limit)?)
        }
    }
}
