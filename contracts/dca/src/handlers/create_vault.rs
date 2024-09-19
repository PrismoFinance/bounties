use crate::constants::{AFTER_LIMIT_ORDER_PLACED_REPLY_ID, TWO_MICRONS};
use crate::error::ContractError;
use crate::helpers::message::get_attribute_in_event;
use crate::helpers::validation::{
    assert_address_is_valid, assert_contract_destination_callbacks_are_valid,
    assert_contract_is_not_paused, assert_destination_allocations_add_up_to_one,
    assert_destination_callback_addresses_are_valid, assert_destinations_limit_is_not_breached,
    assert_exactly_one_asset, assert_label_is_no_longer_than_100_characters,
    assert_no_destination_allocations_are_zero, assert_route_exists_for_denoms,
    assert_slippage_tolerance_is_less_than_or_equal_to_one,
    assert_swap_adjustment_and_performance_assessment_strategies_are_compatible,
    assert_swap_adjustment_strategy_params_are_valid, assert_target_start_time_is_not_in_the_past,
    assert_time_interval_is_valid, assert_weighted_scale_multiplier_is_no_more_than_10,
};
use crate::helpers::vault::get_risk_weighted_average_model_id;
use crate::msg::ExecuteMsg;
use crate::state::cache::BOUNTY_ID_CACHE;
use crate::state::config::get_config;
use crate::state::events::create_event;
use crate::state::triggers::save_trigger;
use crate::state::vaults::{save_bounty, update_bounty};
use crate::types::destination::Destination;
use crate::types::event::{EventBuilder, EventData};
use crate::types::performance_assessment_strategy::{
    PerformanceAssessmentStrategy, PerformanceAssessmentStrategyParams,
};
use crate::types::swap_adjustment_strategy::{
    SwapAdjustmentStrategy, SwapAdjustmentStrategyParams,
};
use crate::types::time_interval::TimeInterval;
use crate::types::trigger::{Trigger, TriggerConfiguration};
use crate::types::bounty::{Bounty, BountyBuilder, BountyStatus};
use cosmwasm_std::{to_json_binary, Addr, Binary, Coin, Decimal, Reply, SubMsg, WasmMsg};
use cosmwasm_std::{DepsMut, Env, MessageInfo, Response, Timestamp, Uint128, Uint64};
use exchange::msg::ExecuteMsg as ExchangeExecuteMsg;

