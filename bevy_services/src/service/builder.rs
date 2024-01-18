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
    BlockingReq, InBlockingReq, AsyncReq, Provider, Service, GenericAsyncReq,
    Delivery, ServiceBundle,
    stream::*,
    private,
};

use bevy::{
    prelude::{App, In},
    ecs::{
        world::EntityMut,
        system::{IntoSystem, Commands, EntityCommands}
    }
};

use std::future::Future;

use futures::future::BoxFuture;

use super::traits::*;

pub struct BuilderMarker;

pub struct ServiceBuilder<Request, Response, Streams, Deliver, With, Also> {
    service: Service<Request, Response>,
    streams: std::marker::PhantomData<Streams>,
    deliver: Deliver,
    with: With,
    also: Also,
}

impl <Request, Response> ServiceBuilder<Request, Response, (), BlockingChosen, (), ()> {
    /// Start building a new blocking service from a system that uses the
    /// standard blocking service signature.
    pub fn new_blocking<M, Sys>(system: Sys) -> Self
    where
        Sys: IntoSystem<BlockingReq<Request>, Response, M>,
        Request: 'static,
        Response: 'static,
    {
        let service = Service::Blocking(Some(Box::new(IntoSystem::into_system(system))));
        Self {
            service,
            streams: Default::default(),
            deliver: BlockingChosen,
            with: (),
            also: (),
        }
    }

    /// Convert any system with an input and output into a blocking service.
    pub fn into_blocking<M, Sys>(system: Sys) -> Self
    where
        Sys: IntoSystem<Request, Response, M>,
        Request: 'static,
        Response: 'static,
    {
        let peel = |In(BlockingReq { request, .. }): InBlockingReq<Request>| request;
        let service = Service::Blocking(Some(
            Box::new(IntoSystem::into_system(peel.pipe(system)))
        ));

        Self {
            service,
            streams: Default::default(),
            deliver: (),
            with: (),
            also: (),
        }
    }
}

impl<Request, Response> ServiceBuilder<Request, Response, (), (), (), ()> {
    /// Start building a service from a system that takes in a simple request
    /// type and returns a future.
    pub fn into_async<M, Sys, Task>(system: Sys) -> Self
    where
        Sys: IntoSystem<Request, Task, M>,
        Task: Future<Output=Response> + 'static + Send,
        Request: 'static,
        Response: 'static,
    {
        let peel = |In(GenericAsyncReq { request, .. }): In<GenericAsyncReq<Request>>| request;

        let into_task = |In(task): In<Task>| {
            let task: BoxFuture<'static, Response> = Box::pin(task);
            task
        };

        let service = Service::Async(Some(
            Box::new(
                IntoSystem::into_system(
                    peel
                    .pipe(system)
                    .pipe(into_task)
                )
            )
        ));

        Self {
            service,
            streams: Default::default(),
            deliver: (),
            with: (),
            also: (),
        }
    }
}

impl<Request, Response, Streams> ServiceBuilder<Request, Response, Streams, (), (), ()> {
    pub fn new_async<M, Sys, Task>(system: Sys) -> Self
    where
        Sys: IntoSystem<AsyncReq<Request, Streams>, Task, M>,
        Task: Future<Output=Option<Response>> + 'static + Send,
        Request: 'static,
        Response: 'static,
        Streams: 'static,
    {
        let into_specific = |In(input): In<GenericAsyncReq<Request>>| {
            input.into_specific::<Streams>()
        };

        let into_task = |In(task): In<Task>| {
            let task: BoxFuture<'static, Option<Response>> = Box::pin(task);
            task
        };

        let service = Service::Async(Some(
            Box::new(
                IntoSystem::into_system(
                    into_specific
                    .pipe(system)
                    .pipe(into_task)
                )
            )
        ));

        Self {
            service,
            streams: Default::default(),
            deliver: (),
            with: (),
            also: (),
        }
    }
}

