use crate::constants::AFTER_SWAP_REPLY_ID;
use crate::error::ContractError;
// use crate::helpers::price::{get_expected_receive_amount, get_slippage, get_twap_to_now};
use crate::helpers::time::get_next_target_time;
use crate::helpers::validation::{assert_contract_is_not_paused, assert_target_time_is_in_past};
// use crate::helpers::vault::{get_swap_amount, simulate_standard_dca_execution};
use crate::msg::ExecuteMsg;
use crate::state::cache::{SwapCache, SWAP_CACHE, BOUNTY_ID_CACHE};
use crate::state::config::get_config;
use crate::state::events::create_event;
use crate::state::triggers::{delete_trigger, save_trigger};
use crate::state::vaults::{get_bounty, update_bounty};
use crate::types::event::{EventBuilder, EventData, ExecutionSkippedReason};
// use crate::types::swap_adjustment_strategy::SwapAdjustmentStrategy;
use crate::types::trigger::{Trigger, TriggerConfiguration};
use crate::types::vault::{Bounty, BountyStatus};
use cosmwasm_std::{to_json_binary, Binary, Coin, Decimal, SubMsg, WasmMsg};
#[cfg(not(feature = "library"))]
use cosmwasm_std::{DepsMut, Env, Response, Uint128};
use exchange::msg::{ExecuteMsg as ExchangeExecuteMsg, Order, QueryMsg as ExchangeQueryMsg};
use shared::balance::query_balance;
use shared::cw20::into_execute_msg;

// We want a few triggers. 
// 1) Escrow Trigger --> a. Rejected Bounty --> Trigger Rejection and refund the Bounty Issuer. b.Completed Bounty --> Trigger Completion and send pay to Bounty Assignee
// 2) Time Trigger --> Set time for the bounty to be completed. If time elapses then the bounty is no longer active and it is treated like an Escrow Rejected Bounty. (e.g. Funds are sent back to the Bounty Issuer)
// If a Time Trigger is activated, then the bounty is no longer available and a new bounty needs created. 


