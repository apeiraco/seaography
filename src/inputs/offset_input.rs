use std::borrow::Cow;

use async_graphql::dynamic::{InputObject, InputValue, ObjectAccessor, TypeRef};

use crate::BuilderContext;

/// used to hold information about offset pagination
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OffsetInput {
    pub offset: u64,
    pub limit: u64,
}

/// The configuration structure for OffsetInputBuilder
pub struct OffsetInputConfig {
    /// name of the object
    pub type_name: String,
    /// name for 'offset' field
    pub offset: String,
    /// name for 'limit' field
    pub limit: String,
}

impl std::default::Default for OffsetInputConfig {
    fn default() -> Self {
        Self {
            type_name: "OffsetInput".into(),
            offset: "offset".into(),
            limit: "limit".into(),
        }
    }
}

/// This builder produces the offset pagination options input object
pub struct OffsetInputBuilder {}

impl OffsetInputBuilder {
    /// used to get type name
    pub fn type_name<'a>(context: &'a BuilderContext) -> Cow<'a, str> {
        Cow::Borrowed(&context.offset_input.type_name)
    }

    /// used to get offset pagination options object
    pub fn input_object(context: &BuilderContext) -> InputObject {
        InputObject::new(&context.offset_input.type_name)
            .field(InputValue::new(
                &context.offset_input.limit,
                TypeRef::named_nn(TypeRef::INT),
            ))
            .field(InputValue::new(
                &context.offset_input.offset,
                TypeRef::named_nn(TypeRef::INT),
            ))
    }

    /// used to parse query input to offset pagination options struct
    pub fn parse_object(context: &BuilderContext, object: &ObjectAccessor) -> OffsetInput {
        let offset = object
            .get(&context.offset_input.offset)
            .map_or_else(|| Ok(0), |v| v.u64())
            .unwrap();

        let limit = object
            .get(&context.offset_input.limit)
            .unwrap()
            .u64()
            .unwrap();

        OffsetInput { offset, limit }
    }
}
