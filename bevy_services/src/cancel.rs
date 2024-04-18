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

use bevy::prelude::Entity;

use std::sync::Arc;

/// Response type that gets sent when a cancellation occurs.
#[derive(Debug)]
pub struct Cancelled<Signal> {
    pub signal: Signal,
    pub cancellation: Cancellation,
}

/// Information about the cancellation that occurred.
#[derive(Debug)]
pub struct Cancellation {
    pub cause: Arc<CancellationCause>,
}

/// Get an explanation for why a cancellation occurred. In most cases the
/// entities provided by these enums will already be despawned by the time you
/// receive this information, but it may be useful to look at them if you need
/// to debug.
#[derive(Debug)]
pub enum CancellationCause {
    /// The target at the end of a chain is unused, meaning a chain was built
    /// but the builder did not end with a [`Chain::detach()`] or a
    /// [`Chain::take()`]. The entity provided in the variant is the unused
    /// target.
    UnusedTarget(Entity),

    /// A [`Service`](crate::Service) provider needed by the chain was despawned
    /// or had a critical component removed. The entity provided in the variant
    /// is the unavailable service.
    ServiceUnavailable(Entity),

    /// The final target of the chain was dropped without detaching, which
    /// implyies that this chain is no longer needed.
    TargetDropped(Entity),

    /// Async services with serial delivery will queue up requests to deliver
    /// them one at a time. Depending on the [label settings](crate::LabelBuilder)
    /// of the incoming requests, a new request might supplant an earlier one,
    /// causing the earlier request to be cancelled.
    Supplanted(Supplanted),

    /// A link in the chain was broken, for example despawned or missing a
    /// component. This type of cancellation indicates that you are modifying
    /// the entities in chain in an unsupported way. If you believe that you are
    /// not doing anything unsupported then this could indicate a bug in
    /// `bevy_impulse`, and you encouraged to open an issue with a minimal
    /// reproducible example.
    ///
    /// The entity provided in the variant is the link where the breakage was
    /// detected.
    BrokenLink(Entity),

    /// A link in the chain filtered out a response.
    Filtered(Entity),

    /// All the branches of a fork were cancelled.
    ForkCancelled(ForkCancelled),

    /// A join was cancelled due to one of these scenarios:
    /// * At least one of its inputs was cancelled
    /// * At least one of its inputs was delivered but one or more of the inputs
    ///   were cancelled or disposed.
    ///
    /// Note that if all inputs for the join are disposed instead of cancelled,
    /// then the join will disposed and not cancelled.
    JoinCancelled(JoinCancelled),

    /// A race was cancelled because all of its inputs were either cancelled or
    /// disposed, with at least one of them being a cancel.
    ///
    /// Note that if all of the inputs for a race are disposed instead of
    /// cancelled, then the race will be disposed and not cancelled.
    RaceCancelled(RaceCancelled),
}

#[derive(Debug)]
pub struct Supplanted {
    /// Entity of the link in the chain that was supplanted
    pub cancelled_at: Entity,
    /// Entity of the link in a different chain that did the supplanting
    pub supplanter: Entity,
}

impl From<Supplanted> for CancellationCause {
    fn from(value: Supplanted) -> Self {
        CancellationCause::Supplanted(value)
    }
}

/// A description of why a fork was cancelled.
#[derive(Debug, Clone)]
pub struct ForkCancelled {
    /// The source link of the fork
    pub fork: Entity,
    /// The cancellation cause of each downstream branch of a fork.
    pub cancelled: Vec<Arc<CancellationCause>>,
}

/// A description of why a join was cancelled.
#[derive(Debug, Clone)]
pub struct JoinCancelled {
    /// The source link of the join
    pub join: Entity,
    /// The inputs of the join which were delivered
    pub delivered: Vec<Entity>,
    /// The inputs of the join which were waiting for a delivery (not delivered,
    /// not cancelled, and not disposed)
    pub pending: Vec<Entity>,
    /// THe inputs of the join which were disposed
    pub disposals: Vec<Entity>,
    /// The inputs of the join which were cancelled
    pub cancellations: Vec<Arc<CancellationCause>>,
}

impl From<JoinCancelled> for CancellationCause {
    fn from(value: JoinCancelled) -> Self {
        CancellationCause::JoinCancelled(value)
    }
}

/// A description of why a race was cancelled.
#[derive(Debug, Clone)]
pub struct RaceCancelled {
    /// The source link of the race
    pub race: Entity,
    /// The inputs of the race that were disposed
    pub disposals: Vec<Entity>,
    /// The inputs of the race that were cancelled
    pub cancellations: Vec<Arc<CancellationCause>>,
}

impl From<RaceCancelled> for CancellationCause {
    fn from(value: RaceCancelled) -> Self {
        CancellationCause::RaceCancelled(value)
    }
}

/// Passed into the [`OperationRoster`](crate::OperationRoster) to indicate when
/// a link needs to be cancelled.
pub struct Cancel {
    pub apply_to: Entity,
    pub cause: Arc<CancellationCause>,
}

impl Cancel {
    /// Create a new [`Cancel`] operation
    pub fn new(apply_to: Entity, cause: CancellationCause) -> Self {
        Self { apply_to, cause: Arc::new(cause) }
    }

    /// Create a broken link cancel operation
    pub fn broken(from: Entity) -> Self {
        Self::new(from, CancellationCause::BrokenLink(from))
    }

    /// Create an unavailable service cancel operation
    pub fn service_unavailable(source: Entity, service: Entity) -> Self {
        Self::new(source, CancellationCause::ServiceUnavailable(service))
    }

    /// Create a supplanted request cancellation operation
    pub fn supplanted(cancelled_at: Entity, supplanter: Entity) -> Self {
        Self::new(cancelled_at, Supplanted { cancelled_at, supplanter }.into())
    }

    /// Create an unused target cancel operation
    pub fn unused_target(target: Entity) -> Self {
        Self::new(target, CancellationCause::UnusedTarget(target))
    }

    /// Create a dropped target cancel operation
    pub fn dropped(target: Entity) -> Self {
        Self::new(target, CancellationCause::TargetDropped(target))
    }

    pub fn filtered(source: Entity) -> Self {
        Self::new(source, CancellationCause::Filtered(source))
    }
}
