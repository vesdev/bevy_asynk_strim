use bevy::prelude::*;
use bevy::tasks::TaskPool;
use bevy::tasks::futures_lite::Stream;
use bevy::tasks::{AsyncComputeTaskPool, block_on, futures_lite::StreamExt, poll_once};

pub use asynk_strim;

pub struct StreamPlugin;

impl Plugin for StreamPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, poll_streams);
    }
}

type PollFn = dyn FnMut(&mut World, Entity) + Send + Sync;

#[derive(Component)]
pub struct StreamHandle {
    poll_fn: Box<PollFn>,
}

#[derive(Component)]
pub struct StreamValue<T: Send + Sync>(pub Option<T>);

impl<T: Send + Sync> StreamValue<T> {
    pub fn consume(&mut self) -> Option<T> {
        self.0.take()
    }
}

struct StreamState<T: Send + 'static> {
    stream: Box<dyn Stream<Item = T> + Send + Sync + Unpin>,
}

pub trait SpawnStreamExt {
    fn spawn_stream_with_taskpool<T, S>(
        &mut self,
        taskpool: &'static TaskPool,
        stream: S,
    ) -> Entity
    where
        T: Send + Sync + 'static,
        S: Stream<Item = T> + Send + Sync + Unpin + 'static;

    fn spawn_stream<T, S>(&mut self, stream: S) -> Entity
    where
        T: Send + Sync + 'static,
        S: Stream<Item = T> + Send + Sync + Unpin + 'static;

    fn spawn_stream_marked<T, S, M>(&mut self, stream: S) -> Entity
    where
        T: Send + Sync + 'static,
        S: Stream<Item = T> + Send + Sync + Unpin + 'static,
        M: Component + Default;

    fn spawn_stream_marked_with_taskpool<T, S, M>(
        &mut self,
        taskpool: &'static TaskPool,
        stream: S,
    ) -> Entity
    where
        T: Send + Sync + 'static,
        S: Stream<Item = T> + Send + Sync + Unpin + 'static,
        M: Component + Default;
}

impl SpawnStreamExt for Commands<'_, '_> {
    fn spawn_stream_with_taskpool<T, S>(&mut self, taskpool: &'static TaskPool, stream: S) -> Entity
    where
        T: Send + Sync + 'static,
        S: Stream<Item = T> + Send + Sync + Unpin + 'static,
    {
        let mut task = Some(taskpool.spawn(async move {
            StreamState {
                stream: Box::new(stream),
            }
        }));

        let poll_fn = Box::new(move |world: &mut World, entity: Entity| {
            let Some(mut task_inner) = task.take() else {
                return;
            };

            let Some(mut state) = block_on(poll_once(&mut task_inner)) else {
                task = Some(task_inner);
                return;
            };

            let Some(Some(new_value)) = block_on(poll_once(state.stream.next())) else {
                task = Some(taskpool.spawn(async move { state }));
                return;
            };

            if let Some(mut value) = world.get_mut::<StreamValue<T>>(entity) {
                value.0 = Some(new_value);
            } else {
                world
                    .entity_mut(entity)
                    .insert(StreamValue(Some(new_value)));
            }

            task = Some(taskpool.spawn(async move { state }));
        });

        self.spawn(StreamHandle { poll_fn }).id()
    }

    fn spawn_stream<T, S>(&mut self, stream: S) -> Entity
    where
        T: Send + Sync + 'static,
        S: Stream<Item = T> + Send + Sync + Unpin + 'static,
    {
        Self::spawn_stream_with_taskpool::<T, S>(self, AsyncComputeTaskPool::get(), stream)
    }

    fn spawn_stream_marked_with_taskpool<T, S, M>(
        &mut self,
        taskpool: &'static TaskPool,
        stream: S,
    ) -> Entity
    where
        T: Send + Sync + 'static,
        S: Stream<Item = T> + Send + Sync + Unpin + 'static,
        M: Component + Default,
    {
        let entity = Self::spawn_stream_with_taskpool::<T, S>(self, taskpool, stream);
        self.entity(entity).insert(M::default());
        entity
    }

    fn spawn_stream_marked<T, S, M>(&mut self, stream: S) -> Entity
    where
        T: Send + Sync + 'static,
        S: Stream<Item = T> + Send + Sync + Unpin + 'static,
        M: Component + Default,
    {
        Self::spawn_stream_marked_with_taskpool::<T, S, M>(
            self,
            AsyncComputeTaskPool::get(),
            stream,
        )
    }
}

fn poll_streams(world: &mut World) {
    let entities: Vec<Entity> = world
        .query_filtered::<Entity, With<StreamHandle>>()
        .iter(world)
        .collect();

    for entity in entities {
        let poll_fn = world
            .entity_mut(entity)
            .take::<StreamHandle>()
            .map(|h| h.poll_fn);

        if let Some(mut poll_fn) = poll_fn {
            poll_fn(world, entity);

            world.entity_mut(entity).insert(StreamHandle { poll_fn });
        }
    }
}
