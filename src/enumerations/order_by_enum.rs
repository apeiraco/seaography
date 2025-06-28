use async_graphql::dynamic::{Enum, EnumItem};

use crate::BuilderContext;

/// The configuration structure for OrderByEnumBuilder
pub struct OrderByEnumConfig {
    /// the enumeration name
    pub type_name: String,
    /// the ASC variant name
    pub asc_variant: String,
    /// the DESC variant name
    pub desc_variant: String,
}

impl std::default::Default for OrderByEnumConfig {
    fn default() -> Self {
        OrderByEnumConfig {
            type_name: "OrderByEnum".into(),
            asc_variant: "ASC".into(),
            desc_variant: "DESC".into(),
        }
    }
}

/// The OrderByEnumeration is used for Entities Fields sorting
pub struct OrderByEnumBuilder {}

impl OrderByEnumBuilder {
    pub fn type_name(context: &BuilderContext) -> String {
        context.order_by_enum.type_name.clone()
    }

    pub fn asc_variant(context: &BuilderContext) -> String {
        context.order_by_enum.asc_variant.clone()
    }

    pub fn desc_variant(context: &BuilderContext) -> String {
        context.order_by_enum.desc_variant.clone()
    }

    pub fn is_asc(context: &BuilderContext, value: &str) -> bool {
        context.order_by_enum.asc_variant.eq(value)
    }

    pub fn is_desc(context: &BuilderContext, value: &str) -> bool {
        context.order_by_enum.desc_variant.eq(value)
    }

    /// used to get the GraphQL enumeration config
    pub fn enumeration(context: &BuilderContext) -> Enum {
        Enum::new(&context.order_by_enum.type_name)
            .item(EnumItem::new(&context.order_by_enum.asc_variant))
            .item(EnumItem::new(&context.order_by_enum.desc_variant))
    }
}