pub fn execute_trigger_handler(
    deps: DepsMut,
    env: Env,
    trigger_id: Uint128,
    mut route: Option<Binary>,
) -> Result<Response, ContractError> {
    assert_contract_is_not_paused(deps.storage)?;

    let mut bounty = get_bounty(deps.storage, trigger_id)?;

    let mut response = Response::new()
        .add_attribute("execute_trigger", "true")
        .add_attribute("bounty_id", bounty.id)
        .add_attribute("owner", bounty.owner.clone());

    delete_trigger(deps.storage, bounty.id)?;

    if bounty.is_cancelled() {
        return Err(ContractError::CustomError {
            val: format!(
                "bounty with id {} is cancelled, and is not available for execution",
                bounty.id
            ),
        });
    }

    // If there is no trigger then the bounty is not valid. We need to have a way for a trigger to be set on creation and then they can update the trigger if it changes.
    if bounty.trigger.is_none() {
        return Err(ContractError::CustomError {
            val: format!(
                "Bounty with id {} has no trigger attached, and is not available for execution",
                bounty.id
            ),
        });
    }

    match bounty.trigger {
        Some(TriggerConfiguration::Time { target_time }) => {
            assert_target_time_is_in_past(env.block.time, target_time)?;
        }
        Some(TriggerConfiguration::Price { order_idx, .. }) => {
            let config = get_config(deps.storage)?;

            let order = deps.querier.query_wasm_smart::<Order>(
                config.exchange_contract_address.clone(),
                &ExchangeQueryMsg::GetOrder {
                    order_idx,
                    denoms: vault.denoms(),
                },
            )?;

            if !order.remaining_offer_amount.amount.is_zero() {
                return Err(ContractError::CustomError {
                    val: String::from("target price has not been met"),
                });
            }

            response = response.add_submessage(SubMsg::new(WasmMsg::Execute {
                contract_addr: config.exchange_contract_address.to_string(),
                msg: to_json_binary(&ExchangeExecuteMsg::WithdrawOrder {
                    order_idx,
                    denoms: vault.denoms(),
                })
                .unwrap(),
                funds: vec![],
            }));
        }
        _ => {
            return Err(ContractError::CustomError {
                val: format!(
                    "vault with id {} has no trigger attached, and is not available for execution",
                    vault.id
                ),
            });
        }
    }

    if vault.is_scheduled() {
        vault = update_vault(
            deps.storage,
            Vault {
                status: VaultStatus::Active,
                started_at: Some(env.block.time),
                ..vault
            },
        )?;
    }

    let config = get_config(deps.storage)?;

    let adjusted_swap_amount = get_swap_amount(&deps.as_ref(), &env, &vault)?;

    if route.is_some() {
        let expected_receive_amount_new_route = get_expected_receive_amount(
            &deps.querier,
            config.exchange_contract_address.clone(),
            adjusted_swap_amount.clone(),
            vault.target_denom.clone(),
            route.clone(),
        )?;

        if expected_receive_amount_new_route.denom != vault.target_denom {
            return Err(ContractError::CustomError {
                val: String::from("route target denom does not match vault target denom"),
            });
        }

        let expected_receive_amount_old_route = get_expected_receive_amount(
            &deps.querier,
            config.exchange_contract_address.clone(),
            adjusted_swap_amount.clone(),
            vault.target_denom.clone(),
            vault.route.clone(),
        )?;

        route = if expected_receive_amount_new_route.amount
            < expected_receive_amount_old_route.amount
        {
            vault.route.clone()
        } else {
            route.clone()
        };
    } else {
        route = vault.route.clone();
    }

    let twap_price = get_twap_to_now(
        &deps.querier,
        config.exchange_contract_address.clone(),
        vault.get_swap_denom(),
        vault.target_denom.clone(),
        config.twap_period,
        route.clone(),
    )?;

    create_event(
        deps.storage,
        EventBuilder::new(
            vault.id,
            env.block.to_owned(),
            EventData::DcaVaultExecutionTriggered {
                base_denom: vault.target_denom.clone(),
                quote_denom: vault.get_swap_denom(),
                asset_price: twap_price,
            },
        ),
    )?;

    response = response.add_attribute("twap_price", twap_price.to_string());

    if let Some(SwapAdjustmentStrategy::RiskWeightedAverage { .. }) = vault.swap_adjustment_strategy
    {
        (vault, response) = simulate_standard_dca_execution(
            response,
            &deps.querier,
            deps.storage,
            &env,
            vault,
            twap_price,
        )?;
    }

    let should_execute_again = vault.is_active()
        || vault.performance_assessment_strategy.clone().map_or(
            false,
            |performance_assessment_strategy| {
                performance_assessment_strategy.should_continue(&vault)
            },
        );

    if should_execute_again {
        save_trigger(
            deps.storage,
            Trigger {
                vault_id: vault.id,
                configuration: TriggerConfiguration::Time {
                    target_time: get_next_target_time(
                        env.block.time,
                        vault.started_at.unwrap_or(env.block.time),
                        vault.time_interval.clone(),
                    ),
                },
            },
        )?;
    } else {
        if vault.should_not_continue() && vault.escrowed_amount.amount > Uint128::zero() {
            response = response.add_submessage(SubMsg::new(WasmMsg::Execute {
                contract_addr: env.contract.address.to_string(),
                msg: to_json_binary(&ExecuteMsg::DisburseEscrow { vault_id: vault.id })?,
                funds: vec![],
            }));
        }

        return Ok(response.add_attribute("execution_skipped", "vault_should_not_continue"));
    }

    if vault.is_inactive() {
        return Ok(response.add_attribute("execution_skipped", "vault_is_inactive"));
    }

    if adjusted_swap_amount.amount.is_zero() {
        create_event(
            deps.storage,
            EventBuilder::new(
                vault.id,
                env.block.clone(),
                EventData::DcaVaultExecutionSkipped {
                    reason: ExecutionSkippedReason::SwapAmountAdjustedToZero,
                },
            ),
        )?;

        return Ok(response
            .add_attribute("execution_skipped", "swap_amount_adjusted_to_zero")
            .add_attribute("twap_price", twap_price.to_string()));
    }

    if vault.price_threshold_exceeded(twap_price)? {
        create_event(
            deps.storage,
            EventBuilder::new(
                vault.id,
                env.block,
                EventData::DcaVaultExecutionSkipped {
                    reason: ExecutionSkippedReason::PriceThresholdExceeded { price: twap_price },
                },
            ),
        )?;

        return Ok(response
            .add_attribute("execution_skipped", "price_threshold_exceeded")
            .add_attribute("twap_price", twap_price.to_string()));
    };

    let get_slippage_result = get_slippage(
        &deps.querier,
        config.exchange_contract_address.clone(),
        adjusted_swap_amount.clone(),
        vault.target_denom.clone(),
        twap_price,
        route.clone(),
    );

    match get_slippage_result {
        Ok(slippage) => {
            if slippage > vault.slippage_tolerance {
                create_event(
                    deps.storage,
                    EventBuilder::new(
                        vault.id,
                        env.block,
                        EventData::DcaVaultExecutionSkipped {
                            reason: ExecutionSkippedReason::SlippageToleranceExceeded,
                        },
                    ),
                )?;

                return Ok(response
                    .add_attribute("execution_skipped", "slippage_tolerance_exceeded")
                    .add_attribute("twap_price", twap_price.to_string())
                    .add_attribute("slippage", slippage.to_string()));
            }
        }
        Err(_) => {
            create_event(
                deps.storage,
                EventBuilder::new(
                    vault.id,
                    env.block,
                    EventData::DcaVaultExecutionSkipped {
                        reason: ExecutionSkippedReason::SlippageQueryError,
                    },
                ),
            )?;

            return Ok(response.add_attribute("execution_skipped", "slippage_query_error"));
        }
    }

    VAULT_ID_CACHE.save(deps.storage, &vault.id)?;

    SWAP_CACHE.save(
        deps.storage,
        &SwapCache {
            swap_denom_balance: query_balance(
                deps.api,
                &deps.querier,
                &vault.get_swap_denom(),
                &env.contract.address,
            )?,
            receive_denom_balance: query_balance(
                deps.api,
                &deps.querier,
                &vault.target_denom,
                &env.contract.address,
            )?,
        },
    )?;

    let adjusted_minimum_receive_amount =
        vault
            .minimum_receive_amount
            .map_or(Uint128::zero(), |minimum_receive_amount| {
                Decimal::from_ratio(adjusted_swap_amount.amount, vault.swap_amount)
                    * minimum_receive_amount
            });

    Ok(response.add_submessage(SubMsg::reply_always(
        into_execute_msg(
            deps.api,
            config.exchange_contract_address.clone(),
            to_json_binary(&ExchangeExecuteMsg::Swap {
                minimum_receive_amount: Coin {
                    amount: adjusted_minimum_receive_amount,
                    denom: vault.target_denom,
                },
                route,
            })?,
            adjusted_swap_amount,
        )?,
        AFTER_SWAP_REPLY_ID,
    )))
}

