use super::{config::get_config, triggers::get_trigger};
use crate::{
    helpers::state::fetch_and_increment_counter,
    types::{
        destination::Destination,
        performance_assessment_strategy::PerformanceAssessmentStrategy,
        swap_adjustment_strategy::SwapAdjustmentStrategy,
        time_interval::TimeInterval,
        vault::{Bounty, BountyBuilder, BountyStatus},
    },
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Binary, Coin, Decimal, Order, StdResult, Storage, Timestamp, Uint128};
use cw_storage_plus::{Bound, Index, IndexList, IndexedMap, Item, UniqueIndex};

const BOUNTY_COUNTER: Item<u64> = Item::new("vault_counter_v8");

struct BountyIndexes<'a> {
    pub owner: UniqueIndex<'a, (Addr, u128), BountyData, u128>,
    pub owner_status: UniqueIndex<'a, (Addr, u8, u128), BountyData, u128>,
}

impl<'a> IndexList<BountyData> for BountyIndexes<'a> {
    fn get_indexes(&'_ self) -> Box<dyn Iterator<Item = &'_ dyn Index<BountyData>> + '_> {
        let v: Vec<&dyn Index<BountyData>> = vec![&self.owner, &self.owner_status];
        Box::new(v.into_iter())
    }
}

fn bounty_store<'a>() -> IndexedMap<'a, u128, BountyData, BountyIndexes<'a>> {
    let indexes = BountyIndexes {
        owner: UniqueIndex::new(|v| (v.owner.clone(), v.id.into()), "bounties_v8__owner"),
        owner_status: UniqueIndex::new(
            |v| (v.owner.clone(), v.status.clone() as u8, v.id.into()),
            "bounties_v8__owner_status",
        ),
    };
    IndexedMap::new("bounties_v8", indexes)
}

pub fn migrate_bounty(store: &mut dyn Storage, bounty: Bounty) -> StdResult<()> {
    bounty_store().save(store, bounty.id.into(), &bounty.into())
}

pub fn save_bounty(store: &mut dyn Storage, bounty_builder: BountyBuilder) -> StdResult<Bounty> {
    let bounty = bounty_builder.build(fetch_and_increment_counter(store, BOUNTY_COUNTER)?.into());
    bounty_store().save(store, bounty.id.into(), &bounty.clone().into())?;
    Ok(bounty)
}

pub fn get_bounty(store: &dyn Storage, bounty_id: Uint128) -> StdResult<Bounty> {
    let bounty_data = bounty_store().load(store, bounty_id.into())?;
    bounty_from(store, &bounty_data)
}

pub fn get_bounties_by_address(
    store: &dyn Storage,
    address: Addr,
    status: Option<BountyStatus>,
    start_after: Option<Uint128>,
    limit: Option<u16>,
) -> StdResult<Vec<Bounty>> {
    let partition = match status {
        Some(status) => bounty_store()
            .idx
            .owner_status
            .prefix((address, status as u8)),
        None => bounty_store().idx.owner.prefix(address),
    };

    Ok(partition
        .range(
            store,
            start_after.map(Bound::exclusive),
            None,
            cosmwasm_std::Order::Ascending,
        )
        .take(limit.unwrap_or_else(|| get_config(store).unwrap().default_page_limit) as usize)
        .flat_map(|result| result.map(|(_, bounty_data)| bounty_from(store, &bounty_data)))
        .flatten()
        .collect::<Vec<Bounty>>())
}

pub fn get_bounties(
    store: &dyn Storage,
    start_after: Option<Uint128>,
    limit: Option<u16>,
    reverse: Option<bool>,
) -> StdResult<Vec<Bounty>> {
    Ok(bounty_store()
        .range(
            store,
            start_after.map(Bound::exclusive),
            None,
            reverse.map_or(Order::Ascending, |reverse| match reverse {
                true => Order::Descending,
                false => Order::Ascending,
            }),
        )
        .take(limit.unwrap_or_else(|| get_config(store).unwrap().default_page_limit) as usize)
        .flat_map(|result| result.map(|(_, bounty_data)| bounty_from(store, &bounty_data)))
        .flatten()
        .collect::<Vec<Bounty>>())
}

pub fn update_bounty(store: &mut dyn Storage, bounty: Bounty) -> StdResult<Bounty> {
    bounty_store().save(store, bounty.id.into(), &bounty.clone().into())?;
    Ok(bounty)
}

#[cw_serde]
struct BountyData {
    id: Uint128,
    created_at: Timestamp,
    owner: Addr,
    label: Option<String>,
    destinations: Vec<Destination>,
    status: BountyStatus,
    balance: Coin,
    target_denom: String,
    swap_amount: Uint128,
    route: Option<Binary>,
    slippage_tolerance: Decimal,
    minimum_receive_amount: Option<Uint128>,
    time_interval: TimeInterval,
    started_at: Option<Timestamp>,
    escrow_level: Decimal,
    deposited_amount: Coin,
    swapped_amount: Coin,
    received_amount: Coin,
    escrowed_amount: Coin,
   // performance_assessment_strategy: Option<PerformanceAssessmentStrategy>,
   // swap_adjustment_strategy: Option<SwapAdjustmentStrategy>,
}

impl From<Bounty> for BountyData {
    fn from(bounty: Bounty) -> Self {
        Self {
            id: bounty.id,
            created_at: bounty.created_at,
            owner: bounty.owner,
            label: bounty.label,
            bount_description: bounty.bounty_description,
            status: bounty.status,
            balance: bounty.balance,
            target_denom: bounty.target_denom,
            route: bounty.route,
            destinations: bounty.destinations,
            swap_amount: bounty.swap_amount,
            slippage_tolerance: bounty.slippage_tolerance,
            minimum_receive_amount: bounty.minimum_receive_amount,
            time_interval: bounty.time_interval,
            started_at: bounty.started_at,
            escrow_level: bounty.escrow_level,
            deposited_amount: bounty.deposited_amount,
           // swapped_amount: bounty.swapped_amount,
            received_amount: bounty.received_amount,
            escrowed_amount: bounty.escrowed_amount,
           // performance_assessment_strategy: vault.performance_assessment_strategy,
           // swap_adjustment_strategy: vault.swap_adjustment_strategy,
        }
    }
}

fn bounty_from(store: &dyn Storage, data: &BountyData) -> StdResult<Bounty> {
    let trigger = get_trigger(store, data.id)?.map(|t| t.configuration);

    Ok(Bounty {
        id: data.id,
        created_at: data.created_at,
        owner: data.owner.clone(),
        label: data.label.clone(),
        status: data.status.clone(),
        balance: data.balance.clone(),
        swap_amount: data.swap_amount,
        target_denom: data.target_denom.clone(),
        route: data.route.clone(),
        destinations: data.destinations.clone(),
        slippage_tolerance: data.slippage_tolerance,
        minimum_receive_amount: data.minimum_receive_amount,
        time_interval: data.time_interval.clone(),
        started_at: data.started_at,
        escrow_level: data.escrow_level,
        deposited_amount: data.deposited_amount.clone(),
       // swapped_amount: data.swapped_amount.clone(),
        received_amount: data.received_amount.clone(),
        escrowed_amount: data.escrowed_amount.clone(),
        //performance_assessment_strategy: data.performance_assessment_strategy.clone(),
       // swap_adjustment_strategy: data.swap_adjustment_strategy.clone(),
        trigger,
    })
}
