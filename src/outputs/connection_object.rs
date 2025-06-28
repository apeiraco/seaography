use async_graphql::dynamic::{Field, FieldFuture, FieldValue, Object, TypeRef};
use sea_orm::EntityTrait;

use crate::{
    BuilderContext, Edge, EdgeObjectBuilder, EntityObjectBuilder, PageInfo, PaginationInfo,
};

/// used to represent a GraphQL Connection node for any Type
#[derive(Clone, Debug)]
pub struct Connection<T>
where
    T: EntityTrait,
    <T as EntityTrait>::Model: Sync,
{
    /// cursor pagination info
    pub page_info: PageInfo,

    /// pagination info
    pub pagination_info: Option<PaginationInfo>,

    /// vector of data vector
    pub edges: Vec<Edge<T>>,
}

/// The configuration structure for ConnectionObjectBuilder
pub struct ConnectionObjectConfig {
    /// used to format the type name of the object
    pub type_name: crate::SimpleNamingFn,
    /// name for 'pageInfo' field
    pub page_info: String,
    /// name for 'paginationInfo' field
    pub pagination_info: String,
    /// name for 'edges' field
    pub edges: String,
    /// name for 'nodes' field
    pub nodes: String,
}

impl std::default::Default for ConnectionObjectConfig {
    fn default() -> Self {
        ConnectionObjectConfig {
            type_name: Box::new(|object_name: &str| -> String {
                format!("{object_name}Connection")
            }),
            page_info: {
                if cfg!(feature = "field-snake-case") {
                    "page_info"
                } else {
                    "pageInfo"
                }
                .into()
            },
            pagination_info: {
                if cfg!(feature = "field-snake-case") {
                    "pagination_info"
                } else {
                    "paginationInfo"
                }
                .into()
            },
            edges: "edges".into(),
            nodes: "nodes".into(),
        }
    }
}

/// This builder produces the Connection object for a SeaORM entity
pub struct ConnectionObjectBuilder {}

impl ConnectionObjectBuilder {
    /// used to get type name
    pub fn type_name(context: &BuilderContext, object_name: &str) -> String {
        context.connection_object.type_name.as_ref()(object_name)
    }

    /// used to get the Connection object for a SeaORM entity
    pub fn to_object<T>(context: &BuilderContext) -> Object
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let object_name = EntityObjectBuilder::type_name::<T>(context);
        let name = Self::type_name(context, &object_name);

        Object::new(name)
            .field(Field::new(
                &context.connection_object.page_info,
                TypeRef::named_nn(&context.page_info_object.type_name),
                |ctx| {
                    FieldFuture::new(async move {
                        let connection = ctx.parent_value.try_downcast_ref::<Connection<T>>()?;
                        Ok(Some(FieldValue::borrowed_any(&connection.page_info)))
                    })
                },
            ))
            .field(Field::new(
                &context.connection_object.pagination_info,
                TypeRef::named(&context.pagination_info_object.type_name),
                |ctx| {
                    FieldFuture::new(async move {
                        let connection = ctx.parent_value.try_downcast_ref::<Connection<T>>()?;
                        if let Some(value) = connection
                            .pagination_info
                            .as_ref()
                            .map(|pagination_info| FieldValue::borrowed_any(pagination_info))
                        {
                            Ok(Some(value))
                        } else {
                            Ok(FieldValue::NONE)
                        }
                    })
                },
            ))
            .field(Field::new(
                &context.connection_object.nodes,
                TypeRef::named_nn_list_nn(&object_name),
                |ctx| {
                    FieldFuture::new(async move {
                        let connection = ctx.parent_value.try_downcast_ref::<Connection<T>>()?;
                        Ok(Some(FieldValue::list(connection.edges.iter().map(
                            |edge: &Edge<T>| FieldValue::borrowed_any(&edge.node),
                        ))))
                    })
                },
            ))
            .field(Field::new(
                &context.connection_object.edges,
                TypeRef::named_nn_list_nn(EdgeObjectBuilder::type_name(context, &object_name)),
                |ctx| {
                    FieldFuture::new(async move {
                        let connection = ctx.parent_value.try_downcast_ref::<Connection<T>>()?;
                        Ok(Some(FieldValue::list(
                            connection
                                .edges
                                .iter()
                                .map(|edge: &Edge<T>| FieldValue::borrowed_any(edge)),
                        )))
                    })
                },
            ))
    }
}
