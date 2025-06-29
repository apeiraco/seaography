use std::{future::Future, pin::Pin, sync::Arc};

use async_graphql::dynamic::{
    Field, FieldFuture, FieldValue, InputValue, ObjectAccessor, ResolverContext, TypeRef,
};
use sea_orm::{
    ActiveModelTrait, DatabaseConnection, EntityTrait, IntoActiveModel, TransactionTrait,
};

use crate::{
    prepare_active_model, BuilderContext, EntityInputBuilder, EntityObjectBuilder,
    EntityQueryFieldBuilder, GuardAction,
};

pub type CreateBatchMutationFn<M> = Arc<
    dyn for<'a> Fn(
            &'a ResolverContext<'a>,
            Vec<ObjectAccessor<'a>>,
        ) -> Pin<
            Box<dyn Future<Output = Result<Vec<M>, async_graphql::Error>> + Send + 'a>,
        > + Send
        + Sync,
>;

/// The configuration structure of EntityCreateBatchMutationBuilder
pub struct EntityCreateBatchMutationConfig {
    /// suffix that is appended on create mutations
    pub mutation_suffix: String,
    /// name for `data` field
    pub data_field: String,
}

impl std::default::Default for EntityCreateBatchMutationConfig {
    fn default() -> Self {
        EntityCreateBatchMutationConfig {
            mutation_suffix: {
                if cfg!(feature = "field-snake-case") {
                    "_create_batch"
                } else {
                    "CreateBatch"
                }
                .into()
            },
            data_field: "data".into(),
        }
    }
}

/// This builder produces the create batch mutation for an entity
pub struct EntityCreateBatchMutationBuilder {
    pub context: &'static BuilderContext,
}

impl EntityCreateBatchMutationBuilder {
    /// used to get mutation name for a SeaORM entity
    pub fn type_name<T>(context: &BuilderContext) -> String
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        format!(
            "{}{}",
            EntityQueryFieldBuilder::type_name::<T>(context),
            context.entity_create_batch_mutation.mutation_suffix
        )
    }

    pub fn default_mutation_fn<T, A>(
        &self,
        active_model_hooks: bool,
    ) -> CreateBatchMutationFn<T::Model>
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
        <T as EntityTrait>::Model: IntoActiveModel<A>,
        A: ActiveModelTrait<Entity = T> + sea_orm::ActiveModelBehavior + std::marker::Send,
    {
        let builder_context = self.context;
        Arc::new(move |resolve_context, input_objects| {
            Box::pin(async move {
                let active_models = input_objects
                    .into_iter()
                    .map(|input_object| {
                        prepare_active_model::<T, A>(
                            builder_context,
                            &input_object,
                            resolve_context,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;

                let db = resolve_context.data::<DatabaseConnection>()?;

                if active_model_hooks {
                    let transaction = db.begin().await?;

                    let mut before_save_models = vec![];

                    for active_model in active_models {
                        let before_save_model =
                            active_model.before_save(&transaction, false).await?;
                        before_save_models.push(before_save_model);
                    }

                    let models: Vec<T::Model> = T::insert_many(before_save_models)
                        .exec_with_returning_many(&transaction)
                        .await?;

                    let mut result = vec![];
                    for model in models {
                        let after_save_model = A::after_save(model, &transaction, false).await?;
                        result.push(after_save_model);
                    }

                    transaction.commit().await?;

                    Ok(result)
                } else {
                    let results: Vec<T::Model> = T::insert_many(active_models)
                        .exec_with_returning_many(db)
                        .await?;

                    Ok(results)
                }
            })
        })
    }

    pub fn to_field_with_mutation_fn<T>(
        &self,
        mutation_fn: CreateBatchMutationFn<T::Model>,
    ) -> Field
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let context = self.context;
        let object_name: String = EntityObjectBuilder::type_name::<T>(context);

        let guard = context.guards.entity_guards.get(&object_name);

        let field_guards = &context.guards.field_guards;

        Field::new(
            Self::type_name::<T>(context),
            TypeRef::named_nn_list_nn(EntityObjectBuilder::basic_type_name::<T>(context)),
            move |resolve_ctx| {
                let mutation_fn = mutation_fn.clone();

                FieldFuture::new(async move {
                    let guard_flag = if let Some(guard) = guard {
                        (*guard)(&resolve_ctx)
                    } else {
                        GuardAction::Allow
                    };

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

                    let mut input_objects = vec![];
                    let list = resolve_ctx
                        .args
                        .get(&context.entity_create_batch_mutation.data_field)
                        .unwrap()
                        .list()?;
                    for input in list.iter() {
                        let input_object = input.object()?;
                        for (column, _) in input_object.iter() {
                            let field_guard = field_guards.get(&format!(
                                "{}.{}",
                                EntityObjectBuilder::type_name::<T>(context),
                                column
                            ));
                            let field_guard_flag = if let Some(field_guard) = field_guard {
                                (*field_guard)(&resolve_ctx)
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

                        input_objects.push(input_object);
                    }

                    let results = mutation_fn(&resolve_ctx, input_objects).await?;

                    Ok(Some(FieldValue::list(
                        results.into_iter().map(FieldValue::owned_any),
                    )))
                })
            },
        )
        .argument(InputValue::new(
            &context.entity_create_batch_mutation.data_field,
            TypeRef::named_nn_list_nn(EntityInputBuilder::insert_type_name::<T>(context)),
        ))
    }

    /// used to get the create mutation field for a SeaORM entity
    pub fn to_field<T, A>(&self, active_model_hooks: bool) -> Field
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
        <T as EntityTrait>::Model: IntoActiveModel<A>,
        A: ActiveModelTrait<Entity = T> + sea_orm::ActiveModelBehavior + std::marker::Send,
    {
        self.to_field_with_mutation_fn::<T>(self.default_mutation_fn::<T, A>(active_model_hooks))
    }
}
