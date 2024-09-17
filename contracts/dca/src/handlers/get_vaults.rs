use crate::state::bounties::get_bounties;
use crate::{helpers::validation::assert_page_limit_is_valid, msg::BountiesResponse};
use cosmwasm_std::{Deps, StdResult, Uint128};

pub fn get_bounties_handler(
    deps: Deps,
    start_after: Option<Uint128>,
    limit: Option<u16>,
    reverse: Option<bool>,
) -> StdResult<BountiesResponse> {
    assert_page_limit_is_valid(limit)?;

    let bounties = get_bounties(deps.storage, start_after, limit, reverse)?;

    Ok(BountiesResponse { bounties })
}

#[cfg(test)]
mod get_bounties_tests {
    use super::*;
    use crate::tests::helpers::{instantiate_contract, setup_bounty};
    use crate::tests::mocks::ADMIN;
    use crate::types::bounty::Bounty;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::Uint128;

    #[test]
    fn with_limit_too_large_should_fail() {
        let mut deps = mock_dependencies();

        instantiate_contract(deps.as_mut(), mock_env(), mock_info(ADMIN, &[]));

        let err = get_bounties_handler(deps.as_ref(), None, Some(1001), None).unwrap_err();

        assert_eq!(
            err.to_string(),
            "Generic error: limit cannot be greater than 1000."
        );
    }

    #[test]
    fn with_no_bounties_should_return_all_bounties() {
        let mut deps = mock_dependencies();

        instantiate_contract(deps.as_mut(), mock_env(), mock_info(ADMIN, &[]));

        let bounties = get_bounties_handler(deps.as_ref(), None, None, None)
            .unwrap()
            .bounties;

        assert_eq!(bounties.len(), 0);
    }

    #[test]
    fn with_multiple_bounties_should_return_all_bounties() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        instantiate_contract(deps.as_mut(), env.clone(), mock_info(ADMIN, &[]));

        setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                id: Uint128::new(1),
                ..Bounty::default()
            },
        );

        setup_bounty(
            deps.as_mut(),
            env,
            Bounty {
                id: Uint128::new(2),
                ..Bounty::default()
            },
        );

        let bounties = get_bounties_handler(deps.as_ref(), None, None, None)
            .unwrap()
            .bounties;

        assert_eq!(bounties.len(), 2);
    }

    #[test]
    fn with_one_bounty_should_return_proper_bounty_data() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        instantiate_contract(deps.as_mut(), env.clone(), mock_info(ADMIN, &[]));

        let bounty = setup_bounty(deps.as_mut(), env, Bounty::default());

        let bounties = get_bounties_handler(deps.as_ref(), None, None, None)
            .unwrap()
            .bounties;

        assert_eq!(bounties.first().unwrap(), &bounty);
    }

    #[test]
    fn with_limit_should_return_limited_bounties() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        instantiate_contract(deps.as_mut(), env.clone(), mock_info(ADMIN, &[]));

        for i in 1..40 {
            setup_bounty(
                deps.as_mut(),
                env.clone(),
                Bounty {
                    id: Uint128::new(i),
                    ..Bounty::default()
                },
            );
        }

        let bounties = get_bounties_handler(deps.as_ref(), None, Some(30), None)
            .unwrap()
            .bounties;

        assert_eq!(bounties.len(), 30);
        assert_eq!(bounties[0].id, Uint128::new(1));
    }

    #[test]
    fn with_start_after_should_return_bounties_after_start_after() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        instantiate_contract(deps.as_mut(), env.clone(), mock_info(ADMIN, &[]));

        setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                id: Uint128::new(1),
                ..Bounty::default()
            },
        );

        setup_bounty(
            deps.as_mut(),
            env,
            Bounty {
                id: Uint128::new(2),
                ..Bounty::default()
            },
        );

        let bounties = get_bounties_handler(deps.as_ref(), Some(Uint128::one()), None, None)
            .unwrap()
            .bounties;

        assert_eq!(bounties.len(), 1);
        assert_eq!(bounties[0].id, Uint128::new(2));
    }

    #[test]
    fn with_limit_and_start_after_should_return_limited_bounties_after_start_after() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        instantiate_contract(deps.as_mut(), env.clone(), mock_info(ADMIN, &[]));

        for i in 1..40 {
            setup_bounty(
                deps.as_mut(),
                env.clone(),
                Bounty {
                    id: Uint128::new(i),
                    ..Bounty::default()
                },
            );
        }

        let bounties = get_bounties_handler(deps.as_ref(), Some(Uint128::one()), Some(30), None)
            .unwrap()
            .bounties;

        assert_eq!(bounties.len(), 30);
        assert_eq!(bounties[0].id, Uint128::new(2));
    }
}
