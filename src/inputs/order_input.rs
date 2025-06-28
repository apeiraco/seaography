use async_graphql::dynamic::{InputObject, InputValue, TypeRef, ValueAccessor};
use sea_orm::{EntityTrait, Iterable};

use crate::{BuilderContext, EntityObjectBuilder};

/// The configuration structure for OrderInputBuilder
pub struct OrderInputConfig {
    /// used to format OrderInput object name
    pub type_name: crate::SimpleNamingFn,
}

impl std::default::Default for OrderInputConfig {
    fn default() -> Self {
        OrderInputConfig {
            type_name: Box::new(|object_name: &str| -> String {
                format!("{object_name}OrderInput")
            }),
        }
    }
}

/// This builder produces the OrderInput object of a SeaORM entity
pub struct OrderInputBuilder {}

impl OrderInputBuilder {
    /// used to get type name
    pub fn type_name(context: &BuilderContext, object_name: &str) -> String {
        context.order_input.type_name.as_ref()(object_name)
    }

    /// used to get the OrderInput object of a SeaORM entity
    pub fn to_object<T>(context: &BuilderContext) -> InputObject
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let object_name = EntityObjectBuilder::type_name::<T>(context);
        let name = Self::type_name(context, &object_name);

        T::Column::iter().fold(InputObject::new(name), |object, column| {
            object.field(InputValue::new(
                EntityObjectBuilder::column_name::<T>(context, &column),
                TypeRef::named(&context.order_by_enum.type_name),
            ))
        })
    }

    pub fn parse_object<T>(
        context: &BuilderContext,
        value: Option<ValueAccessor<'_>>,
    ) -> Vec<(T::Column, sea_orm::sea_query::Order)>
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        match value {
            Some(value) => {
                let mut data = Vec::new();

                let order_by = value.object().unwrap();

                for col in T::Column::iter() {
                    let column_name = EntityObjectBuilder::column_name::<T>(context, &col);
                    let order = order_by.get(&column_name);

                    if let Some(order) = order {
                        let order = order.enum_name().unwrap();

                        let asc_variant = &context.order_by_enum.asc_variant;
                        let desc_variant = &context.order_by_enum.desc_variant;

                        if order.eq(asc_variant) {
                            data.push((col, sea_orm::Order::Asc));
                        } else if order.eq(desc_variant) {
                            data.push((col, sea_orm::Order::Desc));
                        } else {
                            panic!("Cannot map enumeration")
                        }
                    }
                }

                data
            }
            None => Vec::new(),
        }
    }
}
