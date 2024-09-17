use crate::types::trigger::{Trigger, TriggerConfiguration};
use cosmwasm_std::{Order, StdResult, Storage, Timestamp, Uint128};
use cw_storage_plus::{Bound, Index, IndexList, IndexedMap, MultiIndex, UniqueIndex};
use std::marker::PhantomData;

pub(crate) struct TriggerIndexes<'a> {
    pub due_date: MultiIndex<'a, u64, Trigger, u128>,
    pub order_idx: UniqueIndex<'a, u128, Trigger, u128>,
}

impl<'a> IndexList<Trigger> for TriggerIndexes<'a> {
    fn get_indexes(&'_ self) -> Box<dyn Iterator<Item = &'_ dyn Index<Trigger>> + '_> {
        let v: Vec<&dyn Index<Trigger>> = vec![&self.due_date, &self.order_idx];
        Box::new(v.into_iter())
    }
}

pub(crate) fn trigger_store<'a>() -> IndexedMap<'a, u128, Trigger, TriggerIndexes<'a>> {
    let indexes = TriggerIndexes {
        due_date: MultiIndex::new(
            |_, trigger| match trigger.configuration {
                TriggerConfiguration::Time { target_time } => target_time.seconds(),
                _ => u64::MAX,
            },
            "triggers_v8",
            "triggers_v8__due_date",
        ),
        order_idx: UniqueIndex::new(
            |trigger| match trigger.configuration {
                TriggerConfiguration::Price { order_idx, .. } => order_idx.into(),
                _ => u128::MAX - trigger.bounty_id.u128(), // allows a unique entry that will never be found via an order_idx
            },
            "triggers_v8__order_idx",
        ),
    };
    IndexedMap::new("triggers_v8", indexes)
}

pub fn save_trigger(store: &mut dyn Storage, trigger: Trigger) -> StdResult<()> {
    trigger_store().save(store, trigger.bounty_id.into(), &trigger)
}

pub fn get_trigger(store: &dyn Storage, bounty_id: Uint128) -> StdResult<Option<Trigger>> {
    trigger_store().may_load(store, bounty_id.into())
}

pub fn delete_trigger(store: &mut dyn Storage, bounty_id: Uint128) -> StdResult<()> {
    trigger_store().remove(store, bounty_id.into())
}

pub fn get_time_triggers(
    store: &dyn Storage,
    due_before: Timestamp,
    limit: Option<u16>,
) -> StdResult<Vec<Uint128>> {
    Ok(trigger_store()
        .idx
        .due_date
        .range(
            store,
            None,
            Some(Bound::Inclusive((
                (due_before.seconds(), u128::MAX),
                PhantomData,
            ))),
            Order::Ascending,
        )
        .take(limit.unwrap_or(30) as usize)
        .flat_map(|result| result.map(|(_, trigger)| trigger.bounty_id))
        .collect::<Vec<Uint128>>())
}

