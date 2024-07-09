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

use bevy::prelude::{Entity, World};

use smallvec::SmallVec;

use crate::{
    Buffer, CloneFromBuffer, OperationError, OrBroken, InspectInput, ManageInput,
};

pub trait Buffered: Clone {
    fn buffered_count(
        &self,
        session: Entity,
        world: &World,
    ) -> Result<usize, OperationError>;

    type Item;
    fn pull(
        &self,
        session: Entity,
        world: &mut World,
    ) -> Result<Self::Item, OperationError>;

    fn as_input(&self) -> SmallVec<[Entity; 8]>;
}

impl<T: 'static + Send + Sync> Buffered for Buffer<T> {
    fn buffered_count(&self, session: Entity, world: &World) -> Result<usize, OperationError> {
        world.get_entity(self.source).or_broken()?
            .buffered_count::<T>(session)
    }

    type Item = T;
    fn pull(
        &self,
        session: Entity,
        world: &mut World,
    ) -> Result<Self::Item, OperationError> {
        world.get_entity_mut(self.source).or_broken()?
            .pull_from_buffer::<T>(session)
    }

    fn as_input(&self) -> SmallVec<[Entity; 8]> {
        SmallVec::from_iter([self.source])
    }
}

impl<T: 'static + Send + Sync + Clone> Buffered for CloneFromBuffer<T> {
    fn buffered_count(
        &self,
        session: Entity,
        world: &World,
    ) -> Result<usize, OperationError> {
        world.get_entity(self.source).or_broken()?
            .buffered_count::<T>(session)
    }

    type Item = T;
    fn pull(
        &self,
        session: Entity,
        world: &mut World,
    ) -> Result<Self::Item, OperationError> {
        world.get_entity(self.source).or_broken()?
            .clone_from_buffer(session)
    }

    fn as_input(&self) -> SmallVec<[Entity; 8]> {
        SmallVec::from_iter([self.source])
    }
}

impl<T0, T1> Buffered for (T0, T1)
where
    T0: Buffered,
    T1: Buffered,
{
    fn buffered_count(
        &self,
        session: Entity,
        world: &World,
    ) -> Result<usize, OperationError> {
        Ok([
            self.0.buffered_count(session, world)?,
            self.1.buffered_count(session, world)?,
        ].iter().copied().min().unwrap_or(0))
    }

    type Item = (T0::Item, T1::Item);
    fn pull(
        &self,
        session: Entity,
        world: &mut World,
    ) -> Result<Self::Item, OperationError> {
        let t0 = self.0.pull(session, world)?;
        let t1 = self.1.pull(session, world)?;
        Ok((t0, t1))
    }

    fn as_input(&self) -> SmallVec<[Entity; 8]> {
        let mut inputs = SmallVec::new();
        inputs.extend(self.0.as_input());
        inputs.extend(self.1.as_input());
        inputs
    }
}

impl<T0, T1, T2> Buffered for (T0, T1, T2)
where
    T0: Buffered,
    T1: Buffered,
    T2: Buffered,
{
    fn buffered_count(
        &self,
        session: Entity,
        world: &World,
    ) -> Result<usize, OperationError> {
        Ok([
            self.0.buffered_count(session, world)?,
            self.1.buffered_count(session, world)?,
            self.2.buffered_count(session, world)?,
        ].iter().copied().min().unwrap_or(0))
    }

    type Item = (T0::Item, T1::Item, T2::Item);
    fn pull(
        &self,
        session: Entity,
        world: &mut World,
    ) -> Result<Self::Item, OperationError> {
        let t0 = self.0.pull(session, world)?;
        let t1 = self.1.pull(session, world)?;
        let t2 = self.2.pull(session, world)?;
        Ok((t0, t1, t2))
    }

    fn as_input(&self) -> SmallVec<[Entity; 8]> {
        let mut inputs = SmallVec::new();
        inputs.extend(self.0.as_input());
        inputs.extend(self.1.as_input());
        inputs.extend(self.2.as_input());
        inputs
    }
}

impl<T: Buffered, const N: usize> Buffered for [T; N] {
    fn buffered_count(
        &self,
        session: Entity,
        world: &World,
    ) -> Result<usize, OperationError> {
        let mut min_count = None;
        for buffer in self.iter() {
            let count = buffer.buffered_count(session, world)?;
            if !min_count.is_some_and(|min| min < count) {
                min_count = Some(count);
            }
        }

        Ok(min_count.unwrap_or(0))
    }

    // TODO(@mxgrey) We may be able to use [T::Item; N] here instead of SmallVec
    // when try_map is stabilized: https://github.com/rust-lang/rust/issues/79711
    type Item = SmallVec<[T::Item; N]>;
    fn pull(
        &self,
        session: Entity,
        world: &mut World,
    ) -> Result<Self::Item, OperationError> {
        self.iter().map(|buffer| {
            buffer.pull(session, world)
        }).collect()
    }

    fn as_input(&self) -> SmallVec<[Entity; 8]> {
        self.iter().flat_map(|buffer| buffer.as_input()).collect()
    }
}

impl<T: Buffered, const N: usize> Buffered for SmallVec<[T; N]> {
    fn buffered_count(
        &self,
        session: Entity,
        world: &World,
    ) -> Result<usize, OperationError> {
        let mut min_count = None;
        for buffer in self.iter() {
            let count = buffer.buffered_count(session, world)?;
            if !min_count.is_some_and(|min| min < count) {
                min_count = Some(count);
            }
        }

        Ok(min_count.unwrap_or(0))
    }

    type Item = SmallVec<[T::Item; N]>;
    fn pull(
        &self,
        session: Entity,
        world: &mut World,
    ) -> Result<Self::Item, OperationError> {
        self.iter().map(|buffer| {
            buffer.pull(session, world)
        }).collect()
    }

    fn as_input(&self) -> SmallVec<[Entity; 8]> {
        self.iter().flat_map(|buffer| buffer.as_input()).collect()
    }
}
