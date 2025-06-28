use std::collections::BTreeMap;

use async_graphql::dynamic::{InputObject, InputValue, ObjectAccessor, ResolverContext};
use sea_orm::{ColumnTrait, EntityTrait, Iterable, PrimaryKeyToColumn, PrimaryKeyTrait};

use crate::{BuilderContext, EntityObjectBuilder, SeaResult, TypesMapHelper};

/// The configuration structure of EntityInputBuilder
pub struct EntityInputConfig {
    /// suffix that is appended on insert input objects
    pub insert_suffix: String,
    /// names of "{entity}.{column}" you want to skip the insert input to be generated
    pub insert_skips: Vec<String>,
    /// suffix that is appended on update input objects
    pub update_suffix: String,
    /// names of "{entity}.{column}" you want to skip the update input to be generated
    pub update_skips: Vec<String>,
}

impl std::default::Default for EntityInputConfig {
    fn default() -> Self {
        EntityInputConfig {
            insert_suffix: "InsertInput".into(),
            insert_skips: Vec::new(),
            update_suffix: "UpdateInput".into(),
            update_skips: Vec::new(),
        }
    }
}

/// Used to create the entity create/update input object
pub struct EntityInputBuilder {}

impl EntityInputBuilder {
    /// used to get SeaORM entity insert input object name
    pub fn insert_type_name<T>(context: &BuilderContext) -> String
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let object_name = EntityObjectBuilder::type_name::<T>(context);
        format!("{}{}", object_name, context.entity_input.insert_suffix)
    }

    /// used to get SeaORM entity update input object name
    pub fn update_type_name<T>(context: &BuilderContext) -> String
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let object_name = EntityObjectBuilder::type_name::<T>(context);
        format!("{}{}", object_name, context.entity_input.update_suffix)
    }

    /// used to produce the SeaORM entity input object
    fn input_object<T>(context: &BuilderContext, is_insert: bool) -> InputObject
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let name = if is_insert {
            Self::insert_type_name::<T>(context)
        } else {
            Self::update_type_name::<T>(context)
        };

        T::Column::iter().fold(InputObject::new(name), |object, column| {
            let column_name = EntityObjectBuilder::column_name::<T>(context, &column);

            let full_name = format!(
                "{}.{}",
                EntityObjectBuilder::type_name::<T>(context),
                column_name
            );

            let skip = if is_insert {
                context.entity_input.insert_skips.contains(&full_name)
            } else {
                context.entity_input.update_skips.contains(&full_name)
            };

            if skip {
                return object;
            }

            let column_def = column.def();
            let enum_type_name = column.enum_type_name();

            let auto_increment = match <T::PrimaryKey as PrimaryKeyToColumn>::from_column(column) {
                Some(_) => T::PrimaryKey::auto_increment(),
                None => false,
            };
            let has_default_expr = column_def.get_column_default().is_some();
            let has_none_conversion = context
                .types
                .input_none_conversions
                .contains_key(&full_name);

            let is_insert_not_nullable = is_insert
                && !(column_def.is_null()
                    || auto_increment
                    || has_default_expr
                    || has_none_conversion);

            let graphql_type = match TypesMapHelper::sea_orm_column_type_to_graphql_type(
                context,
                column_def.get_column_type(),
                is_insert_not_nullable,
                enum_type_name,
            ) {
                Some(type_name) => type_name,
                None => return object,
            };

            object.field(InputValue::new(column_name, graphql_type))
        })
    }

    /// used to produce the SeaORM entity insert input object
    pub fn insert_input_object<T>(context: &BuilderContext) -> InputObject
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        Self::input_object::<T>(context, true)
    }

    /// used to produce the SeaORM entity update input object
    pub fn update_input_object<T>(context: &BuilderContext) -> InputObject
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        Self::input_object::<T>(context, false)
    }

    pub fn parse_object<T>(
        context: &BuilderContext,
        resolver_context: &ResolverContext<'_>,
        object: &ObjectAccessor,
    ) -> SeaResult<BTreeMap<String, sea_orm::Value>>
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let mut map = BTreeMap::<String, sea_orm::Value>::new();

        let entity_name = EntityObjectBuilder::type_name::<T>(context);
        for column in T::Column::iter() {
            let column_name = EntityObjectBuilder::column_name::<T>(context, &column);

            let value = match object.get(&column_name) {
                Some(value) => value,
                None => {
                    if let Some(parser) = context
                        .types
                        .input_none_conversions
                        .get(&format!("{entity_name}.{column_name}"))
                    {
                        let result = parser.as_ref()(resolver_context)?;
                        if let Some(result) = result {
                            map.insert(column_name, result);
                        }
                        continue;
                    }
                    continue;
                }
            };

            let result = TypesMapHelper::async_graphql_value_to_sea_orm_value::<T>(
                context,
                resolver_context,
                &column,
                &value,
            )?;

            map.insert(column_name, result);
        }

        Ok(map)
    }
}
