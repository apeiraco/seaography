use std::{future::Future, pin::Pin, sync::Arc};

use async_graphql::dynamic::{
    Field, FieldFuture, FieldValue, InputValue, ObjectAccessor, ResolverContext, TypeRef,
};
use sea_orm::{
    ActiveModelTrait, DatabaseConnection, EntityTrait, IntoActiveModel, TransactionTrait,
};

use crate::{
    guard_error, prepare_active_model, BuilderContext, DatabaseContext, EntityInputBuilder,
    EntityObjectBuilder, EntityQueryFieldBuilder, GuardAction, OperationType, UserContext,
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
            self.context.entity_create_batch_mutation.mutation_suffix
        )
    }

    /// used to get the create mutation field for a SeaORM entity using the hooks system
    pub fn to_field<T, A>(&self) -> Field
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
        <T as EntityTrait>::Model: IntoActiveModel<A>,
        A: ActiveModelTrait<Entity = T> + sea_orm::ActiveModelBehavior + Send + 'static,
    {
        let entity_input_builder = EntityInputBuilder {
            context: self.context,
        };
        let entity_object_builder = EntityObjectBuilder {
            context: self.context,
        };

        let context = self.context;

        let object_name: String = entity_object_builder.type_name::<T>();
        let hooks = &self.context.hooks;

        Field::new(
            self.type_name::<T>(),
            TypeRef::named_nn_list_nn(entity_object_builder.basic_type_name::<T>()),
            move |ctx| {
                let object_name = object_name.clone();
                FieldFuture::new(async move {
                    if let GuardAction::Block(reason) =
                        hooks.entity_guard(&ctx, &object_name, OperationType::Create)
                    {
                        return Err(guard_error(reason, "Entity guard triggered."));
                    }

                    let db = &ctx
                        .data::<DatabaseConnection>()?
                        .restricted(ctx.data_opt::<UserContext>())?;

                    let transaction = db.begin().await?;

                    let entity_input_builder = EntityInputBuilder { context };
                    let entity_object_builder = EntityObjectBuilder { context };

                    let mut results: Vec<_> = Vec::new();
                    for input in ctx
                        .args
                        .try_get(&context.entity_create_batch_mutation.data_field)?
                        .list()?
                        .iter()
                    {
                        let input_object = &input.object()?;
                        for (column, _) in input_object.iter() {
                            if let GuardAction::Block(reason) =
                                hooks.field_guard(&ctx, &object_name, column, OperationType::Create)
                            {
                                return Err(guard_error(reason, "Field guard triggered."));
                            }
                        }

                        let mut active_model = prepare_active_model::<T, A>(
                            &entity_input_builder,
                            &entity_object_builder,
                            input_object,
                        )?;
                        if let GuardAction::Block(reason) = hooks.before_active_model_save(
                            &ctx,
                            &object_name,
                            OperationType::Create,
                            &mut active_model,
                        ) {
                            return Err(guard_error(
                                reason,
                                "Blocked by before_active_model_save.",
                            ));
                        }

                        let result = active_model.insert(&transaction).await?;
                        results.push(result);
                    }

                    transaction.commit().await?;

                    hooks
                        .entity_watch(&ctx, &object_name, OperationType::Create)
                        .await;

                    Ok(Some(FieldValue::list(
                        results.into_iter().map(FieldValue::owned_any),
                    )))
                })
            },
        )
        .argument(InputValue::new(
            &context.entity_create_batch_mutation.data_field,
            TypeRef::named_nn_list_nn(entity_input_builder.insert_type_name::<T>()),
        ))
    }

    /// Fork-compatible: create batch mutation field with custom mutation function
    pub fn to_field_with_mutation_fn<T>(
        &self,
        mutation_fn: CreateBatchMutationFn<T::Model>,
    ) -> Field
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let context = self.context;
        let entity_object_builder = EntityObjectBuilder { context };
        let entity_input_builder = EntityInputBuilder { context };
        let object_name: String = entity_object_builder.type_name::<T>();

        let guard = context.guards.entity_guards.get(&object_name);
        let field_guards = &context.guards.field_guards;

        Field::new(
            self.type_name::<T>(),
            TypeRef::named_nn_list_nn(entity_object_builder.basic_type_name::<T>()),
            move |resolve_ctx| {
                let mutation_fn = mutation_fn.clone();

                FieldFuture::new(async move {
                    let guard_flag = if let Some(guard) = guard {
                        (*guard)(&resolve_ctx)
                    } else {
                        GuardAction::Allow
                    };

                    if let GuardAction::Block(reason) = guard_flag {
                        return Err::<Option<_>, async_graphql::Error>(
                            guard_error(reason, "Entity guard triggered."),
                        );
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
                                EntityObjectBuilder { context }.type_name::<T>(),
                                column
                            ));
                            let field_guard_flag = if let Some(field_guard) = field_guard {
                                (*field_guard)(&resolve_ctx)
                            } else {
                                GuardAction::Allow
                            };
                            if let GuardAction::Block(reason) = field_guard_flag {
                                return Err::<Option<_>, async_graphql::Error>(
                                    guard_error(reason, "Field guard triggered."),
                                );
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
            TypeRef::named_nn_list_nn(entity_input_builder.insert_type_name::<T>()),
        ))
    }
}

