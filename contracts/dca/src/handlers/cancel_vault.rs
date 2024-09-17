use crate::constants::FAIL_SILENTLY_REPLY_ID;
use crate::error::ContractError;
use crate::helpers::validation::{
    assert_sender_is_admin_or_vault_owner, assert_vault_is_not_cancelled,
};
use crate::state::config::get_config;
use crate::state::disburse_escrow_tasks::save_disburse_escrow_task;
use crate::state::events::create_event;
use crate::state::triggers::delete_trigger;
use crate::state::vaults::{get_vault, update_vault};
use crate::types::event::{EventBuilder, EventData};
use crate::types::trigger::TriggerConfiguration;
use crate::types::bounty::{Bounty, BountyStatus};
use cosmwasm_std::{to_json_binary, BankMsg, DepsMut, Response, Uint128, WasmMsg};
use cosmwasm_std::{Env, MessageInfo, SubMsg};
use exchange::msg::ExecuteMsg;
use shared::coin::empty_of;

pub fn cancel_bounty_handler(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    bounty_id: Uint128,
) -> Result<Response, ContractError> {
    let bounty = get_bounty(deps.storage, bounty_id)?;

    assert_sender_is_admin_or_bounty_owner(deps.storage, bounty.owner.clone(), info.sender)?;
    assert_bounty_is_not_cancelled(&bounty)?;

    create_event(
        deps.storage,
        EventBuilder::new(bounty.id, env.block.clone(), EventData::DcaVaultCancelled {}),
    )?;

    if bounty.escrowed_amount.amount > Uint128::zero() {
        save_disburse_escrow_task(
            deps.storage,
            bounty.id,
            bounty.get_expected_execution_completed_date(env.block.time),
        )?;
    };

    let mut submessages = Vec::<SubMsg>::new();

    if bounty.balance.amount > Uint128::zero() {
        submessages.push(SubMsg::new(BankMsg::Send {
            to_address: bounty.destination.to_string(),
            amount: vec![bounty.balance.clone()],
        }));
    }

    update_bounty(
        deps.storage,
        Bounty {
            status: BountyStatus::Cancelled,
            balance: empty_of(bounty.balance.clone()),
            ..bounty.clone()
        },
    )?;

    if let Some(TriggerConfiguration::Price { order_idx, .. }) = bounty.trigger {
        let config = get_config(deps.storage)?;

        submessages.push(SubMsg::reply_on_error(
            WasmMsg::Execute {
                contract_addr: config.exchange_contract_address.to_string(),
                msg: to_json_binary(&ExecuteMsg::RetractOrder {
                    order_idx,
                    denoms: vault.denoms(),
                })
                .unwrap(),
                funds: vec![],
            },
            FAIL_SILENTLY_REPLY_ID,
        ));

        submessages.push(SubMsg::reply_on_error(
            WasmMsg::Execute {
                contract_addr: config.exchange_contract_address.to_string(),
                msg: to_json_binary(&ExecuteMsg::WithdrawOrder {
                    order_idx,
                    denoms: vault.denoms(),
                })
                .unwrap(),
                funds: vec![],
            },
            FAIL_SILENTLY_REPLY_ID,
        ));
    };

    delete_trigger(deps.storage, vault.id)?;

    Ok(Response::new()
        .add_attribute("cancel_vault", "true")
        .add_attribute("vault_id", vault.id)
        .add_attribute("owner", vault.owner)
        .add_attribute("refunded_amount", vault.balance.to_string())
        .add_submessages(submessages))
}

#[cfg(test)]
mod cancel_bounty_tests {
    use super::*;
    use crate::constants::ONE;
    use crate::handlers::get_events_by_resource_id::get_events_by_resource_id_handler;
    use crate::handlers::get_vault::get_vault_handler;
    use crate::state::disburse_escrow_tasks::get_disburse_escrow_tasks;
    use crate::tests::helpers::{instantiate_contract, setup_vault};
    use crate::tests::mocks::{calc_mock_dependencies, ADMIN, DENOM_UKUJI};
    use crate::types::event::{EventBuilder, EventData};
    use crate::types::bounty::{Bounty, BountyStatus};
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{BankMsg, Coin, Decimal, SubMsg, Uint128};

    #[test]
    fn should_return_balance_to_owner() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let bounty = setup_bounty(deps.as_mut(), env.clone(), Bounty::default());

        let response = cancel_bounty_handler(deps.as_mut(), env, info, bounty.id).unwrap();

