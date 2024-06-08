/*
 * Copyright (C) 2024 Open Source Robotics Foundation
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 *
*/

use bevy::{
    prelude::{Entity, Component, World},
    ecs::world::{EntityMut, EntityRef},
};

use backtrace::Backtrace;

use std::sync::Arc;

use std::collections::HashMap;

use crate::{
    OperationRoster, operation::ScopeStorage, Cancellation, UnhandledErrors,
};

#[derive(Debug, Clone)]
pub struct Disposal {
    pub cause: Arc<DisposalCause>,
}

impl<T: Into<DisposalCause>> From<T> for Disposal {
    fn from(value: T) -> Self {
        Disposal { cause: Arc::new(value.into())}
    }
}

impl Disposal {
    pub fn service_unavailable(service: Entity, for_node: Entity) -> Disposal {
        ServiceUnavailable { service, for_node }.into()
    }

    pub fn branching(
        branched_at_node: Entity,
        disposed_for_target: Entity,
        reason: Option<anyhow::Error>,
    ) -> Disposal {
        DisposedBranch { branched_at_node, disposed_for_target, reason }.into()
    }

    pub fn supplanted(
        supplanted_at_node: Entity,
        supplanted_by_node: Entity,
        supplanting_session: Entity,
    ) -> Disposal {
        Supplanted { supplanted_at_node, supplanted_by_node, supplanting_session }.into()
    }
}

#[derive(Debug)]
pub enum DisposalCause {
    /// Some services will queue up requests to deliver them one at a time.
    /// Depending on the label of the incoming requests, a new request might
    /// supplant an earlier one, causing the earlier request to be disposed.
    Supplanted(Supplanted),

    /// A node filtered out a response.
    Filtered(Filtered),

    /// A node disposed of one of its output branches.
    Branching(DisposedBranch),

    /// A join was halted because one or more of its inputs became unreachable.
    JoinImpossible(JoinImpossible),

    /// A [`Service`](crate::Service) provider needed by the chain was despawned
    /// or had a critical component removed. The entity provided in the variant
    /// is the unavailable service.
    ServiceUnavailable(ServiceUnavailable),

    /// An output was disposed because a mutex was poisoned.
    PoisonedMutex(PoisonedMutexDisposal),

    /// A scope was cancelled so its output has been disposed.
    Scope(Cancellation),
}

/// A variant of [`DisposalCause`]
#[derive(Debug)]
pub struct Supplanted {
    /// ID of the node whose service request was supplanted
    pub supplanted_at_node: Entity,
    /// ID of the node that did the supplanting
    pub supplanted_by_node: Entity,
    /// ID of the session that did the supplanting
    pub supplanting_session: Entity,
}

impl Supplanted {
    pub fn new(
        cancelled_at_node: Entity,
        supplanting_node: Entity,
        supplanting_session: Entity,
    ) -> Self {
        Self { supplanted_at_node: cancelled_at_node, supplanted_by_node: supplanting_node, supplanting_session }
    }
}

impl From<Supplanted> for DisposalCause {
    fn from(value: Supplanted) -> Self {
        DisposalCause::Supplanted(value)
    }
}

/// A variant of [`DisposalCause`]
#[derive(Debug)]
pub struct Filtered {
    /// ID of the node that did the filtering
    pub filtered_at_node: Entity,
    /// Optionally, a reason given for why the filtering happened.
    pub reason: Option<anyhow::Error>,
}

impl Filtered {
    pub fn new(filtered_at_node: Entity, reason: Option<anyhow::Error>) -> Self {
        Self { filtered_at_node, reason }
    }
}

impl From<Filtered> for DisposalCause {
    fn from(value: Filtered) -> Self {
        Self::Filtered(value)
    }
}

/// A variant of [`DisposalCause`]
#[derive(Debug)]
pub struct DisposedBranch {
    /// The node where the branching happened
    pub branched_at_node: Entity,
    /// The target node whose input was disposed
    pub disposed_for_target: Entity,
    /// Optionally, a reason given for the branching
    pub reason: Option<anyhow::Error>,
}

impl From<DisposedBranch> for DisposalCause {
    fn from(value: DisposedBranch) -> Self {
        Self::Branching(value)
    }
}

