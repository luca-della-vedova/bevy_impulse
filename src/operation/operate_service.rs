/*
 * Copyright (C) 2023 Open Source Robotics Foundation
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

use crate::{
    Operation, SingleTargetStorage, Service, OperationRoster, ServiceRequest,
    SingleInputStorage, dispatch_service, Cancel, OperationCleanup,
    OperationResult, OrBroken, OperationSetup, OperationRequest,
    ActiveTasksStorage, OperationReachability, ReachabilityResult,
    InputBundle,
};

use bevy::{
    prelude::{Component, Entity, World, Query},
    ecs::system::SystemState,
};

pub(crate) struct OperateService<Request> {
    provider: Entity,
    target: Entity,
    _ignore: std::marker::PhantomData<Request>,
}

impl<Request: 'static + Send + Sync> OperateService<Request> {
    pub(crate) fn new<Response, Streams>(
        provider: Service<Request, Response, Streams>,
        target: Entity,
    ) -> Self {
        Self {
            provider: provider.get(),
            target,
            _ignore: Default::default(),
        }
    }
}

impl<Request: 'static + Send + Sync> Operation for OperateService<Request> {
    fn setup(self, OperationSetup { source, world }: OperationSetup) -> OperationResult {
        world.get_entity_mut(self.target).or_broken()?
            .insert(SingleInputStorage::new(source));

        world.entity_mut(source).insert((
            InputBundle::<Request>::new(),
            ProviderStorage(self.provider),
            SingleTargetStorage(self.target),
            ActiveTasksStorage::default(),
        ));
        Ok(())
    }

    fn execute(operation: OperationRequest) -> OperationResult {
        let source_ref = operation.world.get_entity(operation.source).or_broken()?;
        let target = source_ref.get::<SingleTargetStorage>().or_broken()?.0;
        let provider = source_ref.get::<ProviderStorage>().or_broken()?.0;

        dispatch_service(ServiceRequest { provider, target, operation });
        Ok(())
    }

    fn cleanup(mut clean: OperationCleanup) -> OperationResult {
        clean.cleanup_inputs::<Request>()?;
        ActiveTasksStorage::cleanup(clean)
    }

    fn is_reachable(reachability: OperationReachability) -> ReachabilityResult {
        if reachability.has_input::<Request>()? {
            return Ok(true);
        }
        if ActiveTasksStorage::contains_session(reachability)? {
            return Ok(true);
        }
        SingleInputStorage::is_reachable(reachability)
    }
}

#[derive(Component)]
struct ProviderStorage(Entity);

pub(crate) fn cancel_service(
    cancelled_provider: Entity,
    world: &mut World,
    roster: &mut OperationRoster,
) {
    let mut providers_state: SystemState<Query<(Entity, &ProviderStorage)>> =
        SystemState::new(world);
    let providers = providers_state.get(world);
    for (source, ProviderStorage(provider)) in &providers {
        if *provider == cancelled_provider {
            roster.cancel(Cancel::service_unavailable(source, cancelled_provider));
        }
    }
}
