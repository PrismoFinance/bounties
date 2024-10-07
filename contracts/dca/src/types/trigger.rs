use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Timestamp, Uint128};

#[cw_serde]
pub enum TriggerConfiguration {
    Time {
        target_time: Timestamp,
    },
    EscrowReject {
        target_time: Timestamp,
        bounty_id: Uint128,
        label: Option<String>,
        bounty_description: Option<String>,
        status: Option<BountyStatus>, // Status should be updated to Rejected/Canceled
        mut destinations: Vec<Destination>, // Can we make this a clone of owner so that it just sends the target denom and funds to the bounty issuer (e.g. owner)
        target_denom: String,
        route: Option<Binary>,
        slippage_tolerance: Option<Decimal>,          
    },
    EscrowAccept {
        target_time: Timestamp,
        bounty_id: Uint128, 
        label: Option<String>,
        bounty_description: Option<String>,
        status: Option<BountyStatus>, // Status should be updated to Completed/Finished
        mut destinations: Vec<Destination>, // Can we make this a Bounty Assignee addr so that it just sends the target denom and funds to the bounty assignee? 
        target_denom: String,
        route: Option<Binary>,
        slippage_tolerance: Option<Decimal>,
    },
}

#[cw_serde]
pub struct Trigger {
    pub bounty_id: Uint128,
    pub configuration: TriggerConfiguration,
}