pub fn create_bounty_handler(
    // Edit
    deps: DepsMut,
    env: Env,
    info: &MessageInfo,
    owner: Addr,
    label: Option<String>,
    bounty_description: Option<String>,
    mut destinations: Vec<Destination>,
    target_denom: String,
    route: Option<Binary>,
    slippage_tolerance: Option<Decimal>,
    minimum_receive_amount: Option<Uint128>,
    time_interval: TimeInterval,
    target_start_time_utc_seconds: Option<Uint64>,
    target_receive_amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    assert_contract_is_not_paused(deps.storage)?;
    assert_address_is_valid(deps.as_ref(), &owner, "owner")?;
    assert_exactly_one_asset(info.funds.clone())?;
    assert_destinations_limit_is_not_breached(&destinations)?;
    assert_time_interval_is_valid(&time_interval)?;

    assert_route_exists_for_denoms(
        deps.as_ref(),
        info.funds[0].denom.clone(),
        target_denom.clone(),
        route.clone(),
    )?;

    assert_swap_adjustment_and_performance_assessment_strategies_are_compatible(
        &swap_adjustment_strategy_params,
        &performance_assessment_strategy_params,
    )?;

    if let Some(label) = label.clone() {
        assert_label_is_no_longer_than_100_characters(&label)?;
    }

    if let Some(swap_adjustment_strategy_params) = &swap_adjustment_strategy_params {
        assert_swap_adjustment_strategy_params_are_valid(swap_adjustment_strategy_params)?;
    }

    if let Some(slippage_tolerance) = slippage_tolerance {
        assert_slippage_tolerance_is_less_than_or_equal_to_one(slippage_tolerance)?;
    }

    if let Some(target_time) = target_start_time_utc_seconds {
        assert_target_start_time_is_not_in_the_past(
            env.block.time,
            Timestamp::from_seconds(target_time.u64()),
        )?;
    }

    if destinations.is_empty() {
        destinations.push(Destination {
            allocation: Decimal::percent(100),
            address: owner.clone(),
            msg: None,
        });
    }

    assert_destination_callback_addresses_are_valid(deps.as_ref(), &destinations)?;
    assert_contract_destination_callbacks_are_valid(&destinations, &env.contract.address)?;
    assert_no_destination_allocations_are_zero(&destinations)?;
    assert_destination_allocations_add_up_to_one(&destinations)?;

    let config = get_config(deps.storage)?;

    let swap_denom = info.funds[0].denom.clone();

    let swap_adjustment_strategy = match swap_adjustment_strategy_params {
        Some(params) => Some(match params {
            SwapAdjustmentStrategyParams::RiskWeightedAverage {
                base_denom,
                position_type,
            } => SwapAdjustmentStrategy::RiskWeightedAverage {
                model_id: get_risk_weighted_average_model_id(
                    &env.block.time,
                    &info.funds[0],
                    &swap_amount,
                    &time_interval,
                ),
                base_denom,
                position_type,
            },
            SwapAdjustmentStrategyParams::WeightedScale {
                base_receive_amount,
                multiplier,
                increase_only,
            } => {
                assert_weighted_scale_multiplier_is_no_more_than_10(multiplier)?;
                SwapAdjustmentStrategy::WeightedScale {
                    base_receive_amount,
                    multiplier,
                    increase_only,
                }
            }
        }),
        None => None,
    };

    let performance_assessment_strategy = match performance_assessment_strategy_params {
        Some(PerformanceAssessmentStrategyParams::CompareToStandardDca) => {
            Some(PerformanceAssessmentStrategy::CompareToStandardDca {
                swapped_amount: Coin::new(0, swap_denom.clone()),
                received_amount: Coin::new(0, target_denom.clone()),
            })
        }
        _ => None,
    };

    let escrow_level = performance_assessment_strategy
        .clone()
        .map_or(Decimal::zero(), |_| {
            config.risk_weighted_average_escrow_level
        });

    let bounty_builder = BountyBuilder {
        owner,
        label,
        destinations,
        created_at: env.block.time,
        status: BountyStatus::Scheduled,
        target_denom: target_denom.clone(),
        route,
        slippage_tolerance: slippage_tolerance.unwrap_or(config.default_slippage_tolerance),
        minimum_receive_amount,
        balance: info.funds[0].clone(),
        time_interval,
        started_at: None,
        escrow_level,
        deposited_amount: info.funds[0].clone(),
        received_amount: Coin::new(0, target_denom.clone()),
        escrowed_amount: Coin::new(0, target_denom),
    };

    let bounty = save_bounty(deps.storage, bounty_builder)?;

    BOUNTY_ID_CACHE.save(deps.storage, &bounty.id)?;

    create_event(
        deps.storage,
        EventBuilder::new(
            bounty.id,
            env.block.clone(),
            EventData::BountyFundsDeposited {
                amount: Coin::new(
                    (info.funds[0].amount
                        - if target_receive_amount.is_some() {
                            TWO_MICRONS
                        } else {
                            Uint128::zero()
                        })
                    .into(),
                    info.funds[0].denom.clone(),
                ),
            },
        ),
    )?;

    let mut response = Response::new()
        .add_attribute("create_bounty", "true")
        .add_attribute("bounty_id", bounty.id)
        .add_attribute("owner", bounty.owner.clone())
        .add_attribute("deposited_amount", bounty.balance.to_string());

    match (target_start_time_utc_seconds, target_receive_amount) {
        (None, None) | (Some(_), None) => {
            save_trigger(
                deps.storage,
                Trigger {
                    bounty_id: bounty.id,
                    configuration: TriggerConfiguration::Time {
                        target_time: match target_start_time_utc_seconds {
                            Some(time) => Timestamp::from_seconds(time.u64()),
                            None => env.block.time,
                        },
                    },
                },
            )?;

            if target_start_time_utc_seconds.is_none() {
                response = response.add_submessage(SubMsg::new(WasmMsg::Execute {
                    contract_addr: env.contract.address.to_string(),
                    msg: to_json_binary(&ExecuteMsg::ExecuteTrigger {
                        trigger_id: bounty.id,
                        route: bounty.route,
                    })
                    .unwrap(),
                    funds: vec![],
                }));
            }

            Ok(response)
        }
        (None, Some(target_receive_amount)) => {
            let bounty= update_bounty(
                deps.storage,
                Bounty {
                    deposited_amount: Coin::new(
                        (bounty.deposited_amount.amount - TWO_MICRONS).into(),
                        bounty.deposited_amount.denom,
                    ),
                    balance: Coin::new(
                        (bounty.balance.amount - TWO_MICRONS).into(),
                        bounty.balance.denom,
                    ),
                    ..bounty
                },
            )?;

            let target_price = Decimal::from_ratio(swap_amount, target_receive_amount);

            Ok(response.add_submessage(SubMsg::reply_on_success(
                WasmMsg::Execute {
                    contract_addr: config.exchange_contract_address.to_string(),
                    msg: to_json_binary(&ExchangeExecuteMsg::SubmitOrder {
                        target_price: target_price.into(),
                        target_denom: bounty.target_denom.clone(),
                    })
                    .unwrap(),
                    funds: vec![Coin::new(TWO_MICRONS.into(), bounty.get_swap_denom())],
                },
                AFTER_LIMIT_ORDER_PLACED_REPLY_ID,
            )))
        }
        (Some(_), Some(_)) => Err(ContractError::CustomError {
            val: String::from(
                "cannot provide both a target_start_time_utc_seconds and a target_price",
            ),
        }),
    }
}

