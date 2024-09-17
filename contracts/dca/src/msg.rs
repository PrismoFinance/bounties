use crate::types::config::Config;
use crate::types::destination::Destination;
use crate::types::event::Event;
use crate::types::fee_collector::FeeCollector;
use crate::types::performance_assessment_strategy::PerformanceAssessmentStrategyParams;
use crate::types::swap_adjustment_strategy::{
    SwapAdjustmentStrategy, SwapAdjustmentStrategyParams,
};
use crate::types::time_interval::TimeInterval;
use crate::types::bounty::{Bounty, BountyStatus};
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
    CreateBounty {
        owner: Option<Addr>,
        label: Option<String>,
        destinations: Option<Vec<Destination>>, // Destination is in types and consists of allocation, address, and msg. 
        target_denom: String,
        route: Option<Binary>,
        slippage_tolerance: Option<Decimal>,
        minimum_receive_amount: Option<Uint128>,
        pay_amount: Uint128,
        time_interval: TimeInterval,
        target_start_time_utc_seconds: Option<Uint64>,
        target_receive_amount: Option<Uint128>,
    },
    Deposit {
        address: Addr,
        bounty_id: Uint128,
    },
    UpdateBounty {
        bounty_id: Uint128,
        label: Option<String>,
        destinations: Option<Vec<Destination>>,
        slippage_tolerance: Option<Decimal>,
        minimum_receive_amount: Option<Uint128>,
        time_interval: Option<TimeInterval>,
        swap_adjustment_strategy: Option<SwapAdjustmentStrategyParams>,
        swap_amount: Option<Uint128>,
    },
    CancelBounty {
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
        bounty_id: Uint128,
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
    #[returns(BountyResponse)]
    GetBounty { vault_id: Uint128 },
    #[returns(BountiesResponse)]
    GetBountiesByAddress {
        address: Addr,
        status: Option<BountyStatus>,
        start_after: Option<Uint128>,
        limit: Option<u16>,
    },
    #[returns(BountiesResponse)]
    GetBounties {
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
    #[returns(BountyPerformanceResponse)]
    GetBountyPerformance { bounty_id: Uint128 },
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
pub struct BountyResponse {
    pub bounty: Bounty,
}

#[cw_serde]
pub struct BountyPerformanceResponse {
    pub fee: Coin,
    pub factor: Decimal,
}

#[cw_serde]
pub struct BountiesResponse {
    pub bounties: Vec<Bounty>,
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
    pub bounty_ids: Vec<Uint128>,
}
