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
    OperationRoster, StreamPack, AddOperation, OperateService, Provider,
    OperationRequest, PendingOperationRequest, dispose_for_despawned_service,
};

use bevy::prelude::{Entity, App, Commands, World, Component, Bundle, Resource};

mod async_srv;
pub use async_srv::*;

mod blocking;
pub use blocking::*;

mod builder;
pub use builder::ServiceBuilder;

pub(crate) mod delivery;
pub(crate) use delivery::*;

mod traits;
pub use traits::*;

mod workflow;
pub use workflow::*;

use std::collections::VecDeque;

use crossbeam::channel::{unbounded, Sender as CbSender, Receiver as CbReceiver};

pub struct ServiceRequest<'a> {
    /// The entity that holds the service that is being used.
    pub(crate) provider: Entity,
    pub(crate) target: Entity,
    pub(crate) operation: OperationRequest<'a>,
}

impl<'a> ServiceRequest<'a> {
    fn pend(self) -> PendingServiceRequest {
        PendingServiceRequest {
            provider: self.provider,
            target: self.target,
            operation: self.operation.pend(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct PendingServiceRequest {
    pub provider: Entity,
    pub target: Entity,
    pub operation: PendingOperationRequest,
}

impl PendingServiceRequest {
    fn activate<'a>(
        self,
        world: &'a mut World,
        roster: &'a mut OperationRoster
    ) -> ServiceRequest<'a> {
        ServiceRequest {
            provider: self.provider,
            target: self.target,
            operation: self.operation.activate(world, roster),
        }
    }
}

#[derive(Component)]
pub(crate) struct ServiceMarker<Request, Response> {
    _ignore: std::marker::PhantomData<(Request, Response)>,
}

impl<Request, Response> Default for ServiceMarker<Request, Response> {
    fn default() -> Self {
        Self { _ignore: Default::default() }
    }
}

#[derive(Component)]
pub(crate) struct ServiceHook {
    pub(crate) trigger: fn(ServiceRequest),
    pub(crate) lifecycle: Option<ServiceLifecycle>,
}

impl ServiceHook {
    pub(crate) fn new(callback: fn(ServiceRequest)) -> Self {
        Self { trigger: callback, lifecycle: None }
    }
}

/// Keeps track of when a service entity gets despawned so we know to cancel
/// any pending requests
pub(crate) struct ServiceLifecycle {
    /// The entity that this is attached to
    entity: Entity,
    /// Used to send the signal that the service has despawned
    sender: CbSender<Entity>,
}

impl ServiceLifecycle {
    pub(crate) fn new(entity: Entity, sender: CbSender<Entity>) -> Self {
        Self { entity, sender }
    }
}

impl Drop for ServiceLifecycle {
    fn drop(&mut self) {
        self.sender.send(self.entity).ok();
    }
}

#[derive(Resource, Clone)]
pub(crate) struct ServiceLifecycleQueue {
    pub(crate) sender: CbSender<Entity>,
    pub(crate) receiver: CbReceiver<Entity>,
}

impl ServiceLifecycleQueue {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = unbounded();
        Self { sender, receiver }
    }
}

impl Default for ServiceLifecycleQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Bundle)]
pub(crate) struct ServiceBundle<Srv: ServiceTrait + 'static + Send + Sync> {
    hook: ServiceHook,
    marker: ServiceMarker<Srv::Request, Srv::Response>,
}

impl<Srv: ServiceTrait + 'static + Send + Sync> ServiceBundle<Srv> {
    fn new() -> Self {
        Self {
            hook: ServiceHook::new(service_hook::<Srv>),
            marker: Default::default(),
        }
    }
}

fn service_hook<Srv: ServiceTrait>(request: ServiceRequest) {
    Srv::serve(request);
}

/// Provider is the public API handle for referring to an existing service
/// provider. Downstream users can obtain a Provider using
/// - [`crate::ServiceDiscovery`].iter()
/// - [`bevy::prelude::App`].add_*_service(~)
/// - [`bevy::prelude::Commands`].spawn_*_service(~)
///
/// To use a provider, call [`bevy::prelude::Commands`].request(provider, request).
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Service<Request, Response, Streams = ()> {
    entity: Entity,
    _ignore: std::marker::PhantomData<(Request, Response, Streams)>,
}

impl<Req, Res, S> Clone for Service<Req, Res, S> {
    fn clone(&self) -> Self {
        Self { entity: self.entity, _ignore: Default::default() }
    }
}

impl<Req, Res, S> Copy for Service<Req, Res, S> { }

impl<Request, Response, Streams> Service<Request, Response, Streams> {
    /// Get the underlying entity that the service provider is associated with.
    pub fn get(&self) -> Entity {
        self.entity
    }

    /// This can only be used internally. To obtain a Provider, use one of the
    /// following:
    /// - App::add_*_service
    /// - Commands::spawn_*_service
    /// - Commands::spawn_workflow
    /// - ServiceDiscovery::iter()
    fn new(entity: Entity) -> Self {
        Self { entity, _ignore: Default::default() }
    }
}