impl<Request, Response, Streams, With, Also> ServiceBuilder<Request, Response, Streams, (), With, Also> {
    /// Make this service always fulfill requests in serial. The system that
    /// provides the service will not be executed until any prior run of this
    /// service is finished (delivered or cancelled).
    pub fn serial(self) -> ServiceBuilder<Request, Response, Streams, SerialChosen, With, Also> {
        ServiceBuilder {
            service: self.service,
            streams: Default::default(),
            deliver: SerialChosen,
            with: self.with,
            also: self.also,
        }
    }

    /// Allow the service to run in parallel. Requests that shared the same
    /// RequestLabel will still be run in serial or interrupt each other
    /// depending on settings.
    pub fn parallel(self) -> ServiceBuilder<Request, Response, Streams, ParallelChosen, With, Also> {
        ServiceBuilder {
            service: self.service,
            streams: Default::default(),
            deliver: ParallelChosen,
            with: self.with,
            also: self.also,
        }
    }
}

impl<Request, Response, Streams, Deliver> ServiceBuilder<Request, Response, Streams, Deliver, (), ()> {
    pub fn with<With>(self, with: With) -> ServiceBuilder<Request, Response, Streams, Deliver, With, ()> {
        ServiceBuilder {
            service: self.service,
            streams: Default::default(),
            deliver: self.deliver,
            with,
            also: self.also,
        }
    }
}

impl<Request, Response, Streams, Deliver, With> ServiceBuilder<Request, Response, Streams, Deliver, With, ()> {
    pub fn also<Also>(self, also: Also) -> ServiceBuilder<Request, Response, Streams, Deliver, With, Also> {
        ServiceBuilder {
            service: self.service,
            streams: Default::default(),
            deliver: self.deliver,
            with: self.with,
            also,
        }
    }
}

impl<Request, Response, Streams, Deliver, With, Also>
ServiceAdd<BuilderMarker>
for ServiceBuilder<Request, Response, Streams, Deliver, With, Also>
where
    Streams: IntoStreamBundle,
    Deliver: DeliveryChoice,
    With: WithEntityMut,
    Also: AlsoAdd<Request, Response, Streams>,
    Request: 'static + Send + Sync,
    Response: 'static + Send + Sync,
{
    type Request = Request;
    type Response = Response;
    type Streams = Streams;
    fn add_service(self, app: &mut App) {
        let mut entity_mut = app.world.spawn(ServiceBundle::new(self.service));
        let provider = Provider::<Request, Response, Streams>::new(entity_mut.id());
        entity_mut.insert(Streams::StreamOutBundle::default());
        self.deliver.apply_entity_mut(&mut entity_mut);
        self.with.apply(entity_mut);
        self.also.apply(app, provider);
    }
}

impl<M, S: IntoServiceBuilder<M>> ServiceAdd<M> for S
where
    S::Streams: IntoStreamBundle,
    S::DefaultDeliver: DeliveryChoice,
    S::Request: 'static + Send + Sync,
    S::Response: 'static + Send + Sync,
{
    type Request = S::Request;
    type Response = S::Response;
    type Streams = S::Streams;
    fn add_service(self, app: &mut App) {
        ServiceAdd::<BuilderMarker>::add_service(self.builder(), app);
    }
}

impl<Request, Response, Streams, Deliver, With>
ServiceSpawn<BuilderMarker>
for ServiceBuilder<Request, Response, Streams, Deliver, With, ()>
where
    Streams: IntoStreamBundle,
    Deliver: DeliveryChoice,
    With: WithEntityCommands,
    Request: 'static + Send + Sync,
    Response: 'static + Send + Sync,
{
    type Request = Request;
    type Response = Response;
    type Streams = Streams;
    fn spawn_service(self, commands: &mut Commands) -> Provider<Request, Response, Streams> {
        let mut entity_cmds = commands.spawn(ServiceBundle::new(self.service));
        let provider = Provider::<Request, Response, Streams>::new(entity_cmds.id());
        entity_cmds.insert(Streams::StreamOutBundle::default());
        self.deliver.apply_entity_commands(&mut entity_cmds);
        self.with.apply(&mut entity_cmds);
        provider
    }
}

impl<Request, Response, Streams, Deliver, With, Also>
private::Sealed<BuilderMarker> for ServiceBuilder<Request, Response, Streams, Deliver, With, Also> { }

