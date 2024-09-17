use crate::{msg::TriggerIdResponse, state::triggers::trigger_store};
#[cfg(not(feature = "library"))]
use cosmwasm_std::Uint128;
use cosmwasm_std::{Deps, StdError, StdResult};

pub fn get_trigger_id_by_fin_limit_order_idx_handler(
    deps: Deps,
    order_idx: Uint128,
) -> StdResult<TriggerIdResponse> {
    let trigger_id = trigger_store()
        .idx
        .order_idx
        .item(deps.storage, order_idx.into())?
        .map(|(_, trigger)| trigger.bounty_id);

    if let Some(trigger_id) = trigger_id {
        Ok(TriggerIdResponse { trigger_id })
    } else {
        Err(StdError::generic_err("Trigger not found"))
    }
}

#[cfg(test)]
mod get_trigger_id_by_fin_limit_order_idx_handler_tests {
    use super::get_trigger_id_by_fin_limit_order_idx_handler;
    use crate::{
        state::triggers::save_trigger,
        types::trigger::{Trigger, TriggerConfiguration},
    };
    use cosmwasm_std::{testing::mock_dependencies, Decimal, Uint128};

    #[test]
    fn returns_trigger_id() {
        let mut deps = mock_dependencies();

        let order_idx = Uint128::new(89);

        save_trigger(
            deps.as_mut().storage,
            Trigger {
                bounty_id: Uint128::one(),
                configuration: TriggerConfiguration::Price {
                    target_price: Decimal::percent(200),
                    order_idx,
                },
            },
        )
        .unwrap();

        let response =
            get_trigger_id_by_fin_limit_order_idx_handler(deps.as_ref(), order_idx).unwrap();

        assert_eq!(response.trigger_id, Uint128::one());
    }

    #[test]
    fn returns_error_if_trigger_not_found() {
        let mut deps = mock_dependencies();

        let order_idx = Uint128::new(89);

        save_trigger(
            deps.as_mut().storage,
            Trigger {
                vault_id: Uint128::one(),
                configuration: TriggerConfiguration::Price {
                    target_price: Decimal::percent(200),
                    order_idx,
                },
            },
        )
        .unwrap();

        let err = get_trigger_id_by_fin_limit_order_idx_handler(
            deps.as_ref(),
            order_idx + Uint128::one(),
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "Generic error: Trigger not found");
    }
}
