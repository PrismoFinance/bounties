use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Binary, Decimal};


// Where funds should be sent once escrow verification. 
#[cw_serde]
pub struct Destination {
    pub allocation: Decimal,
    pub address: Addr,
    pub msg: Option<Binary>,
}
