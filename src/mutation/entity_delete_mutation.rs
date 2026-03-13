use std::{future::Future, pin::Pin, sync::Arc};

use async_graphql::dynamic::{
    Field, FieldFuture, InputValue, ResolverContext, TypeRef, ValueAccessor,
};
use sea_orm::{
    ActiveModelTrait, DatabaseConnection, DeleteResult, EntityTrait, IntoActiveModel, QueryFilter,
};

use crate::{
    get_filter_conditions, guard_error, BuilderContext, DatabaseContext, EntityObjectBuilder,
    EntityQueryFieldBuilder, FilterInputBuilder, GuardAction, OperationType, UserContext,
};

pub type DeleteMutationFn = Arc<
    dyn for<'a> Fn(
            &'a ResolverContext<'a>,
            Option<ValueAccessor<'a>>,
        )
            -> Pin<Box<dyn Future<Output = Result<u64, async_graphql::Error>> + Send + 'a>>
        + Send
        + Sync,
>;

/// The configuration structure of EntityDeleteMutationBuilder
pub struct EntityDeleteMutationConfig {
    /// suffix that is appended on delete mutations
    pub mutation_suffix: String,

    /// name for `filter` field
    pub filter_field: String,
}

impl std::default::Default for EntityDeleteMutationConfig {
    fn default() -> Self {
        Self {
            mutation_suffix: {
                if cfg!(feature = "field-snake-case") {
                    "_delete"
                } else {
                    "Delete"
                }
                .into()
            },
            filter_field: "filter".into(),
        }
    }
}

/// This builder produces the delete mutation for an entity
pub struct EntityDeleteMutationBuilder {
    pub context: &'static BuilderContext,
}

impl EntityDeleteMutationBuilder {
    /// used to get mutation name for a SeaORM entity
    pub fn type_name<T>(&self) -> String
    where
        T: EntityTrait,
    {
        let entity_query_field_builder = EntityQueryFieldBuilder {
            context: self.context,
        };
        format!(
            "{}{}",
            entity_query_field_builder.type_name::<T>(),
            self.context.entity_delete_mutation.mutation_suffix
        )
    }

    /// used to get the delete mutation field for a SeaORM entity using the hooks system
    pub fn to_field<T, A>(&self) -> Field
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: IntoActiveModel<A>,
        A: ActiveModelTrait<Entity = T> + sea_orm::ActiveModelBehavior + Send,
    {
        let entity_filter_input_builder = FilterInputBuilder {
            context: self.context,
        };
        let entity_object_builder = EntityObjectBuilder {
            context: self.context,
        };
        let object_name: String = entity_object_builder.type_name::<T>();
        let object_name_ = object_name.clone();

        let context = self.context;
        let hooks = &self.context.hooks;

        Field::new(
            self.type_name::<T>(),
            TypeRef::named_nn(TypeRef::INT),
            move |ctx| {
                let object_name = object_name.clone();
                FieldFuture::new(async move {
                    if let GuardAction::Block(reason) =
                        hooks.entity_guard(&ctx, &object_name, OperationType::Delete)
                    {
                        return Err(guard_error(reason, "Entity guard triggered."));
                    }

                    let db = &ctx
                        .data::<DatabaseConnection>()?
                        .restricted(ctx.data_opt::<UserContext>())?;

                    let filters = ctx.args.get(&context.entity_delete_mutation.filter_field);
                    let filter_condition = get_filter_conditions::<T>(context, Some(&ctx), filters)?;

                    let mut stmt = T::delete_many();
                    if let Some(filter) =
                        hooks.entity_filter(&ctx, &object_name, OperationType::Delete)
                    {
                        stmt = stmt.filter(filter);
                    }
                    let res: DeleteResult = stmt.filter(filter_condition).exec(db).await?;

                    hooks
                        .entity_watch(&ctx, &object_name, OperationType::Delete)
                        .await;

                    Ok(Some(async_graphql::Value::from(res.rows_affected)))
                })
            },
        )
        .argument(InputValue::new(
            &context.entity_delete_mutation.filter_field,
            TypeRef::named(entity_filter_input_builder.type_name(&object_name_)),
        ))
    }

    /// Fork-compatible: delete mutation field with custom mutation function
    pub fn to_field_with_mutation_fn<T>(&self, mutation_fn: DeleteMutationFn) -> Field
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let context = self.context;
        let entity_object_builder = EntityObjectBuilder { context };
        let entity_filter_input_builder = FilterInputBuilder { context };
        let object_name: String = entity_object_builder.type_name::<T>();

        let guard = context.guards.entity_guards.get(&object_name);

        Field::new(
            self.type_name::<T>(),
            TypeRef::named_nn(TypeRef::INT),
            move |resolve_context| {
                let mutation_fn = mutation_fn.clone();

                FieldFuture::new(async move {
                    let guard_flag = if let Some(guard) = guard {
                        (*guard)(&resolve_context)
                    } else {
                        GuardAction::Allow
                    };

                    if let GuardAction::Block(reason) = guard_flag {
                        return Err::<Option<_>, async_graphql::Error>(
                            guard_error(reason, "Entity guard triggered."),
                        );
                    }

                    let filters = resolve_context
                        .args
                        .get(&context.entity_delete_mutation.filter_field);

                    let rows_affected: u64 = mutation_fn(&resolve_context, filters).await?;

                    Ok(Some(async_graphql::Value::from(rows_affected)))
                })
            },
        )
        .argument(InputValue::new(
            &context.entity_delete_mutation.filter_field,
            TypeRef::named(entity_filter_input_builder.type_name(&object_name)),
        ))
    }
}

