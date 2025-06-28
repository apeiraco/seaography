use async_graphql::dynamic::{ObjectAccessor, ResolverContext, ValueAccessor};
use sea_orm::{Condition, EntityTrait, Iterable};

use crate::{BuilderContext, EntityObjectBuilder, FilterTypesMapHelper};

/// utility function used to create the query filter condition
/// for a SeaORM entity using query filter inputs
pub fn get_filter_conditions<T>(
    resolver_context: &ResolverContext<'_>,
    context: &BuilderContext,
    filters: Option<ValueAccessor>,
) -> Condition
where
    T: EntityTrait,
    <T as EntityTrait>::Model: Sync,
{
    let filters = filters.map(|f| f.object().unwrap());
    recursive_prepare_condition::<T>(resolver_context, context, filters)
}

/// used to prepare recursively the query filtering condition
pub fn recursive_prepare_condition<T>(
    resolver_context: &ResolverContext<'_>,
    context: &BuilderContext,
    filters: Option<ObjectAccessor>,
) -> Condition
where
    T: EntityTrait,
    <T as EntityTrait>::Model: Sync,
{
    let condition = T::Column::iter().fold(Condition::all(), |condition, column: T::Column| {
        let column_name = EntityObjectBuilder::column_name::<T>(context, &column);

        let filter = filters
            .as_ref()
            .and_then(|f| f.get(&column_name))
            .map(|m| m.object().unwrap());

        FilterTypesMapHelper::prepare_column_condition::<T>(
            context,
            resolver_context,
            condition,
            filter,
            &column,
        )
        .unwrap()
    });

    let condition = if let Some(filters) = filters {
        let condition = if let Some(and) = filters.get("and") {
            let filters = and.list().unwrap();

            condition.add(filters.iter().fold(
                Condition::all(),
                |condition, filters: ValueAccessor| {
                    let filters = filters.object().unwrap();
                    condition.add(recursive_prepare_condition::<T>(
                        resolver_context,
                        context,
                        Some(filters),
                    ))
                },
            ))
        } else {
            condition
        };

        let condition = if let Some(or) = filters.get("or") {
            let filters = or.list().unwrap();

            condition.add(filters.iter().fold(
                Condition::any(),
                |condition, filters: ValueAccessor| {
                    let filters = filters.object().unwrap();
                    condition.add(recursive_prepare_condition::<T>(
                        resolver_context,
                        context,
                        Some(filters),
                    ))
                },
            ))
        } else {
            condition
        };

        condition
    } else {
        condition
    };

    condition
}
