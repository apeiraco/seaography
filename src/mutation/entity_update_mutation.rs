use std::{future::Future, pin::Pin, sync::Arc};

use async_graphql::dynamic::{
    Field, FieldFuture, FieldValue, InputValue, ObjectAccessor, ResolverContext, TypeRef,
    ValueAccessor,
};
use sea_orm::{
    ActiveModelTrait, DatabaseConnection, EntityTrait, IntoActiveModel, QueryFilter,
    TransactionTrait,
};

use crate::{
    get_filter_conditions, prepare_active_model, BuilderContext, EntityInputBuilder,
    EntityObjectBuilder, EntityQueryFieldBuilder, FilterInputBuilder, GuardAction,
};

pub type UpdateMutationFn<M> = Arc<
    dyn for<'a> Fn(
            &ResolverContext<'a>,
            Option<ValueAccessor<'_>>,
            ObjectAccessor<'_>,
        ) -> Pin<
            Box<dyn Future<Output = Result<Vec<M>, async_graphql::Error>> + Send + 'a>,
        > + Send
        + Sync,
>;
/// The configuration structure of EntityUpdateMutationBuilder
pub struct EntityUpdateMutationConfig {
    /// suffix that is appended on update mutations
    pub mutation_suffix: String,

    /// name for `data` field
    pub data_field: String,

    /// name for `filter` field
    pub filter_field: String,
}

impl std::default::Default for EntityUpdateMutationConfig {
    fn default() -> Self {
        Self {
            mutation_suffix: {
                if cfg!(feature = "field-snake-case") {
                    "_update"
                } else {
                    "Update"
                }
                .into()
            },
            data_field: "data".into(),
            filter_field: "filter".into(),
        }
    }
}

/// This builder produces the update mutation for an entity
pub struct EntityUpdateMutationBuilder {
    pub context: &'static BuilderContext,
}

impl EntityUpdateMutationBuilder {
    /// used to get mutation name for a SeaORM entity
    pub fn type_name<T>(context: &BuilderContext) -> String
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        format!(
            "{}{}",
            EntityQueryFieldBuilder::type_name::<T>(context),
            context.entity_update_mutation.mutation_suffix
        )
    }

    pub fn to_field_with_mutation_fn<T>(&self, mutation_fn: UpdateMutationFn<T::Model>) -> Field
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let context = self.context;
        let object_name: String = EntityObjectBuilder::type_name::<T>(context);

        let guard = context.guards.entity_guards.get(&object_name);

        let data_input_value = InputValue::new(
            &context.entity_update_mutation.data_field,
            TypeRef::named_nn(EntityInputBuilder::update_type_name::<T>(context)),
        );

        let filter_input_value = InputValue::new(
            &context.entity_update_mutation.filter_field,
            TypeRef::named(FilterInputBuilder::type_name(context, &object_name)),
        );

        Field::new(
            Self::type_name::<T>(context),
            TypeRef::named_nn_list_nn(EntityObjectBuilder::basic_type_name::<T>(context)),
            move |ctx| {
                let mutation_fn = mutation_fn.clone();
                let guard_flag = if let Some(guard) = guard {
                    (*guard)(&ctx)
                } else {
                    GuardAction::Allow
                };

                FieldFuture::new(async move {
                    if let GuardAction::Block(reason) = guard_flag {
                        return match reason {
                            Some(reason) => Err::<Option<_>, async_graphql::Error>(
                                async_graphql::Error::new(reason),
                            ),
                            None => Err::<Option<_>, async_graphql::Error>(
                                async_graphql::Error::new("Entity guard triggered."),
                            ),
                        };
                    }

                    let filters = ctx.args.get(&context.entity_update_mutation.filter_field);

                    let value_accessor = ctx
                        .args
                        .get(&context.entity_update_mutation.data_field)
                        .unwrap();
                    let input_object = value_accessor.object()?;

                    let field_guards = &context.guards.field_guards;

                    for (column, _) in input_object.iter() {
                        let field_guard = field_guards.get(&format!(
                            "{}.{}",
                            EntityObjectBuilder::type_name::<T>(context),
                            column
                        ));
                        let field_guard_flag = if let Some(field_guard) = field_guard {
                            (*field_guard)(&ctx)
                        } else {
                            GuardAction::Allow
                        };
                        if let GuardAction::Block(reason) = field_guard_flag {
                            return match reason {
                                Some(reason) => Err::<Option<_>, async_graphql::Error>(
                                    async_graphql::Error::new(reason),
                                ),
                                None => Err::<Option<_>, async_graphql::Error>(
                                    async_graphql::Error::new("Field guard triggered."),
                                ),
                            };
                        }
                    }

                    let result = mutation_fn(&ctx, filters, input_object).await?;

                    Ok(Some(FieldValue::list(
                        result.into_iter().map(FieldValue::owned_any),
                    )))
                })
            },
        )
        .argument(data_input_value)
        .argument(filter_input_value)
    }

    pub fn default_mutation_fn<T, A>(&self, active_model_hooks: bool) -> UpdateMutationFn<T::Model>
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
        Arc::new(move |resolve_context, filters, input_object| {
            let active_model =
                prepare_active_model::<T, A>(context, &input_object, resolve_context);
            let db = resolve_context.data::<DatabaseConnection>().cloned();
            let filter_condition = get_filter_conditions::<T>(resolve_context, context, filters);

            Box::pin(async move {
                let db = db?;
                let active_model = active_model?;
                if active_model_hooks {
                    let transaction = db.begin().await?;

                    let active_model = active_model.before_save(&transaction, false).await?;

                    let models = T::update_many()
                        .set(active_model)
                        .filter(filter_condition.clone())
                        .exec_with_returning(&transaction)
                        .await?;
                    let mut result = vec![];

                    for model in models {
                        result.push(A::after_save(model, &transaction, false).await?);
                    }

                    transaction.commit().await?;

                    Ok(result)
                } else {
                    let result = T::update_many()
                        .set(active_model)
                        .filter(filter_condition)
                        .exec_with_returning(&db)
                        .await?;

                    Ok(result)
                }
            })
        })
    }

    /// used to get the update mutation field for a SeaORM entity
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
