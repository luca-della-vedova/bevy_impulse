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

use crate::{StreamPack, AddOperation, OperateService, Provider, InputCommand};

use bevy::{
    prelude::{Entity, App, Commands, Component},
    utils::define_label,
};

mod async_srv;
pub use async_srv::*;

mod blocking;
pub use blocking::*;

mod builder;
pub use builder::ServiceBuilder;

pub(crate) mod delivery;
pub(crate) use delivery::*;

pub(crate) mod internal;
pub(crate) use internal::*;

mod traits;
pub use traits::*;

mod workflow;
pub(crate) use workflow::*;

/// [`Service`] is the public API handle for referring to an existing service
/// provider. Downstream users can obtain a Provider using
/// - [`crate::ServiceDiscovery`].iter()
/// - [`bevy::prelude::App`]`.`[`add_*_service(~)`][1]
/// - [`bevy::prelude::Commands`]`.`[`spawn_*_service(~)`][]
///
/// To use a provider, call [`bevy::prelude::Commands`]`.`[`request(provider, request)`][3].
///
/// [1]: AddservicesExt::add_service
/// [2]: SpawnServicesExt::spawn_service
/// [3]: crate::RequestExt::request
#[derive(Debug, PartialEq, Eq)]
pub struct Service<Request, Response, Streams = ()> {
    provider: Entity,
    instructions: Option<DeliveryInstructions>,
    _ignore: std::marker::PhantomData<(Request, Response, Streams)>,
}

impl<Req, Res, S> Clone for Service<Req, Res, S> {
    fn clone(&self) -> Self {
        Self {
            provider: self.provider,
            instructions: self.instructions,
            _ignore: Default::default()
        }
    }
}

impl<Req, Res, S> Copy for Service<Req, Res, S> {}

impl<Request, Response, Streams> Service<Request, Response, Streams> {
    /// Get the underlying entity that the service provider is associated with.
    pub fn provider(&self) -> Entity {
        self.provider
    }

    /// Get the delivery instructions for this service.
    pub fn instructions(&self) -> Option<&DeliveryInstructions> {
        self.instructions.as_ref()
    }

    /// Give [`DeliveryInstructions`] for this service.
    pub fn instruct(
        mut self,
        instructions: impl Into<DeliveryInstructions>,
    ) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// This can only be used internally. To obtain a Service, use one of the
    /// following:
    /// - App::add_*_service
    /// - Commands::spawn_*_service
    /// - Commands::spawn_workflow
    /// - ServiceDiscovery::iter()
    fn new(entity: Entity) -> Self {
        Self { provider: entity, instructions: None, _ignore: Default::default() }
    }
}

define_label!(
    /// A strongly-typed class of labels used to tag delivery instructions that
    /// are related to each other.
    DeliveryLabel,
    /// Strongly-typed identifier for a [`RequestLabel`].
    DeliveryLabelId,
);

/// When using a service, you can bundle in delivery instructions that affect
/// how multiple requests to the same service may interact with each other.
///
/// `DeliveryInstructions` include a [`DeliveryLabelId`]. A `DeliveryLabelId`
/// value associates different service requests with each other, and the
/// remaining instructions determine how those related requests interact.
///
/// By default when a service provider receives a new request with the same
/// [`DeliveryLabelId`] as an earlier request, the earlier request will be
/// queued until the previous requests with the same label have all finished.
///
/// To change the default behavior there are two modifiers you can apply to
/// this label:
/// - `.preempt()` asks for the request to be preempt all other requests with
///   this same label, effectively cancelling any that have not been finished yet.
/// - `.ensure()` asks for this request to not be cancelled even if a preemptive
///   request comes in with the same label. The preemptive request will instead
///   be queued after this one.
///
/// You can choose to use either, both, or neither of these modifiers in
/// whatever way fits your use case. No matter what modifiers you choose
/// (or don't choose) the same service provider will never simultaneously
/// execute its service for two requests with the same label value. To that
/// extent, applying a label always guarantees mutual exclusivity between
/// requests.
///
/// This mutual exclusivity can be useful if the service involves making
/// modifications to the world which would conflict with each other when two
/// related requests are being delivered at the same time.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeliveryInstructions {
    pub(crate) label: DeliveryLabelId,
    pub(crate) preempt: bool,
    pub(crate) ensure: bool,
}