pub fn save_price_trigger(deps: DepsMut, reply: Reply) -> Result<Response, ContractError> {
    let submit_order_response = reply.result.into_result().unwrap();

    let order_idx = get_attribute_in_event(&submit_order_response.events, "wasm", "order_idx")?
        .parse::<Uint128>()
        .expect("the order id of the submitted order");

    let target_price =
        get_attribute_in_event(&submit_order_response.events, "wasm", "target_price")?
            .parse::<Decimal>()
            .expect("the target price of the submitted order");

    let bounty_id = BOUNTY_ID_CACHE.load(deps.storage)?;

    save_trigger(
        deps.storage,
        Trigger {
            vault_id,
            configuration: TriggerConfiguration::Price {
                order_idx,
                target_price,
            },
        },
    )?;

    Ok(Response::new()
        .add_attribute("save_price_trigger", "true")
        .add_attribute("order_idx", order_idx))
}

#[cfg(test)]
mod create_bounty_tests {
    use super::*;
    use crate::constants::{ONE, TEN};
    use crate::handlers::get_events_by_resource_id::get_events_by_resource_id_handler;
    use crate::handlers::get_vault::get_vault_handler;
    use crate::msg::ExecuteMsg;
    use crate::state::config::{get_config, update_config};
    use crate::tests::helpers::instantiate_contract;
    use crate::tests::mocks::{
        calc_mock_dependencies, ADMIN, DENOM_UKUJI, DENOM_UUSK, USER, VALIDATOR,
    };
    use crate::types::config::Config;
    use crate::types::destination::Destination;
    use crate::types::event::{EventBuilder, EventData};
    use crate::types::swap_adjustment_strategy::SwapAdjustmentStrategy;
    use crate::types::time_interval::TimeInterval;
    use crate::types::trigger::TriggerConfiguration;
    use crate::types::vault::{Bounty, BountyStatus};
    use cosmwasm_std::testing::{mock_env, mock_info};
    use cosmwasm_std::{
        to_json_binary, Addr, Coin, ContractResult, Decimal, Decimal256, SubMsg, SystemResult,
        Timestamp, Uint128, WasmMsg,
    };
    use exchange::msg::Pair;

    #[test]
    fn with_no_assets_fails() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(USER, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let err = create_bounty_handler(
            deps.as_mut(),
            env,
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(10000),
            TimeInterval::Daily,
            None,
            None,
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error: received 0 denoms but required exactly 1"
        );
    }

