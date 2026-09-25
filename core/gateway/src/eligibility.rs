//! Minimal candidate filtering (§8.2 subset).
//!
//! Given a manifest, the caller's view of each route's health, and what the call requires, this
//! module returns the routes eligible to serve it. Scoring within that eligible set (§8.3) is
//! deferred to a later milestone: [`eligible_routes`] returns candidates in manifest order and
//! decides nothing about which of several eligible routes is best.

use std::collections::HashMap;

use crate::manifest::{BillingMode, ModelRoute, RouteManifest, WorkProfile};
use crate::taxonomy::RouteHealth;

/// What a call needs from a route.
///
/// `subscription_only` is a user control (§19): when true, it excludes
/// [`BillingMode::PerToken`] routes so a call that must stay inside a subscription quota can never
/// be routed to a route that spends BYOK budget instead.
#[derive(Clone, Copy, Debug)]
pub struct Requirements {
    pub profile: WorkProfile,
    pub subscription_only: bool,
}

/// Returns the routes in `manifest` eligible to serve `requirements`, in manifest order.
///
/// This crate holds no health registry of its own — `health` is supplied by the caller, keyed by
/// [`ModelRoute::id`]. A route is eligible only when all of the following hold (§8.2 subset
/// implemented in this milestone):
///
/// - it is `enabled` in the manifest;
/// - `health` maps its id to [`RouteHealth::Available`] or [`RouteHealth::Degraded`] — any other
///   state, including the route being absent from `health` entirely, excludes it: an unknown
///   health is not an available one;
/// - it advertises `requirements.profile` among its [`ModelRoute::profiles`];
/// - it is not a [`BillingMode::PerToken`] route while `requirements.subscription_only` is true.
///
/// Full §8.2 candidate filtering (policy, capability, data-handling, provider-independence and
/// context-size checks) and §8.3 scoring are out of scope for this milestone; see the plan's
/// "Deferred, stated" list.
#[must_use]
pub fn eligible_routes<'a>(
    manifest: &'a RouteManifest,
    health: &HashMap<String, RouteHealth>,
    requirements: &Requirements,
) -> Vec<&'a ModelRoute> {
    manifest
        .routes()
        .iter()
        .filter(|route| route.enabled())
        .filter(|route| {
            health.get(route.id()).is_some_and(|state| {
                matches!(state, RouteHealth::Available | RouteHealth::Degraded)
            })
        })
        .filter(|route| route.profiles().contains(&requirements.profile))
        .filter(|route| {
            !(requirements.subscription_only && route.billing_mode() == BillingMode::PerToken)
        })
        .collect()
}
