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

use bevy_ecs::prelude::{Entity, World};

use tokio::sync::mpsc::UnboundedSender as TokioSender;

use std::sync::Arc;

use crate::{OperationRoster, ChannelItem, Disposal, emit_disposal};

/// This is used as a field inside of [`crate::BufferKey`] which keeps track of
/// when a key that was sent out into the world gets fully dropped from use. We
/// could implement the [`Drop`] trait for [`crate::BufferKey`] itself, but then
/// we would be needlessly doing a reachability check every time the key gets
/// cloned.
#[derive(Clone)]
pub(crate) struct BufferAccessLifecycle {
    scope: Entity,
    accessor: Entity,
    session: Entity,
    buffer: Entity,
    sender: TokioSender<ChannelItem>,
    /// This tracker is an additional layer of indirection that allows the
    /// buffer accessor node that created the key to keep track of whether this
    /// lifecycle is still active.
    pub(crate) tracker: Arc<()>,
}

impl BufferAccessLifecycle {
    pub(crate) fn new(
        scope: Entity,
        buffer: Entity,
        session: Entity,
        accessor: Entity,
        sender: TokioSender<ChannelItem>,
        tracker: Arc<()>,
    ) -> Self {
        Self { scope, accessor, session, buffer, sender, tracker }
    }

    pub(crate) fn is_in_use(&self) -> bool {
        Arc::strong_count(&self.tracker) > 1
    }
}

impl Drop for BufferAccessLifecycle {
    fn drop(&mut self) {
        if self.is_in_use() {
            let scope = self.scope;
            let accessor = self.accessor;
            let session = self.session;
            let buffer = self.buffer;
            if let Err(err) = self.sender.send(
                Box::new(move |world: &mut World, roster: &mut OperationRoster| {
                    let disposal = Disposal::buffer_key(accessor, buffer);
                    emit_disposal(accessor, session, disposal, world, roster);
                })
            ) {
                eprintln!(
                    "Failed to send disposal notice for dropped buffer key in \
                    scope [{:?}] for session [{:?}]: {}",
                    scope,
                    session,
                    err,
                );
            }
        }
    }
}