    #[test]
    fn with_multiple_assets_fails() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(
            USER,
            &[Coin::new(10000, DENOM_UKUJI), Coin::new(10000, DENOM_UUSK)],
        );

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let err = create_bounty_handler(
            deps.as_mut(),
            env,
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(10000),
            TimeInterval::Daily,
            None,
            None,
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error: received 2 denoms but required exactly 1"
        );
    }

    #[test]
    fn with_non_existent_pair_fails() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(USER, &[Coin::new(10000, DENOM_UUSK)]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary::<Vec<Pair>>(&vec![]).unwrap(),
            ))
        });

        let err = create_bounty_handler(
            deps.as_mut(),
            env,
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            None,
            None,
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            format!(
                "Error: swapping {} to {} not supported",
                DENOM_UUSK, DENOM_UKUJI
            )
        );
    }

    #[test]
    fn with_destination_allocations_less_than_100_percent_fails() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let admin_info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), admin_info);

        let user_info = mock_info(USER, &[Coin::new(10000, DENOM_UUSK)]);

        let err = create_bounty_handler(
            deps.as_mut(),
            env,
            &user_info,
            user_info.sender.clone(),
            None,
            vec![Destination {
                allocation: Decimal::percent(50),
                address: Addr::unchecked(USER),
                msg: None,
            }],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            None,
            None,
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error: destination allocations must add up to 1"
        );
    }

    #[test]
    fn with_destination_allocation_equal_to_zero_fails() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let admin_info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), admin_info);

        let user_info = mock_info(USER, &[Coin::new(10000, DENOM_UUSK)]);

        let err = create_bounty_handler(
            deps.as_mut(),
            env,
            &user_info,
            user_info.sender.clone(),
            None,
            vec![
                Destination {
                    allocation: Decimal::percent(100),
                    address: Addr::unchecked(USER),
                    msg: None,
                },
                Destination {
                    allocation: Decimal::percent(0),
                    address: Addr::unchecked("other"),
                    msg: None,
                },
            ],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            None,
            None,
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error: all destination allocations must be greater than 0"
        );
    }

    #[test]
    fn with_more_than_10_destination_allocations_fails() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(USER, &[Coin::new(10000, DENOM_UUSK)]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let err = create_bounty_handler(
            deps.as_mut(),
            env,
            &info,
            info.sender.clone(),
            None,
            (0..20)
                .map(|i| Destination {
                    allocation: Decimal::percent(5),
                    address: Addr::unchecked(format!("destination-{}", i)),
                    msg: None,
                })
                .collect(),
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            None,
            None,
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error: no more than 10 destinations can be provided"
        );
    }

    #[test]
    fn with_too_high_weighted_scale_multiplier_fails() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let admin_info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), admin_info);

        let user_info = mock_info(USER, &[Coin::new(10000, DENOM_UUSK)]);

        let err = create_bounty_handler(
            deps.as_mut(),
            env,
            &user_info,
            user_info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            None,
            None,
            None,
            Some(SwapAdjustmentStrategyParams::WeightedScale {
                base_receive_amount: Uint128::new(100000),
                multiplier: Decimal::percent(1100),
                increase_only: false,
            }),
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error: Cannot set weighted scale multiplier to more than 10"
        );
    }

    #[test]
    fn when_contract_is_paused_fails() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(USER, &[Coin::new(10000, DENOM_UUSK)]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let config = get_config(deps.as_ref().storage).unwrap();

        update_config(
            deps.as_mut().storage,
            Config {
                paused: true,
                ..config
            },
        )
        .unwrap();

        let err = create_bounty_handler(
            deps.as_mut(),
            env,
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            None,
            None,
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "Error: contract is paused")
    }

    #[test]
    fn with_time_trigger_with_target_time_in_the_past_fails() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let admin_info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), admin_info);

        let user_info = mock_info(USER, &[Coin::new(10000, DENOM_UUSK)]);

        let err = create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &user_info,
            user_info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            Some(env.block.time.minus_seconds(10).seconds().into()),
            None,
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error: target_start_time_utc_seconds must be some time in the future"
        );
    }

    #[test]
    fn with_invalid_custom_time_interval_fails() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(USER, &[Coin::new(10000, DENOM_UUSK)]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let err = create_bounty_handler(
            deps.as_mut(),
            env,
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Custom { seconds: 23 },
            None,
            None,
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error: custom time interval must be at least 60 seconds"
        );
    }

    #[test]
    fn with_both_target_time_and_target_price_fails() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let pair = Pair::default();

        let err = create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &mock_info(ADMIN, &[Coin::new(1233123, pair.denoms[0].clone())]),
            info.sender,
            None,
            vec![],
            pair.denoms[1].clone(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            Some(env.block.time.seconds().into()),
            Some(Uint128::new(872316)),
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error: cannot provide both a target_start_time_utc_seconds and a target_price"
        );
    }

    #[test]
    fn with_no_swap_adjustment_strategy_and_performance_assessment_strategy_fails() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let swap_amount = Uint128::new(100000);
        info = mock_info(USER, &[Coin::new(100000, DENOM_UUSK)]);

        let err = create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            swap_amount,
            TimeInterval::Daily,
            Some(env.block.time.plus_seconds(10).seconds().into()),
            None,
            Some(PerformanceAssessmentStrategyParams::CompareToStandardDca),
            None,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error: incompatible swap adjustment and performance assessment strategies"
        );
    }

    #[test]
    fn with_swap_adjustment_strategy_and_no_performance_assessment_strategy_fails() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let swap_amount = Uint128::new(100000);
        info = mock_info(USER, &[Coin::new(100000, DENOM_UUSK)]);

        let err = create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            swap_amount,
            TimeInterval::Daily,
            Some(env.block.time.plus_seconds(10).seconds().into()),
            None,
            None,
            Some(SwapAdjustmentStrategyParams::default()),
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error: incompatible swap adjustment and performance assessment strategies"
        );
    }

    #[test]
    fn with_weighted_scale_multiplier_larger_than_10_fails() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let swap_amount = Uint128::new(100000);
        info = mock_info(USER, &[Coin::new(100000, DENOM_UUSK)]);

        let err = create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            swap_amount,
            TimeInterval::Daily,
            Some(env.block.time.plus_seconds(10).seconds().into()),
            None,
            None,
            Some(SwapAdjustmentStrategyParams::WeightedScale {
                base_receive_amount: Uint128::new(232231),
                multiplier: Decimal::percent(1001),
                increase_only: false,
            }),
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error: Cannot set weighted scale multiplier to more than 10"
        );
    }

    #[test]
    fn with_slippage_tolerance_larger_than_one_fails() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let swap_amount = Uint128::new(100000);
        info = mock_info(USER, &[Coin::new(100000, DENOM_UUSK)]);

        let err = create_bounty_handler(
            deps.as_mut(),
            env,
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            Some(Decimal::percent(150)),
            None,
            swap_amount,
            TimeInterval::Daily,
            None,
            None,
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error: slippage tolerance must be less than or equal to 1"
        );
    }

    #[test]
    fn should_create_vault_with_time_trigger() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let swap_amount = Uint128::new(100000);
        info = mock_info(USER, &[Coin::new(100000, DENOM_UUSK)]);

        create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            swap_amount,
            TimeInterval::Daily,
            Some(env.block.time.plus_seconds(10).seconds().into()),
            None,
            None,
            None,
        )
        .unwrap();

        let bounty = get_bounty_handler(deps.as_ref(), Uint128::one())
            .unwrap()
            .bounty;

        let config = get_config(deps.as_ref().storage).unwrap();

        assert_eq!(
            bounty,
            Bounty {
                minimum_receive_amount: None,
                label: None,
                id: Uint128::new(1),
                owner: info.sender,
                destinations: vec![Destination::default()],
                created_at: env.block.time,
                status: BountyStatus::Scheduled,
                time_interval: TimeInterval::Daily,
                balance: info.funds[0].clone(),
                slippage_tolerance: config.default_slippage_tolerance,
                route: None,
                swap_amount,
                target_denom: DENOM_UKUJI.to_string(),
                started_at: None,
                deposited_amount: info.funds[0].clone(),
                escrow_level: Decimal::zero(),
                swapped_amount: Coin::new(0, DENOM_UUSK.to_string()),
                received_amount: Coin::new(0, DENOM_UKUJI.to_string()),
                escrowed_amount: Coin::new(0, DENOM_UKUJI.to_string()),
                swap_adjustment_strategy: None,
                performance_assessment_strategy: None,
                trigger: Some(TriggerConfiguration::Time {
                    target_time: Timestamp::from_seconds(env.block.time.plus_seconds(10).seconds()),
                }),
            }
        );
    }

    #[test]
    fn should_create_bounty_with_pending_price_trigger() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let swap_amount = ONE;
        info = mock_info(USER, &[Coin::new(TEN.into(), DENOM_UUSK)]);

        create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            swap_amount,
            TimeInterval::Daily,
            None,
            Some(ONE / TWO_MICRONS),
            None,
            None,
        )
        .unwrap();

        let bounty = get_bounty_handler(deps.as_ref(), Uint128::one())
            .unwrap()
            .bounty;

        let config = get_config(deps.as_ref().storage).unwrap();

        assert_eq!(
            bounty,
            Bounty {
                minimum_receive_amount: None,
                label: None,
                id: Uint128::new(1),
                owner: info.sender,
                destinations: vec![Destination::default()],
                created_at: env.block.time,
                status: BountyStatus::Scheduled,
                time_interval: TimeInterval::Daily,
                balance: Coin::new(
                    (info.funds[0].amount - TWO_MICRONS).into(),
                    info.funds[0].denom.clone()
                ),
                slippage_tolerance: config.default_slippage_tolerance,
                swap_amount,
                target_denom: DENOM_UKUJI.to_string(),
                route: None,
                started_at: None,
                deposited_amount: Coin::new(
                    (info.funds[0].amount - TWO_MICRONS).into(),
                    info.funds[0].denom.clone()
                ),
                escrow_level: Decimal::zero(),
                swapped_amount: Coin::new(0, DENOM_UUSK.to_string()),
                received_amount: Coin::new(0, DENOM_UKUJI.to_string()),
                escrowed_amount: Coin::new(0, DENOM_UKUJI.to_string()),
                swap_adjustment_strategy: None,
                performance_assessment_strategy: None,
                trigger: None,
            }
        );
    }

    #[test]
    fn should_publish_deposit_event() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        info = mock_info(USER, &[Coin::new(100000, DENOM_UUSK)]);

        create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            Some(env.block.time.plus_seconds(10).seconds().into()),
            None,
            None,
            None,
        )
        .unwrap();

        let events =
            get_events_by_resource_id_handler(deps.as_ref(), Uint128::one(), None, None, None)
                .unwrap()
                .events;

        assert!(events.contains(
            &EventBuilder::new(
                Uint128::one(),
                env.block,
                EventData::BountyFundsDeposited {
                    amount: info.funds[0].clone()
                },
            )
            .build(1),
        ))
    }

    #[test]
    fn for_different_owner_should_succeed() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let owner = Addr::unchecked(USER);
        info = mock_info(ADMIN, &[Coin::new(100000, DENOM_UUSK)]);

        create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &info,
            owner,
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            Some(env.block.time.plus_seconds(10).seconds().into()),
            None,
            None,
            None,
        )
        .unwrap();

        let bounty = get_bounty_handler(deps.as_ref(), Uint128::one())
            .unwrap()
            .bounty;

        assert_eq!(bounty.owner, Addr::unchecked(USER));
    }

    #[test]
    fn with_multiple_destinations_should_succeed() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        info = mock_info(USER, &[Coin::new(100000, DENOM_UUSK)]);

        let destinations = vec![
            Destination {
                allocation: Decimal::percent(50),
                address: env.contract.address.clone(),
                msg: Some(
                    to_json_binary(&ExecuteMsg::ZDelegate {
                        delegator_address: Addr::unchecked("dest-1"),
                        validator_address: Addr::unchecked(VALIDATOR),
                    })
                    .unwrap(),
                ),
            },
            Destination {
                allocation: Decimal::percent(50),
                address: env.contract.address.clone(),
                msg: Some(
                    to_json_binary(&ExecuteMsg::ZDelegate {
                        delegator_address: Addr::unchecked("dest-2"),
                        validator_address: Addr::unchecked(VALIDATOR),
                    })
                    .unwrap(),
                ),
            },
        ];

        create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &info,
            info.sender.clone(),
            None,
            destinations.clone(),
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            Some(env.block.time.plus_seconds(10).seconds().into()),
            None,
            None,
            None,
        )
        .unwrap();

        let bounty = get_bounty_handler(deps.as_ref(), Uint128::one())
            .unwrap()
            .bounty;

        assert_eq!(bounty.destinations, destinations);
    }

    #[test]
    fn should_create_swap_adjustment_strategy() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        info = mock_info(USER, &[Coin::new(100000, DENOM_UUSK)]);

        create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            Some(env.block.time.plus_seconds(10).seconds().into()),
            None,
            Some(PerformanceAssessmentStrategyParams::CompareToStandardDca),
            Some(SwapAdjustmentStrategyParams::default()),
        )
        .unwrap();

        let bounty = get_bounty_handler(deps.as_ref(), Uint128::one())
            .unwrap()
            .bounty;

        assert_eq!(
            bounty.swap_adjustment_strategy,
            Some(SwapAdjustmentStrategy::default())
        );
    }

    #[test]
    fn should_save_no_performance_assessment_strategy_when_none_provided() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        info = mock_info(USER, &[Coin::new(100000, DENOM_UUSK)]);

        create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            Some(env.block.time.plus_seconds(10).seconds().into()),
            None,
            None,
            Some(SwapAdjustmentStrategyParams::WeightedScale {
                base_receive_amount: Uint128::new(100000),
                multiplier: Decimal::percent(200),
                increase_only: false,
            }),
        )
        .unwrap();

        let bounty = get_bounty_handler(deps.as_ref(), Uint128::one())
            .unwrap()
            .bounty;

        assert_eq!(bounty.performance_assessment_strategy, None);
    }

    #[test]
    fn should_create_performance_assessment_strategy() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        info = mock_info(USER, &[Coin::new(100000, DENOM_UUSK)]);

        create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            Some(env.block.time.plus_seconds(10).seconds().into()),
            None,
            Some(PerformanceAssessmentStrategyParams::CompareToStandardDca),
            Some(SwapAdjustmentStrategyParams::default()),
        )
        .unwrap();

        let bounty = get_bounty_handler(deps.as_ref(), Uint128::one())
            .unwrap()
            .bounty;

        assert_eq!(
            bounty.performance_assessment_strategy,
            Some(PerformanceAssessmentStrategy::CompareToStandardDca {
                swapped_amount: Coin::new(0, bounty.balance.denom),
                received_amount: Coin::new(0, DENOM_UKUJI),
            })
        );
    }

    #[test]
    fn with_large_deposit_should_select_longer_duration_model() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        info = mock_info(USER, &[Coin::new(1000000000, DENOM_UUSK)]);

        create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            Some(env.block.time.plus_seconds(10).seconds().into()),
            None,
            Some(PerformanceAssessmentStrategyParams::CompareToStandardDca),
            Some(SwapAdjustmentStrategyParams::default()),
        )
        .unwrap();

        let bounty = get_bounty_handler(deps.as_ref(), Uint128::one())
            .unwrap()
            .bounty;

        assert_eq!(
            bounty
                .swap_adjustment_strategy
                .map(|strategy| match strategy {
                    SwapAdjustmentStrategy::RiskWeightedAverage { model_id, .. } => model_id,
                    _ => panic!("unexpected swap adjustment strategy"),
                }),
            Some(90)
        );
    }

    #[test]
    fn with_no_target_time_should_execute_vault() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        info = mock_info(USER, &[Coin::new(100000, DENOM_UUSK)]);

        let response = create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            None,
            None,
            Some(PerformanceAssessmentStrategyParams::CompareToStandardDca),
            Some(SwapAdjustmentStrategyParams::default()),
        )
        .unwrap();

        assert_eq!(
            response.messages.first().unwrap(),
            &SubMsg::new(WasmMsg::Execute {
                contract_addr: env.contract.address.to_string(),
                funds: vec![],
                msg: to_json_binary(&ExecuteMsg::ExecuteTrigger {
                    trigger_id: Uint128::one(),
                    route: None,
                })
                .unwrap()
            })
        );
    }

    #[test]
    fn with_target_price_should_create_limit_order() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let pair = Pair::default();

        let swap_amount = ONE;
        info = mock_info(USER, &[Coin::new(TEN.into(), pair.denoms[1].clone())]);

        let response = create_bounty_handler(
            deps.as_mut(),
            env,
            &info,
            info.sender.clone(),
            None,
            vec![],
            pair.denoms[0].to_string(),
            None,
            None,
            None,
            swap_amount,
            TimeInterval::Daily,
            None,
            Some(ONE / TWO_MICRONS),
            None,
            None,
        )
        .unwrap();

        let config = get_config(deps.as_ref().storage).unwrap();

        assert_eq!(
            response.messages.first().unwrap(),
            &SubMsg::reply_on_success(
                WasmMsg::Execute {
                    contract_addr: config.exchange_contract_address.to_string(),
                    funds: vec![Coin::new(TWO_MICRONS.into(), info.funds[0].denom.clone())],
                    msg: to_json_binary(&ExchangeExecuteMsg::SubmitOrder {
                        target_price: Decimal256::percent(200),
                        target_denom: pair.denoms[0].clone(),
                    })
                    .unwrap()
                },
                AFTER_LIMIT_ORDER_PLACED_REPLY_ID
            )
        );
    }

    #[test]
    fn should_set_appropriate_escrow_level_for_compare_dca_performance_assessment_strategy() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        info = mock_info(USER, &[Coin::new(100000, DENOM_UUSK)]);

        create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            Some(env.block.time.plus_seconds(10).seconds().into()),
            None,
            Some(PerformanceAssessmentStrategyParams::CompareToStandardDca),
            Some(SwapAdjustmentStrategyParams::default()),
        )
        .unwrap();

        let bounty = get_bounty_handler(deps.as_ref(), Uint128::one())
            .unwrap()
            .vault;

        let config = get_config(deps.as_ref().storage).unwrap();

        assert_eq!(
            bounty.escrow_level,
            config.risk_weighted_average_escrow_level
        );
    }

    #[test]
    fn should_set_appropriate_escrow_level_for_no_performance_assessment_strategy() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        info = mock_info(USER, &[Coin::new(100000, DENOM_UUSK)]);

        create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &info,
            info.sender.clone(),
            None,
            vec![],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            Some(env.block.time.plus_seconds(10).seconds().into()),
            None,
            None,
            None,
        )
        .unwrap();

        let bounty = get_bounty_handler(deps.as_ref(), Uint128::one())
            .unwrap()
            .bounty;

        assert_eq!(bounty.escrow_level, Decimal::zero());
    }

    #[test]
    fn invoking_contract_callback_with_unauthorised_msg_fails() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        info = mock_info(USER, &[Coin::new(100000, DENOM_UUSK)]);

        let err = create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &info,
            info.sender.clone(),
            None,
            vec![Destination {
                address: env.contract.address,
                allocation: Decimal::percent(100),
                msg: Some(
                    to_json_binary(&ExecuteMsg::DisburseEscrow {
                        bounty_id: Uint128::one(),
                    })
                    .unwrap(),
                ),
            }],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            Some(env.block.time.plus_seconds(10).seconds().into()),
            None,
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error: Cannot invoke provided destination callback against the DCA contract"
        );
    }

    #[test]
    fn invoking_contract_callback_with_authorised_msg_succeeds() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let mut info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        info = mock_info(USER, &[Coin::new(100000, DENOM_UUSK)]);

        create_bounty_handler(
            deps.as_mut(),
            env.clone(),
            &info,
            info.sender.clone(),
            None,
            vec![Destination {
                address: env.contract.address.clone(),
                allocation: Decimal::percent(100),
                msg: Some(
                    to_json_binary(&ExecuteMsg::Deposit {
                        address: Addr::unchecked(USER),
                        bounty_id: Uint128::one(),
                    })
                    .unwrap(),
                ),
            }],
            DENOM_UKUJI.to_string(),
            None,
            None,
            None,
            Uint128::new(100000),
            TimeInterval::Daily,
            Some(env.block.time.plus_seconds(10).seconds().into()),
            None,
            None,
            None,
        )
        .unwrap();

        let bounty = get_bounty_handler(deps.as_ref(), Uint128::one())
            .unwrap()
            .bounty;

        assert_eq!(
            bounty.destinations,
            vec![Destination {
                address: env.contract.address,
                allocation: Decimal::percent(100),
                msg: Some(
                    to_json_binary(&ExecuteMsg::Deposit {
                        address: Addr::unchecked(USER),
                        bounty_id: Uint128::one(),
                    })
                    .unwrap(),
                ),
            }]
        );
    }
}