impl<M, S: IntoServiceBuilder<M>> ServiceSpawn<M> for S
where
    S::Streams: IntoStreamBundle,
    S::DefaultDeliver: DeliveryChoice,
    S::Request: 'static + Send + Sync,
    S::Response: 'static + Send + Sync,
{
    type Request = S::Request;
    type Response = S::Response;
    type Streams = S::Streams;
    fn spawn_service(self, commands: &mut Commands) -> Provider<Self::Request, Self::Response, Self::Streams> {
        ServiceSpawn::<BuilderMarker>::spawn_service(self.builder(), commands)
    }
}

impl<Request, Response, M, Sys>
IntoServiceBuilder<(Request, Response, M)> for Sys
where
    Sys: IntoSystem<BlockingReq<Request>, Response, M>,
    Request: 'static,
    Response: 'static,
{
    type Request = Request;
    type Response = Response;
    type Streams = ();
    type DefaultDeliver = BlockingChosen;
    fn builder(self) -> ServiceBuilder<Self::Request, Self::Response, (), Self::DefaultDeliver, (), ()> {
        ServiceBuilder::simple_blocking(self)
    }
    fn with<With>(self, with: With) -> ServiceBuilder<Self::Request, Self::Response, (), BlockingChosen, With, ()> {
        ServiceBuilder::simple_blocking(self).with(with)
    }
    fn also<Also>(self, also: Also) -> ServiceBuilder<Self::Request, Self::Response, (), BlockingChosen, (), Also> {
        ServiceBuilder::simple_blocking(self).also(also)
    }
}

impl<Request, Response, M, Sys>
private::Sealed<(Request, Response, M)> for Sys { }

impl<Request, Response, Streams, Task, M, Sys>
IntoServiceBuilder<(Request, Response, Streams, Task, M)> for Sys
where
    Sys: IntoSystem<AsyncReq<Request>, Task, M>,
    Task: Future<Output=Response> + 'static + Send,
    Streams: IntoStreamBundle + 'static,
    Request: 'static,
    Response: 'static,
{
    type Request = Request;
    type Response = Response;
    type Streams = Streams;
    type DefaultDeliver = ();
    fn builder(self) -> ServiceBuilder<Self::Request, Self::Response, Self::Streams, Self::DefaultDeliver, (), ()> {
        ServiceBuilder::new_async(self)
    }
    fn with<With>(self, with: With) -> ServiceBuilder<Self::Request, Self::Response, Self::Streams, (), With, ()> {
        self.builder().with(with)
    }
    fn also<Also>(self, also: Also) -> ServiceBuilder<Self::Request, Self::Response, Self::Streams, (), (), Also> {
        self.builder().also(also)
    }
}

impl<Request, Response, Streams, Task, M, Sys>
private::Sealed<(Request, Response, Streams, Task, M)> for Sys { }

impl<Request, Response, Streams, Task, M, Sys>
ChooseAsyncServiceDelivery<(Request, Response, Streams, Task, M)> for Sys
where
    Sys: IntoSystem<AsyncReq<Request>, Task, M>,
    Task: Future<Output=Response> + 'static + Send,
    Streams: IntoStreamBundle + 'static,
    Request: 'static,
    Response: 'static,
{
    type Request = Request;
    type Response = Response;
    type Streams = Streams;
    fn serial(self) -> ServiceBuilder<Self::Request, Self::Response, Self::Streams, SerialChosen, (), ()> {
        ServiceBuilder::new_async(self).serial()
    }
    fn parallel(self) -> ServiceBuilder<Self::Request, Self::Response, Self::Streams, ParallelChosen, (), ()> {
        ServiceBuilder::new_async(self).parallel()
    }
}

impl<Request, Response, M, Sys>
IntoBlockingServiceBuilder<(Request, Response, M)> for Sys
where
    Sys: IntoSystem<Request, Response, M>,
    Request: 'static,
    Response: 'static,
{
    type Request = Request;
    type Response = Response;
    fn into_blocking_service(self) -> ServiceBuilder<Self::Request, Self::Response, (), BlockingChosen, (), ()> {
        ServiceBuilder::into_blocking(self)
    }
}

