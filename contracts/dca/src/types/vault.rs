use super::{
    destination::Destination,
     time_interval::TimeInterval,
    trigger::TriggerConfiguration,
};
use crate::helpers::time::get_total_execution_duration;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    Addr, Binary, Coin, Decimal, Decimal256, StdResult, Timestamp, Uint128, Uint256,
};
use std::cmp::max;

#[cw_serde]
pub enum BountyStatus {
    Scheduled,
    Active,
    Inactive,
    Cancelled,
}

#[cw_serde]
pub struct Bounty {
    pub id: Uint128,
    pub created_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub owner: Addr,
    pub label: Option<String>,
    pub destinations: Vec<Destination>,
    pub status: VaultStatus,
    pub balance: Coin,
    pub target_denom: String,
    pub route: Option<Binary>,
    pub slippage_tolerance: Decimal,
    pub minimum_receive_amount: Option<Uint128>,
    pub time_interval: TimeInterval,
    pub escrow_level: Decimal,
    pub deposited_amount: Coin,
    pub received_amount: Coin,
    pub escrowed_amount: Coin,
    pub trigger: Option<TriggerConfiguration>
}

impl Bounty {
    pub fn denoms(&self) -> [String; 2] {
        [self.get_swap_denom(), self.target_denom.clone()]
    }

    pub fn get_swap_denom(&self) -> String {
        self.balance.denom.clone()
    }

    pub fn get_expected_execution_completed_date(&self, current_time: Timestamp) -> Timestamp {
        let remaining_balance = match self.performance_assessment_strategy.clone() {
            Some(PerformanceAssessmentStrategy::CompareToStandardDca {
                swapped_amount, ..
            }) => max(
                self.deposited_amount.amount - swapped_amount.amount,
                self.balance.amount,
            ),
            _ => self.balance.amount,
        };

        let execution_duration = get_total_execution_duration(
            current_time,
            remaining_balance
                .checked_div(self.swap_amount)
                .unwrap()
                .into(),
            &self.time_interval,
        );

        current_time.plus_seconds(
            execution_duration
                .num_seconds()
                .try_into()
                .expect("executed duration should be >= 0 seconds"),
        )
    }

    pub fn price_threshold_exceeded(&self, belief_price: Decimal) -> StdResult<bool> {
        self.minimum_receive_amount
            .map_or(Ok(false), |minimum_receive_amount| {
                let swap_amount_as_decimal =
                    Decimal256::from_ratio(self.swap_amount, Uint256::one());

                let expected_receive_amount_at_price = swap_amount_as_decimal
                    .checked_div(belief_price.into())
                    .expect("belief price should be larger than 0");

                let minimum_receive_amount_as_decimal =
                    Decimal256::from_ratio(minimum_receive_amount, Uint256::one());

                Ok(expected_receive_amount_at_price < minimum_receive_amount_as_decimal)
            })
    }

    pub fn is_active(&self) -> bool {
        self.status == BountyStatus::Active
    }

    pub fn is_scheduled(&self) -> bool {
        self.status == BountyStatus::Scheduled
    }

    pub fn is_inactive(&self) -> bool {
        self.status == BountyStatus::Inactive
    }

    pub fn should_not_continue(&self) -> bool {
        self.is_inactive()
            && self.performance_assessment_strategy.clone().map_or(
                true,
                |performance_assessment_strategy| {
                    !performance_assessment_strategy.should_continue(self)
                },
            )
    }

    pub fn is_cancelled(&self) -> bool {
        self.status == VaultStatus::Cancelled
    }
}

pub struct BountyBuilder {
    pub id: Uint128,
    pub created_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub owner: Addr,
    pub label: Option<String>,
    pub destinations: Vec<Destination>,
    pub status: VaultStatus,
    pub balance: Coin,
    pub target_denom: String,
    pub route: Option<Binary>,
    pub slippage_tolerance: Decimal,
    pub minimum_receive_amount: Option<Uint128>,
    pub time_interval: TimeInterval,
    pub escrow_level: Decimal,
    pub deposited_amount: Coin,
    pub received_amount: Coin,
    pub escrowed_amount: Coin,
    pub trigger: Option<TriggerConfiguration>
}

