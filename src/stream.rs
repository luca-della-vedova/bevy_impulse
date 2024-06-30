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

use bevy::prelude::{Component, Bundle, Entity, Commands, World, BuildChildren};

use crossbeam::channel::{Receiver, unbounded};

use std::sync::Arc;

use crate::{
    InputSlot, Output, UnusedTarget, RedirectWorkflowStream, RedirectScopeStream,
    AddOperation, OperationRoster, OperationResult, OrBroken, ManageInput,
    InnerChannel, TakenStream, StreamChannel, OperationError,
};

pub trait Stream: 'static + Send + Sync + Sized {
    fn send(
        self,
        StreamRequest { session, target, world, roster, .. }: StreamRequest
    ) -> OperationResult {
        world.get_entity_mut(target).or_broken()?.give_input(session, self, roster)
    }

    fn spawn_scope_stream(
        scope: Entity,
        commands: &mut Commands
    ) -> (
        StreamTargetStorage<Self>,
        InputSlot<Self>,
    ) {
        let source = commands.spawn(UnusedTarget).id();
        commands.add(AddOperation::new(source, RedirectScopeStream::<Self>::new()));
        (
            StreamTargetStorage::new(source),
            InputSlot::new(scope, source),
        )
    }

    fn spawn_workflow_stream(
        scope: Entity,
        commands: &mut Commands,
    ) -> (
        StreamTargetStorage<Self>,
        InputSlot<Self>,
    ) {
        let source = commands.spawn(UnusedTarget).id();
        commands.add(AddOperation::new(source, RedirectWorkflowStream::<Self>::new()));
        (
            StreamTargetStorage::new(source),
            InputSlot::new(scope, source),
        )
    }

    fn spawn_node_stream(
        scope: Entity,
        commands: &mut Commands,
    ) -> (
        StreamTargetStorage<Self>,
        Output<Self>,
    ) {
        let target = commands.spawn(UnusedTarget).id();
        (
            StreamTargetStorage::new(target),
            Output::new(scope, target),
        )
    }

    fn spawn_request_stream(
        session: Entity,
        commands: &mut Commands,
    ) -> (
        StreamTargetStorage<Self>,
        Receiver<Self>,
    ) {
        let (sender, receiver) = unbounded::<Self>();
        let target = commands
            .spawn(())
            // Set the parent of this stream to be the session so it can be
            // recursively despawned together.
            .set_parent(session)
            .id();

        commands.add(AddOperation::new(target, TakenStream::new(sender)));

        (
            StreamTargetStorage::new(target),
            receiver,
        )
    }
}

pub struct StreamRequest<'a> {
    /// The node that emitted the stream
    pub source: Entity,
    /// The session of the stream
    pub session: Entity,
    /// The target of the stream
    pub target: Entity,
    /// The world that the stream exists inside
    pub world: &'a mut World,
    /// The roster of the stream
    pub roster: &'a mut OperationRoster,
}

/// [`StreamAvailable`] is a marker component that indicates what streams are offered by
/// a service.
#[derive(Component)]
pub struct StreamAvailable<T: Stream> {
    _ignore: std::marker::PhantomData<T>,
}

impl<T: Stream> Default for StreamAvailable<T> {
    fn default() -> Self {
        Self { _ignore: Default::default() }
    }
}

/// [`StreamTargetStorage`] keeps track of the target for each stream for a source.
#[derive(Component)]
pub struct StreamTargetStorage<T: Stream> {
    target: Entity,
    _ignore: std::marker::PhantomData<T>,
}

impl<T: Stream> StreamTargetStorage<T> {
    fn new(target: Entity) -> Self {
        Self { target, _ignore: Default::default() }
    }

    pub fn get(&self) -> Entity {
        self.target
    }
}

pub trait StreamPack: 'static + Send + Sync {
    type StreamAvailableBundle: Bundle + Default;
    type StreamStorageBundle: Bundle;
    type StreamInputPack;
    type StreamOutputPack;
    type Receiver;
    type Channel;

    fn spawn_scope_streams(scope: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::StreamInputPack,
    );

    fn spawn_workflow_streams(scope: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::StreamInputPack,
    );

    fn spawn_node_streams(scope: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::StreamOutputPack,
    );

    fn make_receiver(session: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::Receiver,
    );

    fn make_channel(inner: &Arc<InnerChannel>, world: &World) -> Result<Self::Channel, OperationError>;
}

