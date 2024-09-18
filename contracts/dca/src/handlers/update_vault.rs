use crate::{
    error::ContractError,
    helpers::{
        time::get_next_target_time,
        validation::{
            assert_destination_allocations_add_up_to_one,
            assert_destination_callback_addresses_are_valid,
            assert_destinations_limit_is_not_breached,
            assert_label_is_no_longer_than_100_characters,
            assert_no_destination_allocations_are_zero,
            assert_slippage_tolerance_is_less_than_or_equal_to_one, assert_time_interval_is_valid,
            assert_vault_is_not_cancelled, assert_weighted_scale_multiplier_is_no_more_than_10,
            asset_sender_is_vault_owner,
        },
    },
    state::{
        events::create_event,
        triggers::{delete_trigger, save_trigger},
        bounties::{get_bounty, update_bounty},
    },
    types::{
        destination::Destination,
        event::{EventBuilder, EventData},
        swap_adjustment_strategy::{SwapAdjustmentStrategy, SwapAdjustmentStrategyParams},
        time_interval::TimeInterval,
        trigger::{Trigger, TriggerConfiguration},
        update::Update,
    },
};
use cosmwasm_std::{Decimal, DepsMut, Env, MessageInfo, Response, Uint128};

pub fn update_bounty_handler(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vault_id: Uint128,
    label: Option<String>,
    bounty_description: Option<String>,
    destinations: Option<Vec<Destination>>,
    slippage_tolerance: Option<Decimal>,
    minimum_receive_amount: Option<Uint128>,
    time_interval: Option<TimeInterval>,
    swap_adjustment_strategy: Option<SwapAdjustmentStrategyParams>,
    swap_amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    let mut bounty = get_bounty(deps.storage, bounty_id)?;

    asset_sender_is_bounty_owner(bounty.owner.clone(), info.sender)?;
    assert_bounty_is_not_cancelled(&bounty)?;

    let mut response = Response::default()
        .add_attribute("update_bounty", "true")
        .add_attribute("bounty_id", bounty.id)
        .add_attribute("owner", bounty.owner.clone());

    let mut updates = Vec::<Update>::new();

    if let Some(swap_amount) = swap_amount {
        if minimum_receive_amount.is_some() {
            return Err(ContractError::CustomError {
                val: "cannot update swap amount and minimum receive amount at the same time."
                    .to_string(),
            });
        }

        if swap_adjustment_strategy.is_some() {
            return Err(ContractError::CustomError {
                val: "cannot update swap amount and swap adjustment strategy at the same time."
                    .to_string(),
            });
        }

        if let Some(minimum_receive_amount) = bounty.minimum_receive_amount {
            let updated_minimum_receive_amount =
                Some(minimum_receive_amount * Decimal::from_ratio(swap_amount, bounty.swap_amount));

            updates.push(Update {
                field: "minimum_receive_amount".to_string(),
                old_value: format!("{:?}", bounty.minimum_receive_amount),
                new_value: format!("{:?}", updated_minimum_receive_amount),
            });

            bounty.minimum_receive_amount = updated_minimum_receive_amount;
            response = response.add_attribute(
                "minimum_receive_amount",
                format!("{:?}", bounty.minimum_receive_amount),
            );
        }

        if let Some(SwapAdjustmentStrategy::WeightedScale {
            base_receive_amount,
            multiplier,
            increase_only,
        }) = bounty.swap_adjustment_strategy
        {
            let updated_swap_adjustment_strategy = Some(SwapAdjustmentStrategy::WeightedScale {
                base_receive_amount: base_receive_amount
                    * Decimal::from_ratio(swap_amount, bounty.swap_amount),
                multiplier,
                increase_only,
            });

            updates.push(Update {
                field: "swap_adjustment_strategy".to_string(),
                old_value: format!("{:?}", bounty.swap_adjustment_strategy),
                new_value: format!("{:?}", updated_swap_adjustment_strategy),
            });

            bounty.swap_adjustment_strategy = updated_swap_adjustment_strategy;
            response = response.add_attribute(
                "swap_adjustment_strategy",
                format!("{:?}", bounty.swap_adjustment_strategy),
            );
        }

        updates.push(Update {
            field: "swap_amount".to_string(),
            old_value: format!("{}", bounty.swap_amount),
            new_value: format!("{}", swap_amount),
        });

        bounty.swap_amount = swap_amount;
        response = response.add_attribute("swap_amount", bounty.swap_amount);
    }

    if let Some(label) = label {
        assert_label_is_no_longer_than_100_characters(&label)?;

        updates.push(Update {
            field: "label".to_string(),
            old_value: bounty.label.unwrap_or_default(),
            new_value: label.clone(),
        });

        bounty.label = Some(label.clone());
        response = response.add_attribute("label", label);
    }

    if let Some(mut destinations) = destinations {
        if destinations.is_empty() {
            destinations.push(Destination {
                allocation: Decimal::percent(100),
                address: bounty.owner.clone(),
                msg: None,
            });
        }

        assert_destinations_limit_is_not_breached(&destinations)?;
        assert_destination_callback_addresses_are_valid(deps.as_ref(), &destinations)?;
        assert_no_destination_allocations_are_zero(&destinations)?;
        assert_destination_allocations_add_up_to_one(&destinations)?;

        updates.push(Update {
            field: "destinations".to_string(),
            old_value: format!("{:?}", bounty.destinations),
            new_value: format!("{:?}", destinations),
        });

        bounty.destinations = destinations.clone();
        response = response.add_attribute("destinations", format!("{:?}", destinations));
    }

    if let Some(slippage_tolerance) = slippage_tolerance {
        assert_slippage_tolerance_is_less_than_or_equal_to_one(slippage_tolerance)?;

        updates.push(Update {
            field: "slippage_tolerance".to_string(),
            old_value: format!("{}", bounty.slippage_tolerance),
            new_value: format!("{}", slippage_tolerance),
        });

        bounty.slippage_tolerance = slippage_tolerance;
        response = response.add_attribute("slippage_tolerance", slippage_tolerance.to_string());
    }

    if let Some(minimum_receive_amount) = minimum_receive_amount {
        updates.push(Update {
            field: "minimum_receive_amount".to_string(),
            old_value: format!("{}", bounty.minimum_receive_amount.unwrap_or_default()),
            new_value: format!("{}", minimum_receive_amount),
        });

        bounty.minimum_receive_amount = Some(minimum_receive_amount);
        response = response.add_attribute("minimum_receive_amount", minimum_receive_amount);
    }

    if let Some(time_interval) = time_interval {
        assert_time_interval_is_valid(&time_interval)?;

        updates.push(Update {
            field: "time_interval".to_string(),
            old_value: format!("{}", bounty.time_interval),
            new_value: format!("{}", time_interval),
        });

        bounty.time_interval = time_interval.clone();
        response = response.add_attribute("time_interval", time_interval);

        if let Some(old_trigger) = bounty.trigger.clone() {
            delete_trigger(deps.storage, bounty.id)?;

            let new_trigger = TriggerConfiguration::Time {
                target_time: get_next_target_time(
                    env.block.time,
                    bounty.started_at.unwrap_or(env.block.time),
                    bounty.time_interval.clone(),
                ),
            };

            save_trigger(
                deps.storage,
                Trigger {
                    bounty_id: bounty.id,
                    configuration: new_trigger.clone(),
                },
            )?;

            updates.push(Update {
                field: "trigger".to_string(),
                old_value: format!("{:?}", old_trigger),
                new_value: format!("{:?}", new_trigger),
            });

            response = response.add_attribute("trigger", format!("{:?}", new_trigger));
        }
    }

    match swap_adjustment_strategy {
        Some(SwapAdjustmentStrategyParams::WeightedScale {
            base_receive_amount,
            multiplier,
            increase_only,
        }) => match bounty.swap_adjustment_strategy {
            Some(SwapAdjustmentStrategy::WeightedScale { .. }) => {
                assert_weighted_scale_multiplier_is_no_more_than_10(multiplier)?;

                updates.push(Update {
                    field: "swap_adjustment_strategy".to_string(),
                    old_value: format!("{:?}", bounty.swap_adjustment_strategy),
                    new_value: format!("{:?}", swap_adjustment_strategy),
                });

                bounty.swap_adjustment_strategy = Some(SwapAdjustmentStrategy::WeightedScale {
                    base_receive_amount,
                    multiplier,
                    increase_only,
                });

                response = response.add_attribute(
                    "swap_adjustment_strategy",
                    format!("{:?}", bounty.swap_adjustment_strategy),
                );
            }
            _ => {
                return Err(ContractError::CustomError {
                    val: format!(
                        "cannot update swap adjustment strategy from {:?} to {:?}",
                        bounty.swap_adjustment_strategy, swap_adjustment_strategy
                    ),
                })
            }
        },
        Some(swap_adjustment_strategy) => {
            return Err(ContractError::CustomError {
                val: format!(
                    "cannot update swap adjustment strategy from {:?} to {:?}",
                    bounty.swap_adjustment_strategy, swap_adjustment_strategy
                ),
            })
        }
        _ => {}
    }

    update_bounty(deps.storage, vault.clone())?;

    create_event(
        deps.storage,
        EventBuilder::new(bounty.id, env.block, EventData::DcaVaultUpdated { updates }),
    )?;

    Ok(response)
}