impl BountyBuilder {
    pub fn new(
    id: Uint128,
    created_at: Timestamp,
    started_at: Option<Timestamp>,
    owner: Addr,
    label: Option<String>,
    destinations: Vec<Destination>,
    status: BountyStatus,
    balance: Coin,
    target_denom: String,
    route: Option<Binary>,
    slippage_tolerance: Decimal,
    minimum_receive_amount: Option<Uint128>,
    time_interval: TimeInterval,
    escrow_level: Decimal,
    deposited_amount: Coin,
    received_amount: Coin,
    escrowed_amount: Coin,
    trigger: Option<TriggerConfiguration>
    ) -> BountyBuilder {
        BountyBuilder {
            id,
            created_at,
            started_at, 
            owner,
            label,
            destinations,
            status,
            balance,
            target_denom,
            route,
            slippage_tolerance,
            minimum_receive_amount,
            time_interval,
            escrow_level,
            deposited_amount,
            received_amount,
            escrowed_amount,
        }
    }

    pub fn build(self, id: Uint128) -> Bounty {
        Bounty {
            id,
            created_at: self.created_at,
            started_at: self.started_at,
            owner: self.owner,
            label: self.label,
            destinations: self.destinations,
            status: self.status,
            balance: self.balance.clone(),
            target_denom: self.target_denom,
            route: self.route,
            slippage_tolerance: self.slippage_tolerance,
            minimum_receive_amount: self.minimum_receive_amount,
            time_interval: self.time_interval,
            escrow_level: self.escrow_level,
            deposited_amount: self.deposited_amount,
            received_amount: self.received_amount,
            escrowed_amount: self.escrowed_amount,
            trigger: None,
        }
    }
}

#[cfg(test)]
mod should_not_continue_tests {
    use crate::{
        constants::{ONE, TEN},
        tests::mocks::DENOM_UKUJI,
        types::{
            performance_assessment_strategy::PerformanceAssessmentStrategy,
            vault::{Vault, VaultStatus},
        },
    };
    use cosmwasm_std::Coin;

    #[test]
    fn when_regular_vault_is_active_is_false() {
        let vault = Vault::default();

        assert!(!vault.should_not_continue());
    }

    #[test]
    fn when_regular_vault_is_inactive_is_true() {
        let vault = Vault {
            status: VaultStatus::Inactive,
            ..Default::default()
        };

        assert!(vault.should_not_continue());
    }

    #[test]
    fn when_dca_vault_is_active_is_false() {
        let vault = Vault {
            performance_assessment_strategy: Some(Default::default()),
            ..Default::default()
        };

        assert!(!vault.should_not_continue());
    }

    #[test]
    fn when_dca_vault_is_inactive_and_standard_dca_is_active_is_false() {
        let vault = Vault {
            status: VaultStatus::Inactive,
            deposited_amount: Coin::new(TEN.into(), DENOM_UKUJI),
            performance_assessment_strategy: Some(
                PerformanceAssessmentStrategy::CompareToStandardDca {
                    swapped_amount: Coin::new((TEN - ONE).into(), DENOM_UKUJI),
                    received_amount: Coin::new((TEN - ONE).into(), DENOM_UKUJI),
                },
            ),
            ..Default::default()
        };

        assert!(!vault.should_not_continue());
    }

    #[test]
    fn when_dca_vault_is_inactive_and_standard_dca_is_inactive_is_true() {
        let vault = Vault {
            status: VaultStatus::Inactive,
            deposited_amount: Coin::new(TEN.into(), DENOM_UKUJI),
            performance_assessment_strategy: Some(
                PerformanceAssessmentStrategy::CompareToStandardDca {
                    swapped_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                    received_amount: Coin::new(TEN.into(), DENOM_UKUJI),
                },
            ),
            ..Default::default()
        };

        assert!(vault.should_not_continue());
    }
}

