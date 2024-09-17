use crate::{
    error::ContractError,
    helpers::{
        disbursement::get_disbursement_messages,
        fees::{get_fee_messages, get_performance_fee},
        price::get_twap_to_now,
        validation::assert_sender_is_executor,
    },
    state::{
        cache::BOUNTY_ID_CACHE,
        config::get_config,
        disburse_escrow_tasks::{delete_disburse_escrow_task, get_disburse_escrow_task_due_date},
        events::create_event,
        bounties::{get_bounty, update_bounty},
    },
    types::{
        event::{EventBuilder, EventData},
        bounty::Bounty,
    },
};
use cosmwasm_std::{Coin, DepsMut, Env, MessageInfo, Response, Uint128};
use shared::coin::{empty_of, subtract};

pub fn disburse_escrow_handler(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    bounty_id: Uint128,
) -> Result<Response, ContractError> {
    assert_sender_is_executor(deps.storage, &env, &info.sender)?;

    let bounty = get_bounty(deps.storage, bounty_id)?;

    let response = Response::new()
        .add_attribute("disburse_escrow", "true")
        .add_attribute("bounty_id", bounty.id)
        .add_attribute("owner", bounty.owner.clone());

    if bounty.escrowed_amount.amount.is_zero() {
        return Ok(response
            .add_attribute(
                "performance_fee",
                format!("{:?}", Coin::new(0, bounty.target_denom.clone())),
            )
            .add_attribute(
                "escrow_disbursed",
                format!("{:?}", Coin::new(0, bounty.target_denom)),
            ));
    }

    let due_date = get_disburse_escrow_task_due_date(deps.storage, bounty.id)?;

    if let Some(due_date) = due_date {
        if env.block.time < due_date {
            return Err(ContractError::CustomError {
                val: format!(
                    "Escrow is not available to be disbursed until {:?}",
                    due_date
                )
                .to_string(),
            });
        }
    }

    let config = get_config(deps.storage)?;

    let current_price = get_twap_to_now(
        &deps.querier,
        config.exchange_contract_address.clone(),
        bounty.get_swap_denom(),
        bounty.target_denom.clone(),
        config.twap_period,
        bounty.route.clone(),
    )?;

    let performance_fee = get_performance_fee(&bounty, current_price)?;
    let amount_to_disburse = subtract(&bounty.escrowed_amount, &performance_fee)?;

    let bounty = update_bounty(
        deps.storage,
        Bounty {
            escrowed_amount: empty_of(bounty.escrowed_amount),
            ..bounty
        },
    )?;

    create_event(
        deps.storage,
        EventBuilder::new(
            bounty.id,
            env.block.clone(),
            EventData::DcaVaultEscrowDisbursed {
                amount_disbursed: amount_to_disburse.clone(),
                performance_fee: performance_fee.clone(),
            },
        ),
    )?;

    delete_disburse_escrow_task(deps.storage, bounty.id)?;

    BOUNTY_ID_CACHE.save(deps.storage, &bounty.id)?;

    Ok(response
        .add_submessages(get_disbursement_messages(
            deps.api,
            deps.storage,
            &bounty,
            amount_to_disburse.amount,
        )?)
        .add_submessages(get_fee_messages(
            deps.as_ref(),
            env,
            vec![performance_fee.amount],
            bounty.target_denom.clone(),
            true,
        )?)
        .add_attribute("performance_fee", format!("{:?}", performance_fee))
        .add_attribute("escrow_disbursed", format!("{:?}", amount_to_disburse)))
}

#[cfg(test)]
mod disburse_escrow_tests {
    use super::*;
    use crate::{
        constants::{AFTER_FAILED_AUTOMATION_REPLY_ID, ONE, TEN, TEN_DECIMAL},
        handlers::get_events_by_resource_id::get_events_by_resource_id_handler,
        state::{
            config::get_config,
            disburse_escrow_tasks::{get_disburse_escrow_tasks, save_disburse_escrow_task},
            bounties::get_bounty,
        },
        tests::{
            helpers::{instantiate_contract, setup_bounty},
            mocks::{calc_mock_dependencies, ADMIN, DENOM_UKUJI, DENOM_UUSK},
        },
        types::{
            destination::Destination,
            event::{Event, EventData},
            performance_assessment_strategy::PerformanceAssessmentStrategy,
            swap_adjustment_strategy::SwapAdjustmentStrategy,
            bounty::{Bounty, BountyStatus},
        },
    };
    use cosmwasm_std::{
        testing::{mock_env, mock_info},
        BankMsg, Coin, Decimal, SubMsg, Uint128,
    };
    use shared::coin::add_to;