#[cfg(test)]
mod execute_trigger_tests {
    use super::*;
    use crate::constants::{
        AFTER_SWAP_REPLY_ID, HALF_DECIMAL, ONE, ONE_MICRON, TEN, THREE_DECIMAL, TWO_MICRONS,
    };
    use crate::handlers::get_events_by_resource_id::get_events_by_resource_id_handler;
    use crate::helpers::fees::{get_automation_fee_rate, get_swap_fee_rate};
    use crate::helpers::vault::get_swap_amount;
    use crate::msg::ExecuteMsg;
    use crate::state::config::update_config;
    use crate::state::swap_adjustments::update_swap_adjustment;
    use crate::state::triggers::delete_trigger;
    use crate::state::vaults::get_vault;
    use crate::tests::helpers::{instantiate_contract, setup_vault};
    use crate::tests::mocks::{calc_mock_dependencies, ADMIN, DENOM_UKUJI, DENOM_UUSK};
    use crate::types::config::Config;
    use crate::types::event::{Event, EventData, ExecutionSkippedReason};
    use crate::types::performance_assessment_strategy::PerformanceAssessmentStrategy;
    use crate::types::position_type::PositionType;
    use crate::types::swap_adjustment_strategy::{BaseDenom, SwapAdjustmentStrategy};
    use crate::types::trigger::TriggerConfiguration;
    use crate::types::vault::{Vault, VaultStatus};
    use cosmwasm_std::testing::{mock_env, mock_info};
    use cosmwasm_std::{to_json_binary, Coin, Decimal, SubMsg, Uint128, WasmMsg};

    #[test]
    fn when_contract_is_paused_should_fail() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(deps.as_mut(), env.clone(), Vault::default());

        update_config(
            deps.as_mut().storage,
            Config {
                paused: true,
                ..Config::default()
            },
        )
        .unwrap();

        let err = execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap_err();