pub fn get_trigger_by_order_idx(
    store: &dyn Storage,
    order_idx: Uint128,
) -> StdResult<Option<Trigger>> {
    trigger_store()
        .idx
        .order_idx
        .item(store, order_idx.into())
        .map(|result| result.map(|(_, trigger)| trigger))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::trigger::TriggerConfiguration;
    use cosmwasm_std::testing::{mock_dependencies, mock_env};
    use cosmwasm_std::{Decimal, Uint128};

    #[test]
    fn fetches_trigger_ids_for_time_triggers_that_are_due() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let trigger = Trigger {
            bounty_id: Uint128::from(1u128),
            configuration: TriggerConfiguration::Time {
                target_time: env.block.time,
            },
        };

        save_trigger(&mut deps.storage, trigger.clone()).unwrap();

        let trigger_ids =
            get_time_triggers(&deps.storage, env.block.time.plus_seconds(10), Some(100)).unwrap();

        assert_eq!(trigger_ids, vec![trigger.bounty_id]);
    }

    #[test]
    fn does_not_fetch_trigger_ids_for_time_triggers_that_are_not_due() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let trigger = Trigger {
            bounty_id: Uint128::from(1u128),
            configuration: TriggerConfiguration::Time {
                target_time: env.block.time.plus_seconds(10),
            },
        };

        save_trigger(&mut deps.storage, trigger).unwrap();

        let trigger_ids = get_time_triggers(&deps.storage, env.block.time, Some(100)).unwrap();

        assert!(trigger_ids.is_empty());
    }

    #[test]
    fn stores_and_fetches_separate_time_triggers_with_the_same_target_time() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let trigger_1 = Trigger {
            bounty_id: Uint128::from(1u128),
            configuration: TriggerConfiguration::Time {
                target_time: env.block.time,
            },
        };
        let trigger_2 = Trigger {
            bounty_id: Uint128::from(2u128),
            configuration: TriggerConfiguration::Time {
                target_time: env.block.time,
            },
        };

        save_trigger(&mut deps.storage, trigger_1.clone()).unwrap();
        save_trigger(&mut deps.storage, trigger_2.clone()).unwrap();

        let trigger_ids = get_time_triggers(&deps.storage, env.block.time, Some(100)).unwrap();

        assert_eq!(trigger_ids, vec![trigger_1.bounty_id, trigger_2.bounty_id]);
    }

    #[test]
    fn deletes_trigger_by_bounty_id() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let trigger = Trigger {
            bounty_id: Uint128::from(1u128),
            configuration: TriggerConfiguration::Time {
                target_time: env.block.time,
            },
        };

        save_trigger(&mut deps.storage, trigger.clone()).unwrap();

        let trigger_ids_before_delete =
            get_time_triggers(&deps.storage, env.block.time, Some(100)).unwrap();

        delete_trigger(&mut deps.storage, trigger.bounty_id).unwrap();

        let trigger_ids_after_delete =
            get_time_triggers(&deps.storage, env.block.time, Some(100)).unwrap();

        assert_eq!(trigger_ids_before_delete, vec![trigger.bounty_id]);
        assert!(trigger_ids_after_delete.is_empty());
    }

    #[test]
    fn overwrites_existing_trigger() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let trigger_1 = Trigger {
            bounty_id: Uint128::from(1u128),
            configuration: TriggerConfiguration::Time {
                target_time: env.block.time,
            },
        };

        let trigger_2 = Trigger {
            bounty_id: Uint128::from(1u128),
            configuration: TriggerConfiguration::Time {
                target_time: env.block.time.plus_seconds(10),
            },
        };

        save_trigger(&mut deps.storage, trigger_1.clone()).unwrap();

        let trigger_ids = get_time_triggers(&deps.storage, env.block.time, Some(100)).unwrap();
        assert_eq!(trigger_ids, vec![trigger_1.bounty_id]);

        save_trigger(&mut deps.storage, trigger_2.clone()).unwrap();

        let trigger_ids =
            get_time_triggers(&deps.storage, env.block.time.plus_seconds(20), Some(100)).unwrap();
        assert_eq!(trigger_ids, vec![trigger_2.bounty_id]);
    }

    #[test]
    fn keeps_other_triggers_when_deleting_trigger_by_bounty_id() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let trigger_1 = Trigger {
            bounty_id: Uint128::from(1u128),
            configuration: TriggerConfiguration::Time {
                target_time: env.block.time,
            },
        };
        let trigger_2 = Trigger {
            bounty_id: Uint128::from(2u128),
            configuration: TriggerConfiguration::Time {
                target_time: env.block.time,
            },
        };

        save_trigger(&mut deps.storage, trigger_1.clone()).unwrap();
        save_trigger(&mut deps.storage, trigger_2.clone()).unwrap();

        let trigger_ids_before_delete =
            get_time_triggers(&deps.storage, env.block.time, Some(100)).unwrap();

        delete_trigger(&mut deps.storage, trigger_1.bounty_id).unwrap();

        let trigger_ids_after_delete =
            get_time_triggers(&deps.storage, env.block.time, Some(100)).unwrap();

        assert_eq!(
            trigger_ids_before_delete,
            vec![trigger_1.vault_id, trigger_2.bounty_id]
        );
        assert_eq!(trigger_ids_after_delete, vec![trigger_2.bounty_id]);
    }

    #[test]
    fn fetches_price_trigger_by_order_id() {
        let mut deps = mock_dependencies();

        let order_idx = Uint128::new(17);

        let trigger = Trigger {
            bounty_id: Uint128::from(1u128),
            configuration: TriggerConfiguration::Price {
                target_price: Decimal::percent(120),
                order_idx,
            },
        };

        save_trigger(&mut deps.storage, trigger.clone()).unwrap();

        let fetched_trigger = get_trigger_by_order_idx(&deps.storage, order_idx).unwrap();

        assert_eq!(fetched_trigger, Some(trigger));
    }
}