    #[test]
    fn when_disburse_escrow_task_is_not_due_fails() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let bounty = setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                escrowed_amount: Coin::new(ONE.into(), DENOM_UUSK),
                ..Bounty::default()
            },
        );

        save_disburse_escrow_task(
            deps.as_mut().storage,
            bounty.id,
            env.block.time.plus_seconds(10),
        )
        .unwrap();

        let err = disburse_escrow_handler(deps.as_mut(), env, info, bounty.id).unwrap_err();

        assert!(err
            .to_string()
            .contains("Error: Escrow is not available to be disbursed"));
    }

    #[test]
    fn caches_bounty_id_for_after_automation_handler() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let bounty = setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                escrowed_amount: Coin::new(ONE.into(), DENOM_UUSK),
                ..Bounty::default()
            },
        );

        save_disburse_escrow_task(
            deps.as_mut().storage,
            bounty.id,
            env.block.time.minus_seconds(10),
        )
        .unwrap();

        let cached_bounty_id = BOUNTY_ID_CACHE.load(deps.as_ref().storage).unwrap();

        assert_eq!(bounty.id, cached_bounty_id)
    }

    #[test]
    fn when_disburse_escrow_task_is_due_succeeds() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let bounty = setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                escrowed_amount: Coin::new(ONE.into(), DENOM_UUSK),
                ..Bounty::default()
            },
        );

        save_disburse_escrow_task(
            deps.as_mut().storage,
            bounty.id,
            env.block.time.minus_seconds(10),
        )
        .unwrap();

        let response = disburse_escrow_handler(deps.as_mut(), env, info, bounty.id).unwrap();

        assert!(!response.messages.is_empty());
    }

    #[test]
    fn when_escrowed_balance_is_empty_sends_no_messages() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let bounty = setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                escrowed_amount: Coin::new(0, DENOM_UUSK),
                ..Bounty::default()
            },
        );

        let response = disburse_escrow_handler(deps.as_mut(), env, info, bounty.id).unwrap();

        assert!(response.messages.is_empty());
    }

    #[test]
    fn when_no_fee_is_owed_returns_entire_escrow_to_owner() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let bounty = setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                status: BountyStatus::Inactive,
                destinations: vec![Destination::default()],
                deposited_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                escrowed_amount: Coin::new((ONE * Decimal::percent(5)).into(), DENOM_UUSK),
                performance_assessment_strategy: Some(
                    PerformanceAssessmentStrategy::CompareToStandardDca {
                        swapped_amount: Coin::new(ONE.into(), DENOM_UKUJI),
                        received_amount: Coin::new(ONE.into(), DENOM_UUSK),
                    },
                ),
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::default()),
                ..Bounty::default()
            },
        );

        let response = disburse_escrow_handler(deps.as_mut(), env, info, bounty.id).unwrap();

        assert!(response.messages.contains(&SubMsg::reply_always(
            BankMsg::Send {
                to_address: bounty.destinations[0].address.to_string(),
                amount: vec![bounty.escrowed_amount]
            },
            AFTER_FAILED_AUTOMATION_REPLY_ID
        )));
    }

    #[test]
    fn when_large_fee_is_owed_returns_entire_escrow_to_fee_collector() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let bounty = setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                status: BountyStatus::Inactive,
                swapped_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                received_amount: Coin::new(TEN.into(), DENOM_UUSK),
                deposited_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                escrowed_amount: Coin::new((ONE * Decimal::percent(5)).into(), DENOM_UUSK),
                performance_assessment_strategy: Some(
                    PerformanceAssessmentStrategy::CompareToStandardDca {
                        swapped_amount: Coin::new(ONE.into(), DENOM_UKUJI),
                        received_amount: Coin::new(ONE.into(), DENOM_UUSK),
                    },
                ),
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::default()),
                ..Bounty::default()
            },
        );

        deps.querier.update_fin_price(&TEN_DECIMAL);

        let config = get_config(&deps.storage).unwrap();

        let response = disburse_escrow_handler(deps.as_mut(), env, info, bounty.id).unwrap();

        assert_eq!(
            response.messages.first().unwrap(),
            &SubMsg::new(BankMsg::Send {
                to_address: config.fee_collectors[0].address.to_string(),
                amount: vec![bounty.escrowed_amount]
            })
        );
    }

    #[test]
    fn publishes_escrow_disbursed_event() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let bounty = setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                status: BountyStatus::Inactive,
                swapped_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                received_amount: Coin::new((TEN + ONE).into(), DENOM_UUSK),
                deposited_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                escrowed_amount: Coin::new(((TEN + ONE) * Decimal::percent(5)).into(), DENOM_UUSK),
                performance_assessment_strategy: Some(
                    PerformanceAssessmentStrategy::CompareToStandardDca {
                        swapped_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                        received_amount: Coin::new(TEN.into(), DENOM_UUSK),
                    },
                ),
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::default()),
                ..Bounty::default()
            },
        );

        disburse_escrow_handler(deps.as_mut(), env.clone(), info, bounty.id).unwrap();

        let events = get_events_by_resource_id_handler(deps.as_ref(), bounty.id, None, None, None)
            .unwrap()
            .events;

        let performance_fee = Coin::new(
            (ONE * Decimal::percent(20) - Uint128::one()).into(),
            bounty.target_denom.clone(),
        );

        assert_eq!(
            events.get(0).unwrap(),
            &Event {
                id: 1,
                resource_id: bounty.id,
                timestamp: env.block.time,
                block_height: env.block.height,
                data: EventData::DcaVaultEscrowDisbursed {
                    amount_disbursed: Coin::new(
                        ((subtract(&bounty.escrowed_amount, &performance_fee).unwrap()).amount
                            - Uint128::one())
                        .into(),
                        DENOM_UUSK
                    ),
                    performance_fee: add_to(&performance_fee, Uint128::one()),
                }
            }
        )
    }

    #[test]
    fn sets_escrow_balance_to_zero() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let bounty = setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                status: BountyStatus::Inactive,
                swapped_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                received_amount: Coin::new((TEN + ONE).into(), DENOM_UUSK),
                deposited_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                escrowed_amount: Coin::new(((TEN + ONE) * Decimal::percent(5)).into(), DENOM_UUSK),
                performance_assessment_strategy: Some(
                    PerformanceAssessmentStrategy::CompareToStandardDca {
                        swapped_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                        received_amount: Coin::new(TEN.into(), DENOM_UUSK),
                    },
                ),
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::default()),
                ..Bounty::default()
            },
        );

        disburse_escrow_handler(deps.as_mut(), env, info, bounty.id).unwrap();

        let bounty = get_bounty(deps.as_ref().storage, bounty.id).unwrap();

        assert_eq!(
            bounty.escrowed_amount,
            Coin::new(0, bounty.target_denom.clone())
        );
    }

    #[test]
    fn deletes_disburse_escrow_task() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let bounty = setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                status: BountyStatus::Inactive,
                swapped_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                received_amount: Coin::new((TEN + ONE).into(), DENOM_UUSK),
                deposited_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                escrowed_amount: Coin::new(((TEN + ONE) * Decimal::percent(5)).into(), DENOM_UUSK),
                performance_assessment_strategy: Some(
                    PerformanceAssessmentStrategy::CompareToStandardDca {
                        swapped_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                        received_amount: Coin::new(TEN.into(), DENOM_UUSK),
                    },
                ),
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::default()),
                ..Bounty::default()
            },
        );

        save_disburse_escrow_task(
            deps.as_mut().storage,
            bounty.id,
            env.block.time.minus_seconds(10),
        )
        .unwrap();

        let disburse_escrow_tasks_before =
            get_disburse_escrow_tasks(deps.as_ref().storage, env.block.time, None).unwrap();

        disburse_escrow_handler(deps.as_mut(), env.clone(), info, bounty.id).unwrap();

        let disburse_escrow_tasks_after =
            get_disburse_escrow_tasks(deps.as_ref().storage, env.block.time, None).unwrap();

        assert_eq!(disburse_escrow_tasks_before.len(), 1);
        assert_eq!(disburse_escrow_tasks_after.len(), 0);
    }
}