        assert_eq!(err.to_string(), "Error: contract is paused");
    }

    #[test]
    fn when_vault_is_cancelled_should_fail() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                status: VaultStatus::Cancelled,
                ..Vault::default()
            },
        );

        let err = execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error: vault with id 0 is cancelled, and is not available for execution"
        );
    }

    #[test]
    fn when_vault_is_cancelled_should_delete_trigger() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                status: VaultStatus::Cancelled,
                ..Vault::default()
            },
        );

        execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap_err();

        let updated_vault = get_vault(deps.as_ref().storage, vault.id).unwrap();

        assert!(vault.trigger.is_some());
        assert_eq!(updated_vault.trigger, None);
    }

    #[test]
    fn when_no_trigger_exists_should_fail() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                status: VaultStatus::Inactive,
                ..Vault::default()
            },
        );

        delete_trigger(deps.as_mut().storage, vault.id).unwrap();

        let err = execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error: vault with id 0 has no trigger attached, and is not available for execution"
        );
    }

    #[test]
    fn when_trigger_is_not_ready_to_fire_should_fail() {
        let mut deps = calc_mock_dependencies();
        let mut env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(deps.as_mut(), env.clone(), Vault::default());

        env.block.time = env.block.time.minus_seconds(10);

        let err = execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error: trigger execution time has not yet elapsed"
        );
    }

    #[test]
    fn should_make_scheduled_vault_active() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                status: VaultStatus::Scheduled,
                ..Vault::default()
            },
        );

        execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap();

        let updated_vault = get_vault(deps.as_ref().storage, vault.id).unwrap();

        assert_eq!(vault.status, VaultStatus::Scheduled);
        assert_eq!(updated_vault.status, VaultStatus::Active);
    }

    #[test]
    fn should_set_scheduled_vault_start_time() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                status: VaultStatus::Scheduled,
                ..Vault::default()
            },
        );

        execute_trigger_handler(deps.as_mut(), env.clone(), vault.id, None).unwrap();

        let updated_vault = get_vault(deps.as_ref().storage, vault.id).unwrap();

        assert_eq!(vault.started_at, None);
        assert_eq!(updated_vault.started_at, Some(env.block.time));
    }

    #[test]
    fn publishes_execution_triggered_event() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(deps.as_mut(), env.clone(), Vault::default());

        execute_trigger_handler(deps.as_mut(), env.clone(), vault.id, None).unwrap();

        let events = get_events_by_resource_id_handler(deps.as_ref(), vault.id, None, None, None)
            .unwrap()
            .events;

        assert_eq!(
            events.first().unwrap(),
            &Event {
                id: 1,
                resource_id: vault.id,
                timestamp: env.block.time,
                block_height: env.block.height,
                data: EventData::DcaVaultExecutionTriggered {
                    base_denom: vault.target_denom.clone(),
                    quote_denom: vault.get_swap_denom(),
                    asset_price: Decimal::one()
                }
            }
        );
    }

    #[test]
    fn with_price_trigger_should_withdraw_limit_order() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let order_idx = Uint128::new(46);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                trigger: Some(TriggerConfiguration::Price {
                    target_price: Decimal::percent(200),
                    order_idx,
                }),
                ..Vault::default()
            },
        );

        let response = execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap();

        let config = get_config(deps.as_ref().storage).unwrap();

        assert_eq!(
            response.messages.first().unwrap(),
            &SubMsg::new(WasmMsg::Execute {
                contract_addr: config.exchange_contract_address.to_string(),
                msg: to_json_binary(&ExchangeExecuteMsg::WithdrawOrder {
                    order_idx,
                    denoms: vault.denoms()
                })
                .unwrap(),
                funds: vec![]
            })
        );
    }

    #[test]
    fn for_non_standard_dca_should_simulate_execution() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::default()),
                performance_assessment_strategy: Some(PerformanceAssessmentStrategy::default()),
                ..Vault::default()
            },
        );

        execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap();

        let updated_vault = get_vault(deps.as_ref().storage, vault.id).unwrap();

        let fee_rate = get_swap_fee_rate(deps.as_mut().storage, &vault.swap_adjustment_strategy)
            .unwrap()
            + get_automation_fee_rate(deps.as_mut().storage, &vault).unwrap();

        let received_amount_before_fee = vault.swap_amount * Decimal::percent(95);
        let fee_amount = received_amount_before_fee * fee_rate;
        let received_amount_after_fee = received_amount_before_fee - fee_amount;

        let performance_assessment_strategy =
            updated_vault.performance_assessment_strategy.unwrap();

        assert_eq!(
            match performance_assessment_strategy.clone() {
                PerformanceAssessmentStrategy::CompareToStandardDca { swapped_amount, .. } =>
                    swapped_amount,
            },
            Coin::new(vault.swap_amount.into(), vault.get_swap_denom()),
        );
        assert_eq!(
            match performance_assessment_strategy {
                PerformanceAssessmentStrategy::CompareToStandardDca {
                    received_amount, ..
                } => received_amount,
            },
            Coin::new(received_amount_after_fee.into(), vault.target_denom)
        );
    }

    #[test]
    fn with_finished_compare_to_standard_dca_performance_assessment_should_not_simulate_execution()
    {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                deposited_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                escrowed_amount: Coin::new((TEN * Decimal::percent(5)).into(), DENOM_UUSK),
                performance_assessment_strategy: Some(
                    PerformanceAssessmentStrategy::CompareToStandardDca {
                        swapped_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                        received_amount: Coin::new(TEN.into(), DENOM_UUSK),
                    },
                ),
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::default()),
                ..Vault::default()
            },
        );

        execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap();

        let updated_vault = get_vault(deps.as_ref().storage, vault.id).unwrap();

        assert_eq!(
            updated_vault.swap_adjustment_strategy.unwrap(),
            vault.swap_adjustment_strategy.unwrap()
        );
    }

    #[test]
    fn with_swap_adjusted_to_zero_should_publish_execution_skipped_event() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::WeightedScale {
                    base_receive_amount: ONE,
                    multiplier: Decimal::percent(500),
                    increase_only: false,
                }),
                ..Vault::default()
            },
        );

        deps.querier.update_fin_price(&THREE_DECIMAL);

        execute_trigger_handler(deps.as_mut(), env.clone(), vault.id, None).unwrap();

        let events = get_events_by_resource_id_handler(deps.as_ref(), vault.id, None, None, None)
            .unwrap()
            .events;

        assert!(events.contains(&Event {
            id: 2,
            resource_id: vault.id,
            timestamp: env.block.time,
            block_height: env.block.height,
            data: EventData::DcaVaultExecutionSkipped {
                reason: ExecutionSkippedReason::SwapAmountAdjustedToZero
            }
        }));
    }

    #[test]
    fn with_swap_adjusted_to_zero_should_not_send_swap_message() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::WeightedScale {
                    base_receive_amount: ONE,
                    multiplier: Decimal::percent(500),
                    increase_only: false,
                }),
                ..Vault::default()
            },
        );

        deps.querier.update_fin_price(&THREE_DECIMAL);

        let response = execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap();

        assert!(response.messages.is_empty());
    }

    #[test]
    fn with_rwa_swap_adjustment_strategy_should_adjust_swap_amount() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                deposited_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                escrowed_amount: Coin::new(0, DENOM_UUSK),
                performance_assessment_strategy: Some(PerformanceAssessmentStrategy::default()),
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::default()),
                ..Vault::default()
            },
        );

        let swap_adjustment = Decimal::percent(150);

        [PositionType::Enter, PositionType::Exit]
            .into_iter()
            .for_each(|position_type| {
                update_swap_adjustment(
                    deps.as_mut().storage,
                    SwapAdjustmentStrategy::RiskWeightedAverage {
                        model_id: 30,
                        base_denom: BaseDenom::Bitcoin,
                        position_type,
                    },
                    swap_adjustment,
                    env.block.time,
                )
                .unwrap();
            });

        let response = execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap();

        let config = get_config(deps.as_ref().storage).unwrap();

        assert_eq!(
            response.messages.first().unwrap(),
            &SubMsg::reply_always(
                WasmMsg::Execute {
                    contract_addr: config.exchange_contract_address.to_string(),
                    msg: to_json_binary(&ExchangeExecuteMsg::Swap {
                        minimum_receive_amount: Coin {
                            amount: vault.minimum_receive_amount.unwrap_or(Uint128::zero()),
                            denom: vault.target_denom.clone(),
                        },
                        route: vault.route.clone()
                    })
                    .unwrap(),
                    funds: vec![Coin::new(
                        (vault.swap_amount * swap_adjustment).into(),
                        vault.get_swap_denom()
                    )]
                },
                AFTER_SWAP_REPLY_ID,
            )
        )
    }

    #[test]
    fn with_rwa_swap_adjustment_strategy_and_exceeded_slippage_tolerance_should_simulate_skipped_execution(
    ) {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                slippage_tolerance: Decimal::percent(1),
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::default()),
                ..Vault::default()
            },
        );

        deps.querier.update_fin_price(&HALF_DECIMAL);

        execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap();

        let updated_vault = get_vault(deps.as_ref().storage, vault.id).unwrap();

        assert_eq!(
            updated_vault.swap_adjustment_strategy.unwrap(),
            SwapAdjustmentStrategy::default()
        );
    }

    #[test]
    fn with_rwa_swap_adjustment_strategy_and_exceeded_price_threshold_should_publish_execution_skipped_event(
    ) {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                swap_amount: ONE,
                minimum_receive_amount: Some(ONE + ONE),
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::default()),
                performance_assessment_strategy: Some(PerformanceAssessmentStrategy::default()),
                ..Vault::default()
            },
        );

        execute_trigger_handler(deps.as_mut(), env.clone(), vault.id, None).unwrap();

        let events = get_events_by_resource_id_handler(deps.as_ref(), vault.id, None, None, None)
            .unwrap()
            .events;

        assert_eq!(
            events.get(1).unwrap(),
            &Event {
                id: 2,
                resource_id: vault.id,
                timestamp: env.block.time,
                block_height: env.block.height,
                data: EventData::SimulatedDcaVaultExecutionSkipped {
                    reason: ExecutionSkippedReason::PriceThresholdExceeded {
                        price: Decimal::one()
                    },
                }
            }
        );
    }

    #[test]
    fn with_rwa_swap_adjustment_strategy_and_exceeded_slippage_tolerance_should_publish_execution_skipped_event(
    ) {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                slippage_tolerance: Decimal::percent(1),
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::default()),
                performance_assessment_strategy: Some(PerformanceAssessmentStrategy::default()),
                ..Vault::default()
            },
        );

        deps.querier.update_fin_price(&HALF_DECIMAL);

        execute_trigger_handler(deps.as_mut(), env.clone(), vault.id, None).unwrap();

        let events = get_events_by_resource_id_handler(deps.as_ref(), vault.id, None, None, None)
            .unwrap()
            .events;

        assert!(events.contains(&Event {
            id: 2,
            resource_id: vault.id,
            timestamp: env.block.time,
            block_height: env.block.height,
            data: EventData::SimulatedDcaVaultExecutionSkipped {
                reason: ExecutionSkippedReason::SlippageToleranceExceeded
            }
        }));
    }

    #[test]
    fn for_inactive_vault_with_active_performance_assessment_should_simulate_execution() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                status: VaultStatus::Inactive,
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::default()),
                balance: Coin::new(TEN.into(), DENOM_UKUJI),
                performance_assessment_strategy: Some(PerformanceAssessmentStrategy::default()),
                escrow_level: Decimal::percent(5),
                ..Vault::default()
            },
        );

        execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap();

        let updated_vault = get_vault(deps.as_ref().storage, vault.id).unwrap();

        let fee_rate = get_swap_fee_rate(deps.as_ref().storage, &vault.swap_adjustment_strategy)
            .unwrap()
            + get_automation_fee_rate(deps.as_ref().storage, &vault).unwrap();

        let received_amount_before_fee = vault.swap_amount * Decimal::percent(95);
        let fee_amount = received_amount_before_fee * fee_rate;
        let received_amount_after_fee = received_amount_before_fee - fee_amount;

        let performance_assessment_strategy =
            updated_vault.performance_assessment_strategy.unwrap();

        assert_eq!(
            match performance_assessment_strategy.clone() {
                PerformanceAssessmentStrategy::CompareToStandardDca { swapped_amount, .. } =>
                    swapped_amount,
            },
            Coin::new(vault.swap_amount.into(), vault.get_swap_denom()),
        );
        assert_eq!(
            match performance_assessment_strategy {
                PerformanceAssessmentStrategy::CompareToStandardDca {
                    received_amount, ..
                } => received_amount,
            },
            Coin::new(received_amount_after_fee.into(), vault.target_denom)
        );
    }

    #[test]
    fn for_inactive_vault_with_finished_performance_assessment_should_disburse_escrow() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                status: VaultStatus::Inactive,
                balance: Coin::new(0, DENOM_UKUJI),
                escrow_level: Decimal::percent(5),
                deposited_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                escrowed_amount: Coin::new(ONE.into(), DENOM_UUSK),
                performance_assessment_strategy: Some(
                    PerformanceAssessmentStrategy::CompareToStandardDca {
                        swapped_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                        received_amount: Coin::new(TEN.into(), DENOM_UUSK),
                    },
                ),
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::default()),
                ..Vault::default()
            },
        );

        deps.querier.update_fin_price(&HALF_DECIMAL);

        let response = execute_trigger_handler(deps.as_mut(), env.clone(), vault.id, None).unwrap();

        assert!(response.messages.contains(&SubMsg::new(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::DisburseEscrow { vault_id: vault.id }).unwrap(),
            funds: vec![],
        })));
    }

    #[test]
    fn for_inactive_vault_with_unfinished_performance_assessment_should_not_disburse_escrow() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                status: VaultStatus::Inactive,
                balance: Coin::new(0, DENOM_UKUJI),
                escrow_level: Decimal::percent(5),
                deposited_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                escrowed_amount: Coin::new(ONE_MICRON.into(), DENOM_UUSK),
                performance_assessment_strategy: Some(
                    PerformanceAssessmentStrategy::CompareToStandardDca {
                        swapped_amount: Coin::new(ONE.into(), DENOM_UKUJI),
                        received_amount: Coin::new(ONE.into(), DENOM_UUSK),
                    },
                ),
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::default()),
                ..Vault::default()
            },
        );

        let response = execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap();

        assert!(response.messages.is_empty());
    }

    #[test]
    fn for_active_vault_should_create_a_new_trigger() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(deps.as_mut(), env.clone(), Vault::default());

        execute_trigger_handler(deps.as_mut(), env.clone(), vault.id, None).unwrap();

        let updated_vault = get_vault(deps.as_ref().storage, vault.id).unwrap();

        let old_target_time = match vault.trigger.unwrap() {
            TriggerConfiguration::Time { target_time } => target_time,
            _ => panic!("wrong trigger type"),
        };

        let new_target_time = match updated_vault.trigger.unwrap() {
            TriggerConfiguration::Time { target_time } => target_time,
            _ => panic!("wrong trigger type"),
        };

        assert_eq!(old_target_time.seconds(), env.block.time.seconds());
        assert_eq!(
            new_target_time.seconds(),
            env.block.time.plus_seconds(24 * 60 * 60).seconds()
        );
    }

    #[test]
    fn for_scheduled_vault_should_create_a_new_trigger() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                status: VaultStatus::Scheduled,
                ..Vault::default()
            },
        );

        execute_trigger_handler(deps.as_mut(), env.clone(), vault.id, None).unwrap();

        let updated_vault = get_vault(deps.as_ref().storage, vault.id).unwrap();

        let old_target_time = match vault.trigger.unwrap() {
            TriggerConfiguration::Time { target_time } => target_time,
            _ => panic!("wrong trigger type"),
        };

        let new_target_time = match updated_vault.trigger.unwrap() {
            TriggerConfiguration::Time { target_time } => target_time,
            _ => panic!("wrong trigger type"),
        };

        assert_eq!(old_target_time.seconds(), env.block.time.seconds());
        assert_eq!(
            new_target_time.seconds(),
            env.block.time.plus_seconds(24 * 60 * 60).seconds()
        );
    }

    #[test]
    fn for_inactive_vault_should_not_create_a_new_trigger() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                status: VaultStatus::Inactive,
                ..Vault::default()
            },
        );

        execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap();

        let updated_vault = get_vault(deps.as_ref().storage, vault.id).unwrap();

        assert!(vault.trigger.is_some());
        assert!(updated_vault.trigger.is_none(),);
    }

    #[test]
    fn for_inactive_vault_with_active_performance_assessment_should_create_a_new_trigger() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                status: VaultStatus::Inactive,
                escrow_level: Decimal::percent(5),
                deposited_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                escrowed_amount: Coin::new(ONE_MICRON.into(), DENOM_UUSK),
                performance_assessment_strategy: Some(
                    PerformanceAssessmentStrategy::CompareToStandardDca {
                        swapped_amount: Coin::new(ONE.into(), DENOM_UKUJI),
                        received_amount: Coin::new(ONE.into(), DENOM_UUSK),
                    },
                ),
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::default()),
                ..Vault::default()
            },
        );

        execute_trigger_handler(deps.as_mut(), env.clone(), vault.id, None).unwrap();

        let updated_vault = get_vault(deps.as_ref().storage, vault.id).unwrap();

        let old_target_time = match vault.trigger.unwrap() {
            TriggerConfiguration::Time { target_time } => target_time,
            _ => panic!("wrong trigger type"),
        };

        let new_target_time = match updated_vault.trigger.unwrap() {
            TriggerConfiguration::Time { target_time } => target_time,
            _ => panic!("wrong trigger type"),
        };

        assert_eq!(old_target_time.seconds(), env.block.time.seconds());
        assert_eq!(
            new_target_time.seconds(),
            env.block.time.plus_seconds(24 * 60 * 60).seconds()
        );
    }

    #[test]
    fn for_inactive_vault_with_finished_performance_assessment_should_not_create_a_new_trigger() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                status: VaultStatus::Inactive,
                escrow_level: Decimal::percent(5),
                deposited_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                escrowed_amount: Coin::new((TEN * Decimal::percent(5)).into(), DENOM_UUSK),
                performance_assessment_strategy: Some(
                    PerformanceAssessmentStrategy::CompareToStandardDca {
                        swapped_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                        received_amount: Coin::new(TEN.into(), DENOM_UUSK),
                    },
                ),
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::default()),
                ..Vault::default()
            },
        );

        execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap();

        let updated_vault = get_vault(deps.as_ref().storage, vault.id).unwrap();

        assert!(vault.trigger.is_some());
        assert!(updated_vault.trigger.is_none());
    }

    #[test]
    fn should_create_swap_message() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(deps.as_mut(), env.clone(), Vault::default());

        let response = execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap();

        let config = get_config(deps.as_ref().storage).unwrap();

        assert_eq!(
            response.messages.first().unwrap(),
            &SubMsg::reply_always(
                WasmMsg::Execute {
                    contract_addr: config.exchange_contract_address.to_string(),
                    msg: to_json_binary(&ExchangeExecuteMsg::Swap {
                        minimum_receive_amount: Coin {
                            amount: vault.minimum_receive_amount.unwrap_or(Uint128::zero()),
                            denom: vault.target_denom.clone(),
                        },
                        route: vault.route.clone()
                    })
                    .unwrap(),
                    funds: vec![Coin::new(vault.swap_amount.into(), vault.get_swap_denom())]
                },
                AFTER_SWAP_REPLY_ID,
            )
        )
    }

    #[test]
    fn should_create_reduced_swap_message_when_balance_is_low() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                balance: Coin::new((ONE / TWO_MICRONS).into(), DENOM_UKUJI),
                swap_amount: ONE,
                ..Vault::default()
            },
        );

        let response = execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap();

        let config = get_config(deps.as_ref().storage).unwrap();

        assert_eq!(
            response.messages.first().unwrap(),
            &SubMsg::reply_always(
                WasmMsg::Execute {
                    contract_addr: config.exchange_contract_address.to_string(),
                    msg: to_json_binary(&ExchangeExecuteMsg::Swap {
                        minimum_receive_amount: Coin {
                            amount: vault.minimum_receive_amount.unwrap_or(Uint128::zero()),
                            denom: vault.target_denom.clone(),
                        },
                        route: vault.route
                    })
                    .unwrap(),
                    funds: vec![vault.balance]
                },
                AFTER_SWAP_REPLY_ID,
            )
        );
    }

    #[test]
    fn should_create_swap_message_with_target_receive_amount_when_slippage_tolerance_set() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                slippage_tolerance: Decimal::percent(6),
                ..Vault::default()
            },
        );

        let response = execute_trigger_handler(deps.as_mut(), env.clone(), vault.id, None).unwrap();

        let config = get_config(deps.as_ref().storage).unwrap();

        assert_eq!(
            response.messages.first().unwrap(),
            &SubMsg::reply_always(
                WasmMsg::Execute {
                    contract_addr: config.exchange_contract_address.to_string(),
                    msg: to_json_binary(&ExchangeExecuteMsg::Swap {
                        minimum_receive_amount: Coin {
                            amount: vault.minimum_receive_amount.unwrap_or(Uint128::zero()),
                            denom: vault.target_denom.clone(),
                        },
                        route: vault.route.clone()
                    })
                    .unwrap(),
                    funds: vec![get_swap_amount(&deps.as_ref(), &env, &vault).unwrap()]
                },
                AFTER_SWAP_REPLY_ID,
            )
        );
    }

    #[test]
    fn should_skip_execution_if_price_threshold_exceeded() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                swap_amount: ONE,
                minimum_receive_amount: Some(ONE + ONE),
                ..Vault::default()
            },
        );

        execute_trigger_handler(deps.as_mut(), env.clone(), vault.id, None).unwrap();

        let events = get_events_by_resource_id_handler(deps.as_ref(), vault.id, None, None, None)
            .unwrap()
            .events;

        assert_eq!(
            events.get(1).unwrap(),
            &Event {
                id: 2,
                resource_id: vault.id,
                timestamp: env.block.time,
                block_height: env.block.height,
                data: EventData::DcaVaultExecutionSkipped {
                    reason: ExecutionSkippedReason::PriceThresholdExceeded {
                        price: Decimal::one()
                    }
                }
            }
        );
    }

    #[test]
    fn should_create_new_trigger_if_price_threshold_exceeded() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                swap_amount: ONE,
                minimum_receive_amount: Some(ONE + ONE),
                ..Vault::default()
            },
        );

        execute_trigger_handler(deps.as_mut(), env.clone(), vault.id, None).unwrap();

        let updated_vault = get_vault(deps.as_ref().storage, vault.id).unwrap();

        let old_target_time = match vault.trigger.unwrap() {
            TriggerConfiguration::Time { target_time } => target_time,
            _ => panic!("wrong trigger type"),
        };

        let new_target_time = match updated_vault.trigger.unwrap() {
            TriggerConfiguration::Time { target_time } => target_time,
            _ => panic!("wrong trigger type"),
        };

        assert_eq!(old_target_time.seconds(), env.block.time.seconds());
        assert_eq!(
            new_target_time.seconds(),
            env.block.time.plus_seconds(24 * 60 * 60).seconds()
        );
    }

    #[test]
    fn should_trigger_execution_if_price_threshold_not_exceeded() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let vault = setup_vault(
            deps.as_mut(),
            env.clone(),
            Vault {
                swap_amount: ONE,
                minimum_receive_amount: Some(ONE / Uint128::new(2)),
                ..Vault::default()
            },
        );

        let response = execute_trigger_handler(deps.as_mut(), env, vault.id, None).unwrap();

        let config = get_config(deps.as_ref().storage).unwrap();

        assert_eq!(
            response.messages.first().unwrap(),
            &SubMsg::reply_always(
                WasmMsg::Execute {
                    contract_addr: config.exchange_contract_address.to_string(),
                    msg: to_json_binary(&ExchangeExecuteMsg::Swap {
                        minimum_receive_amount: Coin {
                            amount: vault.minimum_receive_amount.unwrap_or(Uint128::zero()),
                            denom: vault.target_denom.clone(),
                        },
                        route: vault.route.clone()
                    })
                    .unwrap(),
                    funds: vec![Coin::new(vault.swap_amount.into(), vault.get_swap_denom())]
                },
                AFTER_SWAP_REPLY_ID,
            )
        );
    }
}
