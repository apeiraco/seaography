use std::collections::BTreeMap;

use async_graphql::dynamic::ResolverContext;

/// guards are functions that receive the application context
pub type FnGuard = Box<dyn Fn(&ResolverContext<'_>) -> GuardAction + Sync + Send>;

/// Entities and Field guards configuration.
/// The guards are used to control access to entities or fields.
#[derive(Default)]
pub struct GuardsConfig {
    /// entity guards are executed before accessing an entity
    pub entity_guards: BTreeMap<String, FnGuard>,
    /// field guards are executed before accessing an entity field
    pub field_guards: BTreeMap<String, FnGuard>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum GuardAction {
    Block(Option<String>),
    Allow,
}

pub fn guard_error(reason: Option<String>, fallback: &str) -> async_graphql::Error {
    match reason {
        Some(reason) => async_graphql::Error::new(reason),
        None => async_graphql::Error::new(fallback),
    }
}