#[cfg(test)]
mod update_bounty_tests {
    use super::update_bounty_handler;
    use crate::{
        constants::{ONE, TEN},
        handlers::get_events_by_resource_id::get_events_by_resource_id_handler,
        helpers::time::get_next_target_time,
        state::{config::update_config, vaults::get_bounty},
        tests::{
            helpers::{instantiate_contract, setup_bounty},
            mocks::{ADMIN, USER},
        },
        types::{
            config::Config,
            destination::Destination,
            event::{Event, EventData},
            position_type::PositionType,
            swap_adjustment_strategy::{
                BaseDenom, SwapAdjustmentStrategy, SwapAdjustmentStrategyParams,
            },
            time_interval::TimeInterval,
            trigger::TriggerConfiguration,
            update::Update,
            bounty::{Bounty, BountyStatus},
        },
    };
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env, mock_info},
        Addr, Decimal, Uint128,
    };

    #[test]
    fn with_slippage_tolerance_larger_than_one_fails() {
        let mut deps = mock_dependencies();

        instantiate_contract(deps.as_mut(), mock_env(), mock_info(ADMIN, &[]));

        let bounty = setup_bounty(deps.as_mut(), mock_env(), Bounty::default());

        let err = update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            None,
            Some(Decimal::percent(101)),
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
    fn with_custom_time_interval_less_than_60_seconds_fails() {
        let mut deps = mock_dependencies();

        instantiate_contract(deps.as_mut(), mock_env(), mock_info(ADMIN, &[]));

        let bounty = setup_bounty(deps.as_mut(), mock_env(), Bounty::default());

        let err = update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            None,
            None,
            None,
            Some(TimeInterval::Custom { seconds: 12 }),
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
    fn with_label_longer_than_100_characters_fails() {
        let mut deps = mock_dependencies();

        instantiate_contract(deps.as_mut(), mock_env(), mock_info(ADMIN, &[]));

        let bounty = setup_bounty(deps.as_mut(), mock_env(), Bounty::default());

        let label = Some("12345678910".repeat(10));

        let err = update_vault_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            label,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error: Bounty label cannot be longer than 100 characters"
        );
    }

    #[test]
    fn for_bounty_with_different_owner_fails() {
        let mut deps = mock_dependencies();

        instantiate_contract(deps.as_mut(), mock_env(), mock_info(ADMIN, &[]));

        let bounty = setup_bounty(
            deps.as_mut(),
            mock_env(),
            Bounty {
                owner: Addr::unchecked("random"),
                ..Bounty::default()
            },
        );

        let label = Some("My new bounty".to_string());

        let err = update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            label,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "Unauthorized");
    }

    #[test]
    fn for_cancelled_bounty_fails() {
        let mut deps = mock_dependencies();

        let bounty = setup_bounty(
            deps.as_mut(),
            mock_env(),
            Bounty {
                status: BountyStatus::Cancelled,
                ..Bounty::default()
            },
        );

        let label = Some("My new bounty".to_string());

        let err = update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            label,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "Error: bounty is already cancelled");
    }

    #[test]
    fn with_more_than_10_destinations_fails() {
        let mut deps = mock_dependencies();

        let bounty = setup_bounty(deps.as_mut(), mock_env(), Bounty::default());

        let destinations = vec![
            Destination {
                address: Addr::unchecked("random"),
                allocation: Decimal::percent(10),
                msg: None,
            };
            11
        ];

        let err = update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            Some(destinations),
            None,
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
    fn with_destination_allocations_less_than_100_percent_fails() {
        let mut deps = mock_dependencies();

        let bounty = setup_bounty(deps.as_mut(), mock_env(), Bounty::default());

        let destinations = vec![
            Destination {
                address: Addr::unchecked("random"),
                allocation: Decimal::percent(10),
                msg: None,
            },
            Destination {
                address: Addr::unchecked("random"),
                allocation: Decimal::percent(10),
                msg: None,
            },
        ];

        let err = update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            Some(destinations),
            None,
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
    fn with_destination_allocations_more_than_100_percent_fails() {
        let mut deps = mock_dependencies();

        let bounty = setup_bounty(deps.as_mut(), mock_env(), Bounty::default());

        let destinations = vec![
            Destination {
                address: Addr::unchecked("random"),
                allocation: Decimal::percent(50),
                msg: None,
            },
            Destination {
                address: Addr::unchecked("random"),
                allocation: Decimal::percent(51),
                msg: None,
            },
        ];

        let err = update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            Some(destinations),
            None,
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
    fn with_destination_with_zero_allocation_fails() {
        let mut deps = mock_dependencies();

        let bounty = setup_bounty(deps.as_mut(), mock_env(), Bounty::default());

        let destinations = vec![
            Destination {
                address: Addr::unchecked("random"),
                allocation: Decimal::percent(100),
                msg: None,
            },
            Destination {
                address: Addr::unchecked("random"),
                allocation: Decimal::zero(),
                msg: None,
            },
        ];

        let err = update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            Some(destinations),
            None,
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
    fn updating_risk_weighted_average_strategy_fails() {
        let mut deps = mock_dependencies();

        let existing_swap_adjustment_strategy = Some(SwapAdjustmentStrategy::RiskWeightedAverage {
            model_id: 30,
            base_denom: BaseDenom::Bitcoin,
            position_type: PositionType::Enter,
        });

        let bounty = setup_bounty(
            deps.as_mut(),
            mock_env(),
            Bounty {
                swap_adjustment_strategy: existing_swap_adjustment_strategy.clone(),
                ..Bounty::default()
            },
        );

        let new_swap_adjustment_strategy = SwapAdjustmentStrategyParams::RiskWeightedAverage {
            base_denom: BaseDenom::Bitcoin,
            position_type: PositionType::Enter,
        };

        let err = update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            None,
            None,
            None,
            None,
            Some(new_swap_adjustment_strategy.clone()),
            None,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            format!(
                "Error: cannot update swap adjustment strategy from {:?} to {:?}",
                existing_swap_adjustment_strategy, new_swap_adjustment_strategy
            )
        );
    }

    #[test]
    fn changing_risk_weighted_average_strategy_fails() {
        let mut deps = mock_dependencies();

        let existing_swap_adjustment_strategy = Some(SwapAdjustmentStrategy::RiskWeightedAverage {
            model_id: 30,
            base_denom: BaseDenom::Bitcoin,
            position_type: PositionType::Enter,
        });

        let bounty = setup_bounty(
            deps.as_mut(),
            mock_env(),
            Bounty {
                swap_adjustment_strategy: existing_swap_adjustment_strategy.clone(),
                ..Bounty::default()
            },
        );

        let new_swap_adjustment_strategy = Some(SwapAdjustmentStrategyParams::WeightedScale {
            base_receive_amount: Uint128::new(18277),
            multiplier: Decimal::percent(213),
            increase_only: false,
        });

        let err = update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            None,
            None,
            None,
            None,
            new_swap_adjustment_strategy.clone(),
            None,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            format!(
                "Error: cannot update swap adjustment strategy from {:?} to {:?}",
                existing_swap_adjustment_strategy, new_swap_adjustment_strategy
            )
        );
    }

    #[test]
    fn adding_weighted_scale_swap_adjustment_strategy_fails() {
        let mut deps = mock_dependencies();

        let bounty = setup_bounty(deps.as_mut(), mock_env(), Bounty::default());

        let strategy = Some(SwapAdjustmentStrategyParams::WeightedScale {
            base_receive_amount: Uint128::new(2732),
            multiplier: Decimal::percent(150),
            increase_only: false,
        });

        let err = update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            None,
            None,
            None,
            None,
            strategy.clone(),
            None,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            format!(
                "Error: cannot update swap adjustment strategy from {:?} to {:?}",
                bounty.swap_adjustment_strategy, strategy
            )
        );
    }

    #[test]
    fn updating_swap_amount_and_minimum_receive_amount_fails() {
        let mut deps = mock_dependencies();

        let bounty = setup_bounty(deps.as_mut(), mock_env(), Bounty::default());

        let err = update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            None,
            None,
            Some(Uint128::new(621837621)),
            None,
            None,
            Some(Uint128::new(3498473290)),
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            format!(
                "Error: cannot update swap amount and minimum receive amount at the same time.",
            )
        );
    }

    #[test]
    fn updating_swap_amount_and_swap_adjustment_strategy_fails() {
        let mut deps = mock_dependencies();

        let bounty = setup_bounty(deps.as_mut(), mock_env(), Bounty::default());

        let err = update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            None,
            None,
            None,
            None,
            Some(SwapAdjustmentStrategyParams::WeightedScale {
                base_receive_amount: Uint128::new(2732),
                multiplier: Decimal::percent(150),
                increase_only: false,
            }),
            Some(Uint128::new(436753262)),
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            format!(
                "Error: cannot update swap amount and swap adjustment strategy at the same time."
            )
        );
    }

    #[test]
    fn updating_swap_amount_updates_minimum_receive_amount() {
        let mut deps = mock_dependencies();

        let bounty = setup_bounty(
            deps.as_mut(),
            mock_env(),
            Bounty {
                swap_amount: ONE,
                minimum_receive_amount: Some(TEN),
                ..Bounty::default()
            },
        );

        update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(bounty.swap_amount * Uint128::new(2)),
        )
        .unwrap();

        let updated_bounty = get_bounty(deps.as_ref().storage, bounty.id).unwrap();

        assert_eq!(
            updated_bounty.minimum_receive_amount,
            Some(TEN * Uint128::new(2))
        );
    }

    #[test]
    fn updating_swap_amount_updates_weighted_scale_swap_adjustment_strategy_base_receive_amount() {
        let mut deps = mock_dependencies();

        let bounty = setup_bounty(
            deps.as_mut(),
            mock_env(),
            Bounty {
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::WeightedScale {
                    base_receive_amount: Uint128::new(2732),
                    multiplier: Decimal::percent(150),
                    increase_only: false,
                }),
                ..Bounty::default()
            },
        );

        update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(bounty.swap_amount * Uint128::new(2)),
        )
        .unwrap();

        let updated_bounty = get_bounty(deps.as_ref().storage, bounty.id).unwrap();

        assert_eq!(
            updated_bounty.swap_adjustment_strategy,
            Some(SwapAdjustmentStrategy::WeightedScale {
                base_receive_amount: Uint128::new(2732) * Uint128::new(2),
                multiplier: Decimal::percent(150),
                increase_only: false,
            })
        );
    }

    #[test]
    fn updates_swap_amount() {
        let mut deps = mock_dependencies();

        let bounty = setup_bounty(
            deps.as_mut(),
            mock_env(),
            Bounty {
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::WeightedScale {
                    base_receive_amount: Uint128::new(2732),
                    multiplier: Decimal::percent(150),
                    increase_only: false,
                }),
                minimum_receive_amount: Some(ONE),
                ..Bounty::default()
            },
        );

        let swap_amount = bounty.swap_amount * Uint128::new(2);

        update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(swap_amount),
        )
        .unwrap();

        let updated_bounty = get_bounty(deps.as_ref().storage, bounty.id).unwrap();

        assert_ne!(bounty.swap_amount, swap_amount);
        assert_eq!(updated_bounty.swap_amount, swap_amount);
    }

    #[test]
    fn updates_weighted_scale_swap_adjustment_strategy() {
        let mut deps = mock_dependencies();

        let bounty = setup_bounty(
            deps.as_mut(),
            mock_env(),
            Bounty {
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::WeightedScale {
                    base_receive_amount: Uint128::new(2732),
                    multiplier: Decimal::percent(150),
                    increase_only: false,
                }),
                ..Bounty::default()
            },
        );

        let base_receive_amount = Uint128::new(212831);
        let multiplier = Decimal::percent(300);
        let increase_only = true;

        let strategy = Some(SwapAdjustmentStrategyParams::WeightedScale {
            base_receive_amount,
            multiplier,
            increase_only,
        });

        update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            None,
            None,
            None,
            None,
            strategy,
            None,
        )
        .unwrap();

        let updated_bounty = get_bounty(deps.as_ref().storage, bounty.id).unwrap();

        assert_eq!(
            updated_bounty.swap_adjustment_strategy,
            Some(SwapAdjustmentStrategy::WeightedScale {
                base_receive_amount,
                multiplier,
                increase_only,
            })
        );
    }

    #[test]
    fn updates_the_bounty_label() {
        let mut deps = mock_dependencies();

        let bounty = setup_bounty(deps.as_mut(), mock_env(), Bounty::default());

        let label = Some("123456789".repeat(10));

        update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            label.clone(),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let updated_bounty = get_bounty(deps.as_ref().storage, bounty.id).unwrap();

        assert_eq!(updated_bounty.label, label);
    }

    #[test]
    fn updates_the_bounty_destinations() {
        let mut deps = mock_dependencies();

        let bounty = setup_bounty(deps.as_mut(), mock_env(), Bounty::default());

        let destinations = vec![
            Destination {
                address: Addr::unchecked("random"),
                allocation: Decimal::percent(50),
                msg: None,
            },
            Destination {
                address: Addr::unchecked("random"),
                allocation: Decimal::percent(50),
                msg: None,
            },
        ];

        update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            Some(destinations.clone()),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let updated_bounty = get_bounty(deps.as_ref().storage, bounty.id).unwrap();

        assert_ne!(updated_bounty.destinations, bounty.destinations);
        assert_eq!(updated_bounty.destinations, destinations);
    }

    #[test]
    fn sets_the_bounty_destination_to_owner_when_update_list_is_empty() {
        let mut deps = mock_dependencies();

        let bounty = setup_bounty(deps.as_mut(), mock_env(), Bounty::default());

        update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            Some(vec![]),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let updated_bounty = get_bounty(deps.as_ref().storage, bounty.id).unwrap();

        assert_ne!(updated_bounty.destinations, bounty.destinations);
        assert_eq!(
            updated_bounty.destinations,
            vec![Destination {
                address: bounty.owner,
                allocation: Decimal::percent(100),
                msg: None,
            }]
        );
    }

    #[test]
    fn updates_slippage_tolerance() {
        let mut deps = mock_dependencies();

        let bounty = setup_bounty(deps.as_mut(), mock_env(), Bounty::default());

        let slippage_tolerance = Decimal::percent(1);

        update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            None,
            Some(slippage_tolerance),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let updated_bounty = get_bounty(deps.as_ref().storage, bounty.id).unwrap();

        assert_eq!(updated_bounty.slippage_tolerance, slippage_tolerance);
    }

    #[test]
    fn updates_minimum_receive_amount() {
        let mut deps = mock_dependencies();

        let bounty = setup_bounty(deps.as_mut(), mock_env(), Bounty::default());

        let minimum_receive_amount = Some(Uint128::new(12387));

        update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            None,
            None,
            minimum_receive_amount,
            None,
            None,
            None,
        )
        .unwrap();

        let updated_bounty = get_bounty(deps.as_ref().storage, bounty.id).unwrap();

        assert_eq!(updated_bounty.minimum_receive_amount, minimum_receive_amount);
    }

    #[test]
    fn updates_time_interval() {
        let mut deps = mock_dependencies();

        let bounty = setup_bounty(deps.as_mut(), mock_env(), Bounty::default());

        let time_interval = TimeInterval::Custom { seconds: 31321 };

        update_bounty_handler(
            deps.as_mut(),
            mock_env(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            None,
            None,
            None,
            Some(time_interval.clone()),
            None,
            None,
        )
        .unwrap();

        let updated_bounty = get_bounty(deps.as_ref().storage, bounty.id).unwrap();

        assert_eq!(updated_bounty.time_interval, time_interval);
    }

    #[test]
    fn updates_the_trigger_target_time() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let bounty = setup_bounty(deps.as_mut(), mock_env(), Bounty::default());

        let time_interval = TimeInterval::Custom { seconds: 60 };

        update_bounty_handler(
            deps.as_mut(),
            env.clone(),
            mock_info(USER, &[]),
            bounty.id,
            None,
            None,
            None,
            None,
            Some(time_interval.clone()),
            None,
            None,
        )
        .unwrap();

        let updated_bounty = get_bounty(deps.as_ref().storage, bounty.id).unwrap();

        match updated_bounty.trigger {
            Some(TriggerConfiguration::Time { target_time }) => {
                assert_eq!(
                    target_time,
                    get_next_target_time(
                        env.block.time,
                        bounty.started_at.unwrap_or(env.block.time),
                        time_interval,
                    )
                )
            }
            _ => panic!("expected trigger to be of type Time"),
        }
    }

    #[test]
    fn publishes_bounty_updated_event() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let bounty = setup_bounty(deps.as_mut(), mock_env(), Bounty::default());

        let new_time_interval = TimeInterval::Custom { seconds: 60 };

        update_config(deps.as_mut().storage, Config::default()).unwrap();

        let new_label = &"new vault";
        let new_destinations = vec![
            Destination {
                address: Addr::unchecked("random-1"),
                allocation: Decimal::percent(50),
                msg: None,
            },
            Destination {
                address: Addr::unchecked("random-2"),
                allocation: Decimal::percent(50),
                msg: None,
            },
        ];
        let new_slippage_tolerance = Decimal::percent(12);
        let new_minimum_receive_amount = Uint128::new(2312312231);

        update_bounty_handler(
            deps.as_mut(),
            env.clone(),
            mock_info(USER, &[]),
            bounty.id,
            Some(new_label.to_string()),
            Some(new_destinations.clone()),
            Some(new_slippage_tolerance),
            Some(new_minimum_receive_amount),
            Some(new_time_interval.clone()),
            None,
            None,
        )
        .unwrap();

        let events = get_events_by_resource_id_handler(deps.as_ref(), bounty.id, None, None, None)
            .unwrap()
            .events;

        assert_eq!(
            events.first().unwrap(),
            &Event {
                id: 1,
                resource_id: vault.id,
                timestamp: env.block.time,
                block_height: env.block.height,
                data: EventData::DcaVaultUpdated {
                    updates: vec![
                        Update {
                            field: "label".to_string(),
                            old_value: bounty.label.unwrap(),
                            new_value: new_label.to_string(),
                        },
                        Update {
                            field: "destinations".to_string(),
                            old_value: format!("{:?}", bounty.destinations),
                            new_value: format!("{:?}", new_destinations),
                        },
                        Update {
                            field: "slippage_tolerance".to_string(),
                            old_value: bounty.slippage_tolerance.to_string(),
                            new_value: new_slippage_tolerance.to_string(),
                        },
                        Update {
                            field: "minimum_receive_amount".to_string(),
                            old_value: bounty.minimum_receive_amount.unwrap_or_default().to_string(),
                            new_value: new_minimum_receive_amount.to_string(),
                        },
                        Update {
                            field: "time_interval".to_string(),
                            old_value: bounty.time_interval.to_string(),
                            new_value: new_time_interval.to_string(),
                        },
                        Update {
                            field: "trigger".to_string(),
                            old_value: format!("{:?}", bounty.trigger.unwrap()),
                            new_value: format!(
                                "{:?}",
                                TriggerConfiguration::Time {
                                    target_time: get_next_target_time(
                                        env.block.time,
                                        bounty.started_at.unwrap_or(env.block.time),
                                        new_time_interval,
                                    )
                                }
                            )
                        }
                    ]
                }
            }
        )
    }
}