impl<T: Stream> StreamPack for T {
    type StreamAvailableBundle = StreamAvailable<Self>;
    type StreamStorageBundle = StreamTargetStorage<Self>;
    type StreamInputPack = InputSlot<Self>;
    type StreamOutputPack = Output<Self>;
    type Receiver = Receiver<Self>;
    type Channel = StreamChannel<Self>;

    fn spawn_scope_streams(scope: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::StreamInputPack,
    ) {
        T::spawn_scope_stream(scope, commands)
    }

    fn spawn_workflow_streams(scope: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::StreamInputPack,
    ) {
        T::spawn_workflow_stream(scope, commands)
    }

    fn spawn_node_streams(scope: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::StreamOutputPack,
    ) {
        T::spawn_node_stream(scope, commands)
    }

    fn make_receiver(session: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::Receiver,
    ) {
        Self::spawn_request_stream(session, commands)
    }

    fn make_channel(
        inner: &Arc<InnerChannel>,
        world: &World,
    ) -> Result<Self::Channel, OperationError> {
        let target = world.get::<StreamTargetStorage<Self>>(inner.source()).or_broken()?.target;
        Ok(StreamChannel::new(target, Arc::clone(inner)))
    }
}

impl StreamPack for () {
    type StreamAvailableBundle = ();
    type StreamStorageBundle = ();
    type StreamInputPack = ();
    type StreamOutputPack = ();
    type Receiver = ();
    type Channel = ();

    fn spawn_scope_streams(_: Entity, _: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::StreamInputPack,
    ) {
        ((), ())
    }

    fn spawn_workflow_streams(_: Entity, _: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::StreamInputPack,
    ) {
        ((), ())
    }

    fn spawn_node_streams(_: Entity, _: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::StreamOutputPack,
    ) {
        ((), ())
    }

    fn make_receiver(_: Entity, _: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::Receiver,
    ) {
        ((), ())
    }

    fn make_channel(
        _: &Arc<InnerChannel>,
        _: &World,
    ) -> Result<Self::Channel, OperationError> {
        Ok(())
    }
}

impl<T1: StreamPack> StreamPack for (T1,) {
    type StreamAvailableBundle = T1::StreamAvailableBundle;
    type StreamStorageBundle = T1::StreamStorageBundle;
    type StreamInputPack = T1::StreamInputPack;
    type StreamOutputPack = T1::StreamOutputPack;
    type Receiver = T1::Receiver;
    type Channel = T1::Channel;

    fn spawn_scope_streams(scope: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::StreamInputPack,
    ) {
        T1::spawn_scope_streams(scope, commands)
    }

    fn spawn_workflow_streams(scope: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::StreamInputPack,
    ) {
        T1::spawn_workflow_streams(scope, commands)
    }

    fn spawn_node_streams(scope: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::StreamOutputPack,
    ) {
        T1::spawn_node_streams(scope, commands)
    }

    fn make_receiver(session: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::Receiver,
    ) {
        T1::make_receiver(session, commands)
    }

    fn make_channel(
        inner: &Arc<InnerChannel>,
        world: &World,
    ) -> Result<Self::Channel, OperationError> {
        T1::make_channel(inner, world)
    }
}

impl<T1: StreamPack, T2: StreamPack> StreamPack for (T1, T2) {
    type StreamAvailableBundle = (T1::StreamAvailableBundle, T2::StreamAvailableBundle);
    type StreamStorageBundle = (T1::StreamStorageBundle, T2::StreamStorageBundle);
    type StreamInputPack = (T1::StreamInputPack, T2::StreamInputPack);
    type StreamOutputPack = (T1::StreamOutputPack, T2::StreamOutputPack);
    type Receiver = (T1::Receiver, T2::Receiver);
    type Channel = (T1::Channel, T2::Channel);