/// This trait extends the Commands interface so that services can spawned from
/// any system.
pub trait SpawnServicesExt<'w, 's> {
    /// Call this with Commands to create a new async service from any system.
    fn spawn_service<'a, M1, M2, B: IntoServiceBuilder<M1, Also=()>>(
        &'a mut self,
        builder: B,
    ) -> Service<
            <B::Service as IntoService<M2>>::Request,
            <B::Service as IntoService<M2>>::Response,
            <B::Service as IntoService<M2>>::Streams,
        >
    where
        B::Service: IntoService<M2>,
        B::Deliver: DeliveryChoice,
        B::With: WithEntityCommands,
        <B::Service as IntoService<M2>>::Request: 'static + Send + Sync,
        <B::Service as IntoService<M2>>::Response: 'static + Send + Sync,
        <B::Service as IntoService<M2>>::Streams: StreamPack;
}

impl<'w, 's> SpawnServicesExt<'w, 's> for Commands<'w, 's> {
    fn spawn_service<'a, M1, M2, B: IntoServiceBuilder<M1, Also=()>>(
        &'a mut self,
        builder: B,
    ) -> Service<
            <B::Service as IntoService<M2>>::Request,
            <B::Service as IntoService<M2>>::Response,
            <B::Service as IntoService<M2>>::Streams,
        >
    where
        B::Service: IntoService<M2>,
        B::Deliver: DeliveryChoice,
        B::With: WithEntityCommands,
        <B::Service as IntoService<M2>>::Request: 'static + Send + Sync,
        <B::Service as IntoService<M2>>::Response: 'static + Send + Sync,
        <B::Service as IntoService<M2>>::Streams: StreamPack,
    {
        builder.into_service_builder().spawn_service(self)
    }
}

/// This trait extends the App interface so that services can be added while
/// configuring an App.
pub trait AddServicesExt {
    /// Call this on an App to create a service that is available immediately.
    fn add_service<M1, M2, B: IntoServiceBuilder<M1>>(&mut self, builder: B) -> &mut Self
    where
        B::Service: IntoService<M2>,
        B::Deliver: DeliveryChoice,
        B::With: WithEntityMut,
        B::Also: AlsoAdd<
                <B::Service as IntoService<M2>>::Request,
                <B::Service as IntoService<M2>>::Response,
                <B::Service as IntoService<M2>>::Streams
            >,
        <B::Service as IntoService<M2>>::Request: 'static + Send + Sync,
        <B::Service as IntoService<M2>>::Response: 'static + Send + Sync,
        <B::Service as IntoService<M2>>::Streams: StreamPack;
}

impl AddServicesExt for App {
    fn add_service<M1, M2, B: IntoServiceBuilder<M1>>(&mut self, builder: B) -> &mut Self
    where
        B::Service: IntoService<M2>,
        B::Deliver: DeliveryChoice,
        B::With: WithEntityMut,
        B::Also: AlsoAdd<
                <B::Service as IntoService<M2>>::Request,
                <B::Service as IntoService<M2>>::Response,
                <B::Service as IntoService<M2>>::Streams
            >,
        <B::Service as IntoService<M2>>::Request: 'static + Send + Sync,
        <B::Service as IntoService<M2>>::Response: 'static + Send + Sync,
        <B::Service as IntoService<M2>>::Streams: StreamPack
    {
        builder.into_service_builder().add_service(self);
        self
    }
}

#[derive(Resource)]
struct ServiceQueue {
    is_delivering: bool,
    queue: VecDeque<PendingServiceRequest>,
}

impl ServiceQueue {
    fn new() -> Self {
        Self { is_delivering: false, queue: VecDeque::new() }
    }
}

pub(crate) fn dispatch_service(
    ServiceRequest {
        provider,
        target,
        operation: OperationRequest { source, world, roster }
    }: ServiceRequest,
) {
    let pending = PendingServiceRequest {
        provider, target, operation: PendingOperationRequest { source }
    };
    let mut service_queue = world.get_resource_or_insert_with(|| ServiceQueue::new());
    service_queue.queue.push_back(pending);
    if service_queue.is_delivering {
        // Services are already being delivered, so to keep things simple we
        // will add this dispatch command to the service queue and let the
        // services be processed one at a time. Otherwise service recursion gets
        // messy or even impossible.
        return;
    }

    service_queue.is_delivering = true;

    while let Some(pending) = world.resource_mut::<ServiceQueue>().queue.pop_back() {
        let Some(hook) = world.get::<ServiceHook>(pending.provider) else {
            // The service has become unavailable, so we should drain the source
            // node of all its inputs, emitting disposals for all of them.
            dispose_for_despawned_service(provider, world, roster);
            continue;
        };

        (hook.trigger)(pending.activate(world, roster));
    }
    world.resource_mut::<ServiceQueue>().is_delivering = false;
}

