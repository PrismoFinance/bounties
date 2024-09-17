use crate::types::config::Config;
use crate::types::destination::Destination;
use crate::types::event::Event;
use crate::types::fee_collector::FeeCollector;
use crate::types::performance_assessment_strategy::PerformanceAssessmentStrategyParams;
use crate::types::swap_adjustment_strategy::{
    SwapAdjustmentStrategy, SwapAdjustmentStrategyParams,
};
use crate::types::time_interval::TimeInterval;
use crate::types::vault::{Vault, VaultStatus};
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary, Coin, Decimal, Uint128, Uint64};
use cw20::Cw20ReceiveMsg;
use exchange::msg::Pair;

#[cw_serde]
pub struct InstantiateMsg {
    pub admin: Addr,
    pub executors: Vec<Addr>,
    pub fee_collectors: Vec<FeeCollector>,
    pub automation_fee_percent: Decimal,
    pub paused: bool,
    pub exchange_contract_address: Addr,
}

#[cw_serde]
pub struct MigrateMsg {
    pub admin: Addr,
    pub executors: Vec<Addr>,
    pub fee_collectors: Vec<FeeCollector>,
    pub automation_fee_percent: Decimal,
    pub paused: bool,
    pub exchange_contract_address: Addr,
}

#[cw_serde]
pub enum ExecuteMsg {
    CreateVault {
        owner: Option<Addr>,
        label: Option<String>,
        destinations: Option<Vec<Destination>>,
        target_denom: String,
        route: Option<Binary>,
        slippage_tolerance: Option<Decimal>,
        minimum_receive_amount: Option<Uint128>,
        pay_amount: Uint128,
        time_interval: TimeInterval,
        target_start_time_utc_seconds: Option<Uint64>,
        target_receive_amount: Option<Uint128>,
        performance_assessment_strategy: Option<PerformanceAssessmentStrategyParams>,
        swap_adjustment_strategy: Option<SwapAdjustmentStrategyParams>,
    },
    Deposit {
        address: Addr,
        vault_id: Uint128,
    },
    UpdateVault {
        vault_id: Uint128,
        label: Option<String>,
        destinations: Option<Vec<Destination>>,
        slippage_tolerance: Option<Decimal>,
        minimum_receive_amount: Option<Uint128>,
        time_interval: Option<TimeInterval>,
        swap_adjustment_strategy: Option<SwapAdjustmentStrategyParams>,
        swap_amount: Option<Uint128>,
    },
    CancelVault {
        vault_id: Uint128,
    },
    ExecuteTrigger {
        trigger_id: Uint128,
        route: Option<Binary>,
    },
    UpdateConfig {
        executors: Option<Vec<Addr>>,
        fee_collectors: Option<Vec<FeeCollector>>,
        default_swap_fee_percent: Option<Decimal>,
        weighted_scale_swap_fee_percent: Option<Decimal>,
        automation_fee_percent: Option<Decimal>,
        default_page_limit: Option<u16>,
        paused: Option<bool>,
        risk_weighted_average_escrow_level: Option<Decimal>,
        twap_period: Option<u64>,
        default_slippage_tolerance: Option<Decimal>,
        exchange_contract_address: Option<Addr>,
    },
    UpdateSwapAdjustment {
        strategy: SwapAdjustmentStrategy,
        value: Decimal,
    },
    DisburseEscrow {
        vault_id: Uint128,
    },
    ZDelegate {
        delegator_address: Addr,
        validator_address: Addr,
    },
    Receive(Cw20ReceiveMsg),
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    GetConfig {},
    #[returns(PairsResponse)]
    GetPairs {
        start_after: Option<Pair>,
        limit: Option<u16>,
    },
    #[returns(TriggerIdsResponse)]
    GetTimeTriggerIds { limit: Option<u16> },
    #[returns(TriggerIdResponse)]
    GetTriggerIdByFinLimitOrderIdx { order_idx: Uint128 },
    #[returns(VaultResponse)]
    GetVault { vault_id: Uint128 },
    #[returns(VaultsResponse)]
    GetVaultsByAddress {
        address: Addr,
        status: Option<VaultStatus>,
        start_after: Option<Uint128>,
        limit: Option<u16>,
    },
    #[returns(VaultsResponse)]
    GetVaults {
        start_after: Option<Uint128>,
        limit: Option<u16>,
        reverse: Option<bool>,
    },
    #[returns(EventsResponse)]
    GetEventsByResourceId {
        resource_id: Uint128,
        start_after: Option<u64>,
        limit: Option<u16>,
        reverse: Option<bool>,
    },
    #[returns(EventsResponse)]
    GetEvents {
        start_after: Option<u64>,
        limit: Option<u16>,
        reverse: Option<bool>,
    },
    #[returns(VaultPerformanceResponse)]
    GetVaultPerformance { vault_id: Uint128 },
    #[returns(DisburseEscrowTasksResponse)]
    GetDisburseEscrowTasks { limit: Option<u16> },
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct PairsResponse {
    pub pairs: Vec<Pair>,
}

#[cw_serde]
pub struct TriggerIdResponse {
    pub trigger_id: Uint128,
}

#[cw_serde]
pub struct TriggerIdsResponse {
    pub trigger_ids: Vec<Uint128>,
}

#[cw_serde]
pub struct VaultResponse {
    pub vault: Vault,
}

#[cw_serde]
pub struct VaultPerformanceResponse {
    pub fee: Coin,
    pub factor: Decimal,
}

#[cw_serde]
pub struct VaultsResponse {
    pub vaults: Vec<Vault>,
}

#[cw_serde]
pub struct EventsResponse {
    pub events: Vec<Event>,
}

#[cw_serde]
pub struct CustomFeesResponse {
    pub custom_fees: Vec<(String, Decimal)>,
}

#[cw_serde]
pub struct DisburseEscrowTasksResponse {
    pub vault_ids: Vec<Uint128>,
}