        assert!(response.messages.contains(&SubMsg::new(BankMsg::Send {
            to_address: bounty.owner.to_string(),
            amount: vec![bounty.balance],
        })));
    }

    #[test]
    fn with_price_trigger_should_return_balance_to_owner() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let bounty = setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                trigger: Some(TriggerConfiguration::Price {
                    target_price: Decimal::percent(200),
                    order_idx: Uint128::new(28),
                }),
                ..Bounty::default()
            },
        );

        let response = cancel_bounty_handler(deps.as_mut(), env, info, bounty.id).unwrap();

        assert!(response.messages.contains(&SubMsg::new(BankMsg::Send {
            to_address: bounty.owner.to_string(),
            amount: vec![bounty.balance],
        })));
    }

    #[test]
    fn should_publish_bounty_cancelled_event() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let bounty = setup_vault(deps.as_mut(), env.clone(), Bounty::default());

        cancel_bounty_handler(deps.as_mut(), env.clone(), info, bounty.id).unwrap();

        let events = get_events_by_resource_id_handler(deps.as_ref(), bounty.id, None, None, None)
            .unwrap()
            .events;

        assert!(events.contains(
            &EventBuilder::new(bounty.id, env.block, EventData::DcaVaultCancelled {}).build(1)
        ));
    }

    #[test]
    fn when_bounty_has_time_trigger_should_cancel_bounty() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let bounty = setup_bounty(deps.as_mut(), env.clone(), Bounty::default());

        cancel_bounty_handler(deps.as_mut(), env, info, bounty.id).unwrap();

        let updated_bounty = get_bounty_handler(deps.as_ref(), bounty.id).unwrap().bounty;

        assert_eq!(bounty.status, BountyStatus::Active);
        assert_eq!(updated_bounty.status, BountyStatus::Cancelled);
    }

    #[test]
    fn should_empty_bounty_balance() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let bounty = setup_bounty(deps.as_mut(), env.clone(), Bounty::default());

        cancel_bounty_handler(deps.as_mut(), env, info, bounty.id).unwrap();

        let updated_bounty = get_bounty_handler(deps.as_ref(), bounty.id).unwrap().bounty;

        assert!(bounty.balance.amount.gt(&Uint128::zero()));
        assert!(updated_bounty.balance.amount.is_zero());
    }

    #[test]
    fn on_already_cancelled_bounty_should_fail() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let bounty = setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                status: BountyStatus::Cancelled,
                ..Bounty::default()
            },
        );

        let err = cancel_bounty_handler(deps.as_mut(), env, info, bounty.id).unwrap_err();

        assert_eq!(err.to_string(), "Error: Bounty is already cancelled");
    }

    #[test]
    fn for_bounty_with_different_owner_should_fail() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let bounty = setup_vault(deps.as_mut(), env.clone(), bounty::default());

        let err = cancel_bounty_handler(
            deps.as_mut(),
            env,
            mock_info("not-the-owner", &[]),
            bounty.id,
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "Unauthorized");
    }

    #[test]
    fn should_delete_the_trigger() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let bounty = setup_bounty(deps.as_mut(), env.clone(), Bounty::default());

        cancel_bounty_handler(deps.as_mut(), env, info, bounty.id).unwrap();

        let updated_bounty = get_bounty_handler(deps.as_ref(), bounty.id).unwrap().bounty;

        assert_ne!(bounty.trigger, None);
        assert_eq!(updated_bounty.trigger, None);
    }

    #[test]
    fn with_escrowed_balance_should_save_disburse_escrow_task() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let bounty = setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                escrowed_amount: Coin::new(ONE.into(), DENOM_UKUJI.to_string()),
                ..Bounty::default()
            },
        );

        cancel_bounty_handler(deps.as_mut(), env.clone(), info, bounty.id).unwrap();

        let disburse_escrow_tasks_before = get_disburse_escrow_tasks(
            deps.as_ref().storage,
            bounty
                .get_expected_execution_completed_date(env.block.time)
                .minus_seconds(10),
            Some(100),
        )
        .unwrap();

        assert!(disburse_escrow_tasks_before.is_empty());

        let disburse_escrow_tasks_after = get_disburse_escrow_tasks(
            deps.as_ref().storage,
            bounty
                .get_expected_execution_completed_date(env.block.time)
                .plus_seconds(10),
            Some(100),
        )
        .unwrap();

        assert!(disburse_escrow_tasks_after.contains(&bounty.id));
    }

    #[test]
    fn should_retract_limit_order() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let order_idx = Uint128::new(123);

        let bounty = setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                trigger: Some(TriggerConfiguration::Price {
                    target_price: Decimal::percent(200),
                    order_idx,
                }),
                ..Bounty::default()
            },
        );

        let response = cancel_bounty_handler(deps.as_mut(), env, info, bounty.id).unwrap();

        let config = get_config(deps.as_ref().storage).unwrap();

        assert_eq!(
            response.messages.get(1).unwrap(),
            &SubMsg::reply_on_error(
                WasmMsg::Execute {
                    contract_addr: config.exchange_contract_address.to_string(),
                    msg: to_json_binary(&ExecuteMsg::RetractOrder {
                        order_idx,
                        denoms: vault.denoms()
                    })
                    .unwrap(),
                    funds: vec![]
                },
                FAIL_SILENTLY_REPLY_ID
            )
        );
    }

    #[test]
    fn should_withdraw_limit_order() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info.clone());

        let order_idx = Uint128::new(123);

        let bounty = setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                trigger: Some(TriggerConfiguration::Price {
                    target_price: Decimal::percent(200),
                    order_idx,
                }),
                ..Bounty::default()
            },
        );

        let response = cancel_bounty_handler(deps.as_mut(), env, info, bounty.id).unwrap();

        let config = get_config(deps.as_ref().storage).unwrap();

        assert_eq!(
            response.messages.get(2).unwrap(),
            &SubMsg::reply_on_error(
                WasmMsg::Execute {
                    contract_addr: config.exchange_contract_address.to_string(),
                    msg: to_json_binary(&ExecuteMsg::WithdrawOrder {
                        order_idx,
                        denoms: vault.denoms()
                    })
                    .unwrap(),
                    funds: vec![]
                },
                FAIL_SILENTLY_REPLY_ID
            )
        );
    }
}