impl<Request, Response, Streams> Provider for Service<Request, Response, Streams>
where
    Request: 'static + Send + Sync,
{
    type Request = Request;
    type Response = Response;
    type Streams = Streams;

    fn provide(self, source: Entity, target: Entity, commands: &mut Commands) {
        commands.add(AddOperation::new(source, OperateService::new(self, target)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockingService, InBlockingService, AsyncService, InAsyncService};
    use bevy::{
        prelude::*,
        ecs::world::EntityMut,
    };
    use std::future::Future;

    #[derive(Component)]
    struct TestPeople {
        name: String,
        age: u64,
    }

    #[derive(Component)]
    struct Multiplier(u64);

    #[derive(Resource)]
    struct TestSystemRan(bool);

    #[derive(Resource)]
    struct MyServiceProvider {
        #[allow(unused)]
        provider: Service<String, u64>,
    }

    #[test]
    fn test_spawn_async_service() {
        let mut app = App::new();
        app
            .insert_resource(TestSystemRan(false))
            .add_systems(Startup, sys_spawn_async_service)
            .add_systems(Update, sys_find_service);

        app.update();
        assert!(app.world.resource::<TestSystemRan>().0);
    }

    #[test]
    fn test_add_async_service() {
        let mut app = App::new();
        app
            .insert_resource(TestSystemRan(false))
            .add_service(sys_async_service)
            .add_systems(Update, sys_find_service);

        app.update();
        assert!(app.world.resource::<TestSystemRan>().0);
    }

    #[test]
    fn test_add_async_service_serial() {
        let mut app = App::new();
        app
            .insert_resource(TestSystemRan(false))
            .add_service(sys_async_service.serial())
            .add_systems(Update, sys_find_service);

        app.update();
        assert!(app.world.resource::<TestSystemRan>().0);
    }

    #[test]
    fn test_add_built_async_service() {
        let mut app = App::new();
        app
            .insert_resource(TestSystemRan(false))
            .add_service(
                sys_async_service
                .also(|app: &mut App, provider| {
                    app.insert_resource(MyServiceProvider { provider });
                })
            )
            .add_systems(Update, sys_use_my_service_provider);

        app.update();
        assert!(app.world.resource::<TestSystemRan>().0);
    }

    #[test]
    fn test_spawn_blocking_service() {
        let mut app = App::new();
        app
            .insert_resource(TestSystemRan(false))
            .add_systems(Startup, sys_spawn_blocking_service)
            .add_systems(Update, sys_find_service);

        app.update();
        assert!(app.world.resource::<TestSystemRan>().0);
    }

    #[test]
    fn test_add_simple_blocking_service() {
        let mut app = App::new();
        app
            .insert_resource(TestSystemRan(false))
            .add_service(sys_blocking_system.into_blocking_service())
            .add_systems(Update, sys_find_service);

        app.update();
        assert!(app.world.resource::<TestSystemRan>().0);
    }

    #[test]
    fn test_add_self_aware_blocking_service() {
        let mut app = App::new();
        app
            .insert_resource(TestSystemRan(false))
            .add_service(
                sys_blocking_service
                .with(|mut entity_mut: EntityMut| {
                    entity_mut.insert(Multiplier(2));
                })
            )
            .add_systems(Update, sys_find_service);

        app.update();
        assert!(app.world.resource::<TestSystemRan>().0);
    }

    fn sys_async_service(
        In(AsyncService{ request, .. }): InAsyncService<String>,
        people: Query<&TestPeople>,
    ) -> impl Future<Output=u64> {
        let mut matching_people = Vec::new();
        for person in &people {
            if person.name == request {
                matching_people.push(person.age);
            }
        }

        async move {
            matching_people.into_iter().fold(0, |sum, age| sum + age)
        }
    }

    fn sys_spawn_async_service(
        mut commands: Commands,
    ) {
        commands.spawn_service(sys_async_service);
    }

    fn sys_blocking_service(
        In(BlockingService{ request, provider }): InBlockingService<String>,
        people: Query<&TestPeople>,
        multipliers: Query<&Multiplier>,
    ) -> u64 {
        let mut sum = 0;
        let multiplier = multipliers.get(provider).unwrap().0;
        for person in &people {
            if person.name == request {
                sum += multiplier * person.age;
            }
        }
        sum
    }

    fn sys_blocking_system(
        In(name): In<String>,
        people: Query<&TestPeople>,
    ) -> u64 {
        let mut sum = 0;
        for person in &people {
            if person.name == name {
                sum += person.age;
            }
        }
        sum
    }

    fn sys_spawn_blocking_service(
        mut commands: Commands,
    ) {
        commands.spawn_service(sys_blocking_service);
    }

    fn sys_find_service(
        query: Query<&ServiceMarker<String, u64>>,
        mut ran: ResMut<TestSystemRan>,
    ) {
        assert!(!query.is_empty());
        ran.0 = true;
    }

    fn sys_use_my_service_provider(
        my_provider: Option<Res<MyServiceProvider>>,
        mut ran: ResMut<TestSystemRan>,
    ) {
        assert!(my_provider.is_some());
        ran.0 = true;
    }
}