impl<Request, Response, Task, M, Sys>
IntoAsyncServiceBuilder<(Request, Response, (), Task, M)> for Sys
where
    Sys: IntoSystem<Request, Task, M>,
    Task: Future<Output=Response> + 'static + Send,
    Request: 'static,
    Response: 'static,
{
    type Request = Request;
    type Response = Response;
    fn into_async_service(self) -> ServiceBuilder<Self::Request, Self::Response, (), (), (), ()> {
        ServiceBuilder::into_async(self)
    }
}

/// When this is used in the Deliver type parameter of AsyncServiceBuilder, the
/// user has indicated that the service should be executed in serial.
pub struct SerialChosen;

impl DeliveryChoice for SerialChosen {
    fn apply_entity_mut<'w>(self, entity_mut: &mut EntityMut<'w>) {
        entity_mut.insert(Delivery::serial());
    }

    fn apply_entity_commands<'w, 's, 'a>(self, entity_commands: &mut EntityCommands<'w, 's, 'a>) {
        entity_commands.insert(Delivery::serial());
    }
}

impl private::Sealed<()> for SerialChosen { }

/// When this is used in the Deliver type parameter of AsyncServiceBuilder, the
/// user has indicated that the service should be executed in parallel.
pub struct ParallelChosen;

impl DeliveryChoice for ParallelChosen {
    fn apply_entity_mut<'w>(self, entity_mut: &mut EntityMut<'w>) {
        entity_mut.insert(Delivery::parallel());
    }

    fn apply_entity_commands<'w, 's, 'a>(self, entity_commands: &mut EntityCommands<'w, 's, 'a>) {
        entity_commands.insert(Delivery::parallel());
    }
}

impl private::Sealed<()> for ParallelChosen { }

/// When this is used in the Deliver type parameter of ServiceBuilder, the user
/// has indicated that the service is blocking and therefore does not have a
/// delivery type.
pub struct BlockingChosen;

impl DeliveryChoice for BlockingChosen {
    fn apply_entity_commands<'w, 's, 'a>(self, _: &mut EntityCommands<'w, 's, 'a>) {
        // Do nothing
    }
    fn apply_entity_mut<'w>(self, _: &mut EntityMut<'w>) {
        // Do nothing
    }
}

impl private::Sealed<()> for BlockingChosen { }

impl DeliveryChoice for () {
    fn apply_entity_commands<'w, 's, 'a>(self, entity_commands: &mut EntityCommands<'w, 's, 'a>) {
        ParallelChosen.apply_entity_commands(entity_commands)
    }
    fn apply_entity_mut<'w>(self, entity_mut: &mut EntityMut<'w>) {
        ParallelChosen.apply_entity_mut(entity_mut)
    }
}

impl private::Sealed<()> for () { }

impl<T: FnOnce(EntityMut)> WithEntityMut for T {
    fn apply<'w>(self, entity_mut: EntityMut<'w>) {
        self(entity_mut);
    }
}

impl WithEntityMut for () {
    fn apply<'w>(self, _: EntityMut<'w>) {
        // Do nothing
    }
}

impl<T: FnOnce(&mut EntityCommands)> WithEntityCommands for T {
    fn apply<'w, 's, 'a>(self, entity_commands: &mut EntityCommands<'w, 's, 'a>) {
        self(entity_commands);
    }
}

impl WithEntityCommands for () {
    fn apply<'w, 's, 'a>(self, _: &mut EntityCommands<'w, 's ,'a>) {
        // Do nothing
    }
}

impl<Request, Response, Streams, T> AlsoAdd<Request, Response, Streams> for T
where
    T: FnOnce(&mut App, Provider<Request, Response, Streams>)
{
    fn apply<'w>(self, app: &mut App, provider: Provider<Request, Response, Streams>) {
        self(app, provider)
    }
}

impl<Request, Response, Streams> AlsoAdd<Request, Response, Streams> for () {
    fn apply<'w>(self, _: &mut App, _: Provider<Request, Response, Streams>) {
        // Do nothing
    }
}
