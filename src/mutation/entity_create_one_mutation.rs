use std::{future::Future, pin::Pin, sync::Arc};

use async_graphql::dynamic::{
    Field, FieldFuture, FieldValue, InputValue, ObjectAccessor, ResolverContext, TypeRef,
};
use sea_orm::{
    ActiveModelTrait, DatabaseConnection, EntityTrait, IntoActiveModel, Iterable,
    PrimaryKeyToColumn, PrimaryKeyTrait, TransactionTrait,
};

use crate::{
    BuilderContext, EntityInputBuilder, EntityObjectBuilder, EntityQueryFieldBuilder, GuardAction,
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
    pub fn type_name<T>(context: &BuilderContext) -> String
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        format!(
            "{}{}",
            EntityQueryFieldBuilder::type_name::<T>(context),
            context.entity_create_one_mutation.mutation_suffix
        )
    }

    pub fn to_field_with_mutation_fn<T>(&self, mutation_fn: CreateOneMutationFn<T::Model>) -> Field
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
            TypeRef::named_nn(EntityObjectBuilder::basic_type_name::<T>(context)),
            move |resolver_context| {
                let mutation_fn = mutation_fn.clone();

                FieldFuture::new(async move {
                    let guard_flag = if let Some(guard) = guard {
                        (*guard)(&resolver_context)
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

                    let value_accessor = resolver_context
                        .args
                        .get(&context.entity_create_one_mutation.data_field)
                        .unwrap();
                    let input_object = value_accessor.object()?;

                    for (column, _) in input_object.iter() {
                        let field_guard = field_guards.get(&format!(
                            "{}.{}",
                            EntityObjectBuilder::type_name::<T>(context),
                            column
                        ));
                        let field_guard_flag = if let Some(field_guard) = field_guard {
                            (*field_guard)(&resolver_context)
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

                    let result = mutation_fn(&resolver_context, input_object).await?;

                    Ok(Some(FieldValue::owned_any(result)))
                })
            },
        )
        .argument(InputValue::new(
            &context.entity_create_one_mutation.data_field,
            TypeRef::named_nn(EntityInputBuilder::insert_type_name::<T>(context)),
        ))
    }

    pub fn default_mutation_fn<T, A>(
        &self,
        active_model_hooks: bool,
    ) -> CreateOneMutationFn<T::Model>
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
        <T as EntityTrait>::Model: IntoActiveModel<A>,
        A: ActiveModelTrait<Entity = T> + sea_orm::ActiveModelBehavior + std::marker::Send,
    {
        let context = self.context;
        Arc::new(move |resolve_context, input_object| {
            Box::pin(async move {
                let active_model =
                    prepare_active_model::<T, A>(context, &input_object, resolve_context)?;
                let db = resolve_context.data::<DatabaseConnection>()?;

                if active_model_hooks {
                    let transaction = db.begin().await?;

                    let active_model = active_model.before_save(&transaction, true).await?;

                    let result: T::Model = active_model.insert(&transaction).await?;

                    let result = A::after_save(result, &transaction, true).await?;

                    transaction.commit().await?;

                    Ok(result)
                } else {
                    let result: T::Model = active_model.insert(db).await?;

                    Ok(result)
                }
            })
        })
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

pub fn prepare_active_model<T, A>(
    context: &BuilderContext,
    input_object: &ObjectAccessor<'_>,
    resolver_context: &ResolverContext<'_>,
) -> async_graphql::Result<A>
where
    T: EntityTrait,
    <T as EntityTrait>::Model: Sync,
    <T as EntityTrait>::Model: IntoActiveModel<A>,
    A: ActiveModelTrait<Entity = T> + sea_orm::ActiveModelBehavior + std::marker::Send,
{
    let mut data = EntityInputBuilder::parse_object::<T>(context, resolver_context, input_object)?;

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

        match data.remove(&EntityObjectBuilder::column_name::<T>(context, &column)) {
            Some(value) => {
                active_model.set(column, value);
            }
            None => continue,
        }
    }

    Ok(active_model)
}
