use cosmwasm_std::{Decimal, Uint128};

pub const AFTER_LIMIT_ORDER_PLACED_REPLY_ID: u64 = 1;
// pub const AFTER_SWAP_REPLY_ID: u64 = 2;
pub const AFTER_FAILED_AUTOMATION_REPLY_ID: u64 = 3;
pub const AFTER_DELEGATION_REPLY_ID: u64 = 4;
pub const AFTER_ORDER_MIGRATION_REPLY_ID: u64 = 5;
pub const FAIL_SILENTLY_REPLY_ID: u64 = 6;

// pub const SWAP_FEE_RATE: &str = "0.0015";

pub const ONE_MICRON: Uint128 = Uint128::new(1);
pub const TWO_MICRONS: Uint128 = Uint128::new(2);
pub const TEN_MICRONS: Uint128 = Uint128::new(10);
pub const ONE: Uint128 = Uint128::new(1000000);
pub const TEN: Uint128 = Uint128::new(10000000);
pub const ONE_HUNDRED: Uint128 = Uint128::new(100000000);
pub const ONE_THOUSAND: Uint128 = Uint128::new(1000000000);

pub const HALF_DECIMAL: Decimal = Decimal::new(Uint128::new(500000000000000000));
pub const ONE_DECIMAL: Decimal = Decimal::new(Uint128::new(1000000000000000000));
pub const ONE_AND_HALF_DECIMAL: Decimal = Decimal::new(Uint128::new(1500000000000000000));
pub const TWO_DECIMAL: Decimal = Decimal::new(Uint128::new(2000000000000000000));
pub const THREE_DECIMAL: Decimal = Decimal::new(Uint128::new(3000000000000000000));
pub const TEN_DECIMAL: Decimal = Decimal::new(Uint128::new(10000000000000000000));

pub const PAIR_CONTRACT_ADDRESS: &str = "pair-contract";
pub const EXCHANGE_CONTRACT_ADDRESS: &str = "swap-contract";
