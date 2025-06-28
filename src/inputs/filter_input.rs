use async_graphql::dynamic::{InputObject, InputValue, TypeRef};
use sea_orm::{EntityTrait, Iterable};

use crate::{BuilderContext, EntityObjectBuilder, FilterTypesMapHelper};

/// The configuration structure for FilterInputBuilder
pub struct FilterInputConfig {
    /// the filter input type name formatter function
    pub type_name: crate::SimpleNamingFn,
}

impl std::default::Default for FilterInputConfig {
    fn default() -> Self {
        FilterInputConfig {
            type_name: Box::new(|object_name: &str| -> String {
                format!("{object_name}FilterInput")
            }),
        }
    }
}

/// This builder is used to produce the filter input object of a SeaORM entity
pub struct FilterInputBuilder {}

impl FilterInputBuilder {
    /// used to get the filter input object name
    /// object_name is the name of the SeaORM Entity GraphQL object
    pub fn type_name(context: &BuilderContext, object_name: &str) -> String {
        context.filter_input.type_name.as_ref()(object_name)
    }

    /// used to produce the filter input object of a SeaORM entity
    pub fn to_object<T>(context: &BuilderContext) -> InputObject
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let entity_name = EntityObjectBuilder::type_name::<T>(context);
        let filter_name = Self::type_name(context, &entity_name);

        let object = T::Column::iter().fold(InputObject::new(&filter_name), |object, column| {
            match FilterTypesMapHelper::get_column_filter_input_value::<T>(context, &column) {
                Some(field) => object.field(field),
                None => object,
            }
        });

        object
            .field(InputValue::new("and", TypeRef::named_nn_list(&filter_name)))
            .field(InputValue::new("or", TypeRef::named_nn_list(&filter_name)))
    }
}