/// A variant of [`DisposalCause`]
#[derive(Debug)]
pub struct JoinImpossible {
    /// The source node of the join
    pub join: Entity,
    /// The unreachable input nodes
    pub unreachable: Vec<Entity>,
}

impl From<JoinImpossible> for DisposalCause {
    fn from(value: JoinImpossible) -> Self {
        DisposalCause::JoinImpossible(value)
    }
}

/// A variant of [`DisposalCause`]
#[derive(Debug)]
pub struct ServiceUnavailable {
    /// The service that is no longer available
    pub service: Entity,
    /// The node that intended to use the service
    pub for_node: Entity,
}

impl From<ServiceUnavailable> for DisposalCause {
    fn from(value: ServiceUnavailable) -> Self {
        Self::ServiceUnavailable(value)
    }
}

/// A variant of [`DisposalCause`]
#[derive(Debug)]
pub struct PoisonedMutexDisposal {
    /// The node containing the poisoned mutex
    pub for_node: Entity,
}

impl From<PoisonedMutexDisposal> for DisposalCause {
    fn from(value: PoisonedMutexDisposal) -> Self {
        Self::PoisonedMutex(value)
    }
}

pub trait ManageDisposal {
    fn emit_disposal(
        &mut self,
        session: Entity,
        disposal: Disposal,
        roster: &mut OperationRoster,
    );

    fn clear_disposals(&mut self, session: Entity);
}

pub trait InspectDisposals {
    fn get_disposals(&self, session: Entity) -> Option<&Vec<Disposal>>;
}

impl<'w> ManageDisposal for EntityMut<'w> {
    fn emit_disposal(
        &mut self,
        session: Entity,
        disposal: Disposal,
        roster: &mut OperationRoster,
    ) {
        let Some(scope) = self.get::<ScopeStorage>() else {
            let broken_node = self.id();
            self.world_scope(|world| {
                world
                .get_resource_or_insert_with(|| UnhandledErrors::default())
                .disposals
                .push(DisposalFailure {
                    disposal, broken_node, backtrace: Some(Backtrace::new())
                });
            });
            return;
        };
        let scope = scope.get();

        if let Some(mut storage) = self.get_mut::<DisposalStorage>() {
            storage.disposals.entry(session).or_default().push(disposal);
        } else {
            let mut storage = DisposalStorage::default();
            storage.disposals.entry(session).or_default().push(disposal);
            self.insert(storage);
        }

        roster.disposed(scope, session);
    }

    fn clear_disposals(&mut self, session: Entity) {
        if let Some(mut storage) = self.get_mut::<DisposalStorage>() {
            storage.disposals.remove(&session);
        }
    }
}

impl<'w> InspectDisposals for EntityMut<'w> {
    fn get_disposals(&self, session: Entity) -> Option<&Vec<Disposal>> {
        if let Some(storage) = self.get::<DisposalStorage>() {
            return storage.disposals.get(&session);
        }

        None
    }
}

impl<'w> InspectDisposals for EntityRef<'w> {
    fn get_disposals(&self, session: Entity) -> Option<&Vec<Disposal>> {
        if let Some(storage) = self.get::<DisposalStorage>() {
            return storage.disposals.get(&session);
        }

        None
    }
}

pub fn emit_disposal(
    source: Entity,
    session: Entity,
    disposal: Disposal,
    world: &mut World,
    roster: &mut OperationRoster,
) {
    if let Some(mut source_mut) = world.get_entity_mut(source) {
        source_mut.emit_disposal(session, disposal, roster);
    } else {
        world
        .get_resource_or_insert_with(|| UnhandledErrors::default())
        .disposals
        .push(DisposalFailure {
            disposal,
            broken_node: source,
            backtrace: Some(Backtrace::new()),
        });
    }
}

#[derive(Component, Default)]
struct DisposalStorage {
    /// A map from a session to all the disposals that occurred for the session
    disposals: HashMap<Entity, Vec<Disposal>>,
}

/// When it is impossible for some reason to perform a disposal, the incident
/// will be logged in this resource. This may happen if a node somehow gets
/// despawned while its service is attempting to dispose a request.
pub struct DisposalFailure {
    /// The disposal that was attempted
    pub disposal: Disposal,
    /// The node which was attempting to report the disposal
    pub broken_node: Entity,
    /// The backtrace indicating what led up to the failure
    pub backtrace: Option<Backtrace>,
}
