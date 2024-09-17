use crate::state::bounties::get_bounties_by_address as fetch_bounties_by_address;
use crate::types::bounty::BountyStatus;
use crate::{helpers::validation::assert_page_limit_is_valid, msg::BountiesResponse};
use cosmwasm_std::{Addr, Deps, StdResult, Uint128};

pub fn get_bounties_by_address_handler(
    deps: Deps,
    address: Addr,
    status: Option<BountyStatus>,
    start_after: Option<Uint128>,
    limit: Option<u16>,
) -> StdResult<BountiesResponse> {
    deps.api.addr_validate(address.as_ref())?;
    assert_page_limit_is_valid(limit)?;

    let bounties = fetch_bounties_by_address(deps.storage, address, status, start_after, limit)?;

    Ok(BountiesResponse { bounties })
}

#[cfg(test)]
mod get_bounties_by_address_tests {
    use crate::contract::query;
    use crate::msg::{QueryMsg, VaultsResponse};
    use crate::tests::helpers::{instantiate_contract, setup_vault};
    use crate::tests::mocks::ADMIN;
    use crate::types::bounty::{Bounty, BountyStatus};
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{from_json, Uint128};

    #[test]
    fn with_no_bounties_should_return_all_bounties() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let bounties = from_json::<BountiesResponse>(
            &query(
                deps.as_ref(),
                env,
                QueryMsg::GetBountiesByAddress {
                    address: Bounty::default().owner,
                    status: None,
                    start_after: None,
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap()
        .bounties;

        assert_eq!(bounties.len(), 0);
    }

    #[test]
    fn with_multiple_bounties_should_return_all_bounties() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        setup_bounty(deps.as_mut(), env.clone(), Bounty::default());
        setup_bounty(deps.as_mut(), env.clone(), Bounty::default());

        let bounties = from_json::<BountiesResponse>(
            &query(
                deps.as_ref(),
                env,
                QueryMsg::GetBountiesByAddress {
                    address: Bounty::default().owner,
                    status: None,
                    start_after: None,
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap()
        .bounties;

        assert_eq!(bounties.len(), 2);
    }

    #[test]
    fn with_limit_should_return_limited_bounties() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        for _ in 0..40 {
            setup_bounty(deps.as_mut(), env.clone(), Bounty::default());
        }

        let bounties = from_json::<BountiesResponse>(
            &query(
                deps.as_ref(),
                env,
                QueryMsg::GetBountiesByAddress {
                    address: Bounty::default().owner,
                    status: None,
                    start_after: None,
                    limit: Some(30),
                },
            )
            .unwrap(),
        )
        .unwrap()
        .vaults;

        assert_eq!(bounties.len(), 30);
        assert_eq!(bounties[0].id, Uint128::new(0));
    }

    #[test]
    fn with_start_after_should_return_bounties_after_start_after() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        setup_bounty(deps.as_mut(), env.clone(), Bounty::default());
        setup_bounty(deps.as_mut(), env.clone(), Bounty::default());

        let bounties = from_json::<BountiesResponse>(
            &query(
                deps.as_ref(),
                env,
                QueryMsg::GetBountiesByAddress {
                    address: Bounty::default().owner,
                    status: None,
                    start_after: Some(Uint128::zero()),
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap()
        .bounties;

        assert_eq!(bounties.len(), 1);
        assert_eq!(bounties[0].id, Uint128::new(1));
    }

    #[test]
    fn with_limit_and_start_after_should_return_limited_bounties_after_start_after() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        for _ in 0..40 {
            setup_bounty(deps.as_mut(), env.clone(), Bounty::default());
        }

        let bounties = from_json::<BountiesResponse>(
            &query(
                deps.as_ref(),
                env,
                QueryMsg::GetBountiesByAddress {
                    address: Bounty::default().owner,
                    status: None,
                    start_after: Some(Uint128::one()),
                    limit: Some(30),
                },
            )
            .unwrap(),
        )
        .unwrap()
        .bounties;

        assert_eq!(bounties.len(), 30);
        assert_eq!(bounties[0].id, Uint128::new(2));
    }

    #[test]
    fn with_limit_too_large_should_fail() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        let err = query(
            deps.as_ref(),
            env,
            QueryMsg::GetBountiesByAddress {
                address: Bounty::default().owner,
                status: None,
                start_after: Some(Uint128::one()),
                limit: Some(10000),
            },
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Generic error: limit cannot be greater than 1000."
        )
    }

    #[test]
    fn with_status_filter_should_return_all_bounties_with_status() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info(ADMIN, &[]);

        instantiate_contract(deps.as_mut(), env.clone(), info);

        setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                status: BountyStatus::Active,
                ..Bounty::default()
            },
        );

        setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                status: BountyStatus::Active,
                ..Bounty::default()
            },
        );

        setup_bounty(
            deps.as_mut(),
            env.clone(),
            Bounty {
                status: BountyStatus::Scheduled,
                ..Bounty::default()
            },
        );

        let bounties = from_json::<BountiesResponse>(
            &query(
                deps.as_ref(),
                env,
                QueryMsg::GetBountiesByAddress {
                    address: Bounty::default().owner,
                    status: Some(BountyStatus::Active),
                    start_after: None,
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap()
        .vaults;

        assert_eq!(bounties.len(), 2);
        bounties
            .iter()
            .for_each(|v| assert!(v.status == BountyStatus::Active));
    }
}
