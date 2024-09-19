use cosmwasm_schema::cw_serde;
use cosmwasm_std::{BlockInfo, Coin, Decimal, SubMsg, Timestamp, Uint128};

use super::update::Update;

#[cw_serde]
pub enum ExecutionSkippedReason {
    SlippageToleranceExceeded,
    PriceThresholdExceeded { price: Decimal },
    SwapAmountAdjustedToZero,
    SlippageQueryError,
    UnknownError { msg: String },
}

#[cw_serde]
pub enum EventData {
    BountyFundsDeposited {
        amount: Coin,
    },
    BountyExecutionTriggered {
        base_denom: String,
        quote_denom: String,
        asset_price: Decimal,
    },
    BountyExecutionCompleted {
        sent: Coin,
        received: Coin,
        fee: Coin,
    },
    SimulatedBountyExecutionCompleted {
        sent: Coin,
        received: Coin,
        fee: Coin,
    },
    BountyExecutionSkipped {
        reason: ExecutionSkippedReason,
    },
    SimulatedBountyExecutionSkipped {
        reason: ExecutionSkippedReason,
    },
    BountyCancelled {},
    BountyEscrowDisbursed {
        amount_disbursed: Coin,
        performance_fee: Coin,
    },
    BountyPostExecutionActionFailed {
        msg: SubMsg,
        funds: Vec<Coin>,
    },
    BountyUpdated {
        updates: Vec<Update>,
    },
}

#[cw_serde]
pub struct Event {
    pub id: u64,
    pub resource_id: Uint128,
    pub timestamp: Timestamp,
    pub block_height: u64,
    pub data: EventData,
}

#[derive(Clone)]
pub struct EventBuilder {
    resource_id: Uint128,
    timestamp: Timestamp,
    block_height: u64,
    data: EventData,
}

impl EventBuilder {
    pub fn new(resource_id: Uint128, block: BlockInfo, data: EventData) -> EventBuilder {
        EventBuilder {
            resource_id,
            timestamp: block.time,
            block_height: block.height,
            data,
        }
    }

    pub fn build(self, id: u64) -> Event {
        Event {
            id,
            resource_id: self.resource_id,
            timestamp: self.timestamp,
            block_height: self.block_height,
            data: self.data,
        }
    }
}
