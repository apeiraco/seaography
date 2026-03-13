use std::{future::Future, pin::Pin, sync::Arc};

use async_graphql::dynamic::{
    Field, FieldFuture, FieldValue, InputValue, ObjectAccessor, ResolverContext, TypeRef,
};
use sea_orm::{
    ActiveModelTrait, DatabaseConnection, EntityTrait, IntoActiveModel, Iterable,
    PrimaryKeyToColumn, PrimaryKeyTrait,
};

use crate::{
    guard_error, BuilderContext, DatabaseContext, EntityInputBuilder, EntityObjectBuilder,
    EntityQueryFieldBuilder, GuardAction, OperationType, UserContext,
};

pub type CreateOneMutationFn<M> = Arc<
    dyn for<'a> Fn(
            &'a ResolverContext<'a>,
            ObjectAccessor<'a>,
        )
            -> Pin<Box<dyn Future<Output = Result<M, async_graphql::Error>> + Send + 'a>>
        + Send
        + Sync,
>;

/// The configuration structure of EntityCreateOneMutationBuilder
pub struct EntityCreateOneMutationConfig {
    /// suffix that is appended on create mutations
    pub mutation_suffix: String,
    /// name for `data` field
    pub data_field: String,
}

impl std::default::Default for EntityCreateOneMutationConfig {
    fn default() -> Self {
        EntityCreateOneMutationConfig {
            mutation_suffix: {
                if cfg!(feature = "field-snake-case") {
                    "_create_one"
                } else {
                    "CreateOne"
                }
                .into()
            },
            data_field: "data".into(),
        }
    }
}

/// This builder produces the create one mutation for an entity
pub struct EntityCreateOneMutationBuilder {
    pub context: &'static BuilderContext,
}

impl EntityCreateOneMutationBuilder {
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
            self.context.entity_create_one_mutation.mutation_suffix
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
            TypeRef::named_nn(entity_object_builder.basic_type_name::<T>()),
            move |ctx| {
                let object_name = object_name.clone();
                FieldFuture::new(async move {
                    if let GuardAction::Block(reason) =
                        hooks.entity_guard(&ctx, &object_name, OperationType::Create)
                    {
                        return Err(guard_error(reason, "Entity guard triggered."));
                    }

                    let entity_input_builder = EntityInputBuilder { context };
                    let entity_object_builder = EntityObjectBuilder { context };
                    let value_accessor = ctx
                        .args
                        .try_get(&context.entity_create_one_mutation.data_field)?;
                    let input_object = &value_accessor.object()?;

                    for (column, _) in input_object.iter() {
                        if let GuardAction::Block(reason) =
                            hooks.field_guard(&ctx, &object_name, column, OperationType::Create)
                        {
                            return Err(guard_error(reason, "Field guard triggered."));
                        }
                    }

                    let db = &ctx
                        .data::<DatabaseConnection>()?
                        .restricted(ctx.data_opt::<UserContext>())?;

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
                        return Err(guard_error(reason, "Blocked by before_active_model_save."));
                    }

                    let result = active_model.insert(db).await?;

                    hooks
                        .entity_watch(&ctx, &object_name, OperationType::Create)
                        .await;

                    Ok(Some(FieldValue::owned_any(result)))
                })
            },
        )
        .argument(InputValue::new(
            &context.entity_create_one_mutation.data_field,
            TypeRef::named_nn(entity_input_builder.insert_type_name::<T>()),
        ))
    }

    /// Fork-compatible: create mutation field with custom mutation function
    /// Uses BTreeMap-based guards from GuardsConfig
    pub fn to_field_with_mutation_fn<T>(&self, mutation_fn: CreateOneMutationFn<T::Model>) -> Field
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
            TypeRef::named_nn(entity_object_builder.basic_type_name::<T>()),
            move |resolver_context| {
                let mutation_fn = mutation_fn.clone();

                FieldFuture::new(async move {
                    let guard_flag = if let Some(guard) = guard {
                        (*guard)(&resolver_context)
                    } else {
                        GuardAction::Allow
                    };

                    if let GuardAction::Block(reason) = guard_flag {
                        return Err::<Option<_>, async_graphql::Error>(
                            guard_error(reason, "Entity guard triggered."),
                        );
                    }

                    let value_accessor = resolver_context
                        .args
                        .get(&context.entity_create_one_mutation.data_field)
                        .unwrap();
                    let input_object = value_accessor.object()?;

                    for (column, _) in input_object.iter() {
                        let field_guard = field_guards.get(&format!(
                            "{}.{}",
                            EntityObjectBuilder { context }.type_name::<T>(),
                            column
                        ));
                        let field_guard_flag = if let Some(field_guard) = field_guard {
                            (*field_guard)(&resolver_context)
                        } else {
                            GuardAction::Allow
                        };
                        if let GuardAction::Block(reason) = field_guard_flag {
                            return Err::<Option<_>, async_graphql::Error>(
                                guard_error(reason, "Field guard triggered."),
                            );
                        }
                    }

                    let result = mutation_fn(&resolver_context, input_object).await?;

                    Ok(Some(FieldValue::owned_any(result)))
                })
            },
        )
        .argument(InputValue::new(
            &context.entity_create_one_mutation.data_field,
            TypeRef::named_nn(entity_input_builder.insert_type_name::<T>()),
        ))
    }
}

pub fn prepare_active_model<T, A>(
    entity_input_builder: &EntityInputBuilder,
    entity_object_builder: &EntityObjectBuilder,
    input_object: &ObjectAccessor<'_>,
) -> async_graphql::Result<A>
where
    T: EntityTrait,
    <T as EntityTrait>::Model: IntoActiveModel<A>,
    A: ActiveModelTrait<Entity = T> + sea_orm::ActiveModelBehavior + Send,
{
    let mut data = entity_input_builder.parse_object::<T>(input_object)?;

    let mut active_model = A::default();

    for column in T::Column::iter() {
        // used to skip auto created primary keys
        let auto_increment = match <T::PrimaryKey as PrimaryKeyToColumn>::from_column(column) {
            Some(_) => T::PrimaryKey::auto_increment(),
            None => false,
        };

        if auto_increment {
            continue;
        }

        match data.remove(&entity_object_builder.column_name::<T>(&column)) {
            Some(value) => {
                active_model.try_set(column, value)?;
            }
            None => continue,
        }
    }

    Ok(active_model)
}

