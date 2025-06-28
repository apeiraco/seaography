use std::{future::Future, pin::Pin, sync::Arc};

use async_graphql::dynamic::{
    Field, FieldFuture, InputValue, ResolverContext, TypeRef, ValueAccessor,
};
use sea_orm::{
    ActiveModelTrait, DatabaseConnection, EntityTrait, IntoActiveModel, QueryFilter,
    TransactionTrait,
};

use crate::{
    get_filter_conditions, BuilderContext, EntityObjectBuilder, EntityQueryFieldBuilder,
    FilterInputBuilder, GuardAction,
};

pub type DeleteMutationFn = Arc<
    dyn for<'a> Fn(
            &ResolverContext<'a>,
            Option<ValueAccessor<'_>>,
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
    pub fn type_name<T>(context: &BuilderContext) -> String
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        format!(
            "{}{}",
            EntityQueryFieldBuilder::type_name::<T>(context),
            context.entity_delete_mutation.mutation_suffix
        )
    }

    pub fn to_field_with_mutation_fn<T>(&self, mutation_fn: DeleteMutationFn) -> Field
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let context = self.context;
        let object_name: String = EntityObjectBuilder::type_name::<T>(context);

        let guard = context.guards.entity_guards.get(&object_name);

        Field::new(
            Self::type_name::<T>(context),
            TypeRef::named_nn(TypeRef::INT),
            move |resolve_context| {
                let mutation_fn = mutation_fn.clone();
                let guard_flag = if let Some(guard) = guard {
                    (*guard)(&resolve_context)
                } else {
                    GuardAction::Allow
                };

                FieldFuture::new(async move {
                    if let GuardAction::Block(reason) = guard_flag {
                        return Err::<Option<_>, async_graphql::Error>(async_graphql::Error::new(
                            reason.unwrap_or("Entity guard triggered.".into()),
                        ));
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
            TypeRef::named(FilterInputBuilder::type_name(context, &object_name)),
        ))
    }

    pub fn default_mutation_fn<T, A>(&self, active_model_hooks: bool) -> DeleteMutationFn
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
        <T as EntityTrait>::Model: IntoActiveModel<A>,
        A: ActiveModelTrait<Entity = T>
            + sea_orm::ActiveModelBehavior
            + std::marker::Send
            + 'static,
    {
        let context = self.context;
        Arc::new(move |resolve_context, filters| {
            let db = resolve_context.data::<DatabaseConnection>().cloned();

            let filters_condition = get_filter_conditions::<T>(resolve_context, context, filters);

            Box::pin(async move {
                let db = db?;
                if active_model_hooks {
                    let transaction = db.begin().await?;

                    let models: Vec<T::Model> = T::find()
                        .filter(filters_condition.clone())
                        .all(&transaction)
                        .await?;

                    let mut active_models: Vec<A> = vec![];
                    for model in models {
                        let active_model = model.into_active_model();
                        active_models.push(active_model.before_delete(&transaction).await?);
                    }

                    let result = T::delete_many()
                        .filter(filters_condition)
                        .exec(&transaction)
                        .await?;

                    for active_model in active_models {
                        active_model.after_delete(&transaction).await?;
                    }

                    transaction.commit().await?;

                    Ok(result.rows_affected)
                } else {
                    let result = T::delete_many().filter(filters_condition).exec(&db).await?;

                    Ok(result.rows_affected)
                }
            })
        })
    }

    /// used to get the delete mutation field for a SeaORM entity
    pub fn to_field<T, A>(&self, active_model_hooks: bool) -> Field
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
        <T as EntityTrait>::Model: IntoActiveModel<A>,
        A: ActiveModelTrait<Entity = T>
            + sea_orm::ActiveModelBehavior
            + std::marker::Send
            + 'static,
    {
        self.to_field_with_mutation_fn::<T>(self.default_mutation_fn::<T, A>(active_model_hooks))
    }
}