#[cfg(test)]
mod get_expected_execution_completed_date_tests {
    use super::Vault;
    use crate::{
        constants::{ONE, TEN},
        tests::mocks::DENOM_UKUJI,
        types::{
            performance_assessment_strategy::PerformanceAssessmentStrategy, vault::VaultStatus,
        },
    };
    use cosmwasm_std::{testing::mock_env, Coin};

    #[test]
    fn expected_execution_end_date_is_now_when_vault_is_empty() {
        let env = mock_env();
        let vault = Vault {
            balance: Coin::new(0, DENOM_UKUJI),
            ..Vault::default()
        };

        assert_eq!(
            vault.get_expected_execution_completed_date(env.block.time),
            env.block.time
        );
    }

    #[test]
    fn expected_execution_end_date_is_in_future_when_vault_is_not_empty() {
        let env = mock_env();
        let vault = Vault::default();

        assert_eq!(
            vault.get_expected_execution_completed_date(env.block.time),
            env.block.time.plus_seconds(1000 / 100 * 24 * 60 * 60)
        );
    }

    #[test]
    fn expected_execution_end_date_is_at_end_of_standard_dca_execution() {
        let env = mock_env();
        let vault = Vault {
            status: VaultStatus::Inactive,
            balance: Coin::new(ONE.into(), DENOM_UKUJI),
            swap_amount: ONE,
            performance_assessment_strategy: Some(
                PerformanceAssessmentStrategy::CompareToStandardDca {
                    swapped_amount: Coin::new(ONE.into(), DENOM_UKUJI),
                    received_amount: Coin::new(ONE.into(), DENOM_UKUJI),
                },
            ),
            ..Vault::default()
        };

        assert_eq!(
            vault.get_expected_execution_completed_date(env.block.time),
            env.block.time.plus_seconds(9 * 24 * 60 * 60)
        );
    }

    #[test]
    fn expected_execution_end_date_is_at_end_of_performance_assessment() {
        let env = mock_env();
        let vault = Vault {
            balance: Coin::new((TEN - ONE).into(), DENOM_UKUJI),
            swap_amount: ONE,
            performance_assessment_strategy: Some(
                PerformanceAssessmentStrategy::CompareToStandardDca {
                    swapped_amount: Coin::new((ONE + ONE + ONE).into(), DENOM_UKUJI),
                    received_amount: Coin::new((ONE + ONE + ONE).into(), DENOM_UKUJI),
                },
            ),
            ..Vault::default()
        };

        assert_eq!(
            vault.get_expected_execution_completed_date(env.block.time),
            env.block.time.plus_seconds(9 * 24 * 60 * 60)
        );
    }
}

#[cfg(test)]
mod price_threshold_exceeded_tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn should_not_be_exceeded_when_price_is_below_threshold() {
        let vault = Vault {
            swap_amount: Uint128::new(100),
            minimum_receive_amount: Some(Uint128::new(50)),
            ..Vault::default()
        };

        assert_eq!(
            vault.price_threshold_exceeded(Decimal::from_str("1.9").unwrap()),
            Ok(false)
        );
    }

    #[test]
    fn should_not_be_exceeded_when_price_equals_threshold() {
        let vault = Vault {
            swap_amount: Uint128::new(100),
            minimum_receive_amount: Some(Uint128::new(50)),
            ..Vault::default()
        };

        assert_eq!(
            vault.price_threshold_exceeded(Decimal::from_str("2.0").unwrap()),
            Ok(false)
        );
    }

    #[test]
    fn should_be_exceeded_when_price_is_above_threshold() {
        let vault = Vault {
            swap_amount: Uint128::new(100),
            minimum_receive_amount: Some(Uint128::new(50)),
            ..Vault::default()
        };

        assert_eq!(
            vault.price_threshold_exceeded(Decimal::from_str("2.1").unwrap()),
            Ok(true)
        );
    }
}
