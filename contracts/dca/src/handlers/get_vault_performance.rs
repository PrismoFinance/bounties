use crate::{
    helpers::{fees::get_performance_fee, price::get_twap_to_now, vault::get_performance_factor},
    msg::BountyPerformanceResponse,
    state::{config::get_config, bounties::get_bounty},
};
use cosmwasm_std::{Deps, StdError, StdResult, Uint128};

pub fn get_bounty_performance_handler(
    deps: Deps,
    bounty_id: Uint128,
) -> StdResult<BountyPerformanceResponse> {
    let bounty = get_bounty(deps.storage, bounty_id)?;

    let config = get_config(deps.storage)?;

    let current_price = get_twap_to_now(
        &deps.querier,
        config.exchange_contract_address.clone(),
        bounty.get_swap_denom(),
        bounty.target_denom.clone(),
        config.twap_period,
        bounty.route.clone(),
    )?;

    bounty.performance_assessment_strategy.clone().map_or(
        Err(StdError::GenericErr {
            msg: format!(
                "Bounty {} does not have a performance assessment strategy",
                bounty_id
            ),
        }),
        |_| {
            Ok(BountyPerformanceResponse {
                fee: get_performance_fee(&bounty, current_price)?,
                factor: get_performance_factor(&bounty, current_price)?,
            })
        },
    )
}

#[cfg(test)]
mod get_bounty_performance_tests {
    use super::get_bounty_performance_handler;
    use crate::{
        constants::{ONE, TEN},
        tests::{
            helpers::{instantiate_contract, setup_bounty},
            mocks::{calc_mock_dependencies, ADMIN, DENOM_UKUJI, DENOM_UUSK},
        },
        types::{
            performance_assessment_strategy::PerformanceAssessmentStrategy,
            swap_adjustment_strategy::SwapAdjustmentStrategy, bounty::Bounty,
        },
    };
    use cosmwasm_std::{
        testing::{mock_env, mock_info},
        Coin, Decimal, Uint128,
    };

    #[test]
    fn if_bounty_has_no_performance_assessment_strategy_fails() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();

        instantiate_contract(deps.as_mut(), env.clone(), mock_info(ADMIN, &[]));

        let bounty = setup_bounty(deps.as_mut(), env, Bounty::default());

        let err = get_bounty_performance_handler(deps.as_ref(), bounty.id).unwrap_err();

        assert_eq!(
            err.to_string(),
            "Generic error: Bounty 0 does not have a performance assessment strategy"
        );
    }

    #[test]
    fn performance_fee_and_factor_match() {
        let mut deps = calc_mock_dependencies();
        let env = mock_env();

        instantiate_contract(deps.as_mut(), env.clone(), mock_info(ADMIN, &[]));

        let standard_received_amount = TEN - ONE;

        let performance_assessment_strategy = PerformanceAssessmentStrategy::CompareToStandardDca {
            swapped_amount: Coin::new(TEN.into(), DENOM_UKUJI),
            received_amount: Coin::new(standard_received_amount.into(), DENOM_UUSK),
        };

        let bounty = setup_bounty(
            deps.as_mut(),
            env,
            Bounty {
                swapped_amount: Coin::new(TEN.into(), DENOM_UUSK),
                received_amount: Coin::new(TEN.into(), DENOM_UUSK),
                escrowed_amount: Coin::new(TEN.into(), DENOM_UUSK),
                swap_adjustment_strategy: Some(SwapAdjustmentStrategy::default()),
                performance_assessment_strategy: Some(performance_assessment_strategy),
                escrow_level: Decimal::percent(5),
                ..Bounty::default()
            },
        );

        let response = get_bounty_performance_handler(deps.as_ref(), bounty.id).unwrap();

        assert_eq!(
            response.fee,
            Coin::new(
                ((standard_received_amount * response.factor - standard_received_amount)
                    * Decimal::percent(20)
                    + Uint128::one())
                .into(),
                DENOM_UUSK
            )
        );
    }
}