#[cfg(test)]
mod save_limit_order_id_tests {
    use super::save_price_trigger;
    use crate::{
        state::{cache::BOUNTY_ID_CACHE, triggers::get_trigger},
        types::trigger::{Trigger, TriggerConfiguration},
    };
    use cosmwasm_std::{
        testing::mock_dependencies, Decimal, Event, Reply, SubMsgResponse, SubMsgResult, Uint128,
    };

    #[test]
    fn should_save_limit_order_id() {
        let mut deps = mock_dependencies();

        let bounty_id = Uint128::one();
        let order_idx = Uint128::new(67);

        BOUNTY_ID_CACHE
            .save(deps.as_mut().storage, &bounty_id)
            .unwrap();

        let reply = Reply {
            id: 1,
            result: SubMsgResult::Ok(SubMsgResponse {
                events: vec![Event::new("wasm")
                    .add_attribute("order_idx", order_idx.to_string())
                    .add_attribute("target_price", Decimal::percent(200).to_string())],
                data: None,
            }),
        };

        save_price_trigger(deps.as_mut(), reply).unwrap();

        let trigger = get_trigger(deps.as_ref().storage, bounty_id).unwrap();

        assert_eq!(
            trigger,
            Some(Trigger {
                bounty_id,
                configuration: TriggerConfiguration::Price {
                    target_price: Decimal::percent(200),
                    order_idx,
                },
            })
        );
    }
}