impl DeliveryInstructions {
    /// Begin building a label for a request. You do not need to call this
    /// function explicitly. You can instead use `.preempt()` or `.ensure()`
    /// directly on a `RequestLabel` instance.
    pub fn new(label: impl DeliveryLabel) -> Self {
        Self {
            label: label.as_label(),
            preempt: false,
            ensure: false,
        }
    }

    /// See the label for these delivery instructions.
    pub fn label(&self) -> &DeliveryLabelId {
        &self.label
    }

    /// New requests will preempt earlier requests.
    ///
    /// Ordinarily when multiple requests have the same delivery label, they
    /// will queue up with each other, running one at a time in order of which
    /// request arrived first. Use this method to change the instructions so
    /// that new requests will preempt earlier requests with the same delivery
    /// label, cancelling those earlier requests if they have not finished yet.
    ///
    /// To prevent requests from being preempted you can apply [`Self::ensure`]
    /// to the delivery instructions.
    pub fn preempt(mut self) -> Self {
        self.preempt = true;
        self
    }

    /// Check whether the requests will be preemptive.
    pub fn is_preemptive(&self) -> bool {
        self.preempt
    }

    /// Decide at runtime whether the [`Self::preempt`] field will be true or false.
    pub fn with_preemptive(mut self, preempt: bool) -> Self {
        self.preempt = preempt;
        self
    }

    /// Ensure that this request is resolved even if a preemptive request with
    /// the same label arrives.
    ///
    /// The [`Self::preempt`] setting will typically cause any earlier requests
    /// with the same delivery label to be cancelled when a new request comes
    /// in. If you apply `ensure` to the instructions, then later preemptive
    /// requests will not be able to cancel, and they will get queued instead.
    pub fn ensure(mut self) -> Self {
        self.ensure = true;
        self
    }

    /// Check whether the delivery instructions are ensuring that this will be
    /// delivered.
    pub fn is_ensured(&self) -> bool {
        self.ensure
    }

    /// Decide at runtime whether the [`Self::ensure`] field will be true or
    /// false.
    pub fn with_ensured(mut self, ensure: bool) -> Self {
        self.ensure = ensure;
        self
    }
}

impl<L: DeliveryLabel> From<L> for DeliveryInstructions {
    fn from(label: L) -> Self {
        DeliveryInstructions::new(label)
    }
}

/// Allow anything that can be converted into [`DeliveryInstructions`] to have
/// access to the [`preempt`] and [`ensure`] methods.
pub trait AsDeliveryInstructions {
    /// Instruct the delivery to have [preemptive behavior][1].
    ///
    /// [1]: DeliveryInstructions::preempt
    fn preempt(self) -> DeliveryInstructions;

    /// Decide at runtime whether to be preemptive
    fn with_preemptive(self, preempt: bool) -> DeliveryInstructions;

    /// Instruct the delivery to [be ensured][1].
    ///
    /// [1]: DeliveryInstructions::ensure
    fn ensure(self) -> DeliveryInstructions;

    /// Decide at runtime whether to be ensured.
    fn with_ensured(self, ensure: bool) -> DeliveryInstructions;
}

impl<T: Into<DeliveryInstructions>> AsDeliveryInstructions for T {
    fn preempt(self) -> DeliveryInstructions {
        self.into().preempt()
    }

    fn with_preemptive(self, preempt: bool) -> DeliveryInstructions {
        self.into().with_preemptive(preempt)
    }

    fn ensure(self) -> DeliveryInstructions {
        self.into().ensure()
    }

    fn with_ensured(self, ensure: bool) -> DeliveryInstructions {
        self.into().with_ensured(ensure)
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

impl<Request, Response, Streams> Provider for Service<Request, Response, Streams>
where
    Request: 'static + Send + Sync,
{
    type Request = Request;
    type Response = Response;
    type Streams = Streams;

    fn connect(self, source: Entity, target: Entity, commands: &mut Commands) {
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