    fn spawn_scope_streams(scope: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::StreamInputPack,
    ) {
        let t1 = T1::spawn_scope_streams(scope, commands);
        let t2 = T2::spawn_scope_streams(scope, commands);
        ((t1.0, t2.0), (t1.1, t2.1))
    }

    fn spawn_workflow_streams(scope: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::StreamInputPack,
    ) {
        let t1 = T1::spawn_workflow_streams(scope, commands);
        let t2 = T2::spawn_workflow_streams(scope, commands);
        ((t1.0, t2.0), (t1.1, t2.1))
    }

    fn spawn_node_streams(scope: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::StreamOutputPack,
    ) {
        let t1 = T1::spawn_node_streams(scope, commands);
        let t2 = T2::spawn_node_streams(scope, commands);
        ((t1.0, t2.0), (t1.1, t2.1))
    }

    fn make_receiver(session: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::Receiver,
    ) {
        let t1 = T1::make_receiver(session, commands);
        let t2 = T2::make_receiver(session, commands);
        ((t1.0, t2.0), (t1.1, t2.1))
    }

    fn make_channel(
        inner: &Arc<InnerChannel>,
        world: &World,
    ) -> Result<Self::Channel, OperationError> {
        let t1 = T1::make_channel(inner, world)?;
        let t2 = T2::make_channel(inner, world)?;
        Ok((t1, t2))
    }
}

impl<T1: StreamPack, T2: StreamPack, T3: StreamPack> StreamPack for (T1, T2, T3) {
    type StreamAvailableBundle = (T1::StreamAvailableBundle, T2::StreamAvailableBundle, T3::StreamAvailableBundle);
    type StreamStorageBundle = (T1::StreamStorageBundle, T2::StreamStorageBundle, T3::StreamStorageBundle);
    type StreamInputPack = (T1::StreamInputPack, T2::StreamInputPack, T3::StreamInputPack);
    type StreamOutputPack = (T1::StreamOutputPack, T2::StreamOutputPack, T3::StreamOutputPack);
    type Receiver = (T1::Receiver, T2::Receiver, T3::Receiver);
    type Channel = (T1::Channel, T2::Channel, T3::Channel);

    fn spawn_scope_streams(scope: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::StreamInputPack,
    ) {
        let t1 = T1::spawn_scope_streams(scope, commands);
        let t2 = T2::spawn_scope_streams(scope, commands);
        let t3 = T3::spawn_scope_streams(scope, commands);
        ((t1.0, t2.0, t3.0), (t1.1, t2.1, t3.1))
    }

    fn spawn_workflow_streams(scope: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::StreamInputPack,
    ) {
        let t1 = T1::spawn_workflow_streams(scope, commands);
        let t2 = T2::spawn_workflow_streams(scope, commands);
        let t3 = T3::spawn_workflow_streams(scope, commands);
        ((t1.0, t2.0, t3.0), (t1.1, t2.1, t3.1))
    }

    fn spawn_node_streams(scope: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::StreamOutputPack,
    ) {
        let t1 = T1::spawn_node_streams(scope, commands);
        let t2 = T2::spawn_node_streams(scope, commands);
        let t3 = T3::spawn_node_streams(scope, commands);
        ((t1.0, t2.0, t3.0), (t1.1, t2.1, t3.1))
    }

    fn make_receiver(session: Entity, commands: &mut Commands) -> (
        Self::StreamStorageBundle,
        Self::Receiver,
    ) {
        let t1 = T1::make_receiver(session, commands);
        let t2 = T2::make_receiver(session, commands);
        let t3 = T3::make_receiver(session, commands);
        ((t1.0, t2.0, t3.0), (t1.1, t2.1, t3.1))
    }

    fn make_channel(
        inner: &Arc<InnerChannel>,
        world: &World,
    ) -> Result<Self::Channel, OperationError> {
        let t1 = T1::make_channel(inner, world)?;
        let t2 = T2::make_channel(inner, world)?;
        let t3 = T3::make_channel(inner, world)?;
        Ok((t1, t2, t3))
    }
}
