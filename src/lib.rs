use std::collections::VecDeque;
use std::marker::PhantomData;

use bevy::prelude::*;
use bevy::tasks::TaskPool;
use bevy::tasks::futures_lite::Stream as StreamTrait;
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

/// Stores yielded values from a stream
#[derive(Component)]
pub struct Stream<T: Send + Sync> {
    buffer: VecDeque<T>,
    max_buffer_size: Option<usize>,
}

#[allow(clippy::new_without_default)]
impl<T: Send + Sync> Stream<T> {
    /// Create a new Stream
    pub fn new(max_buffer_size: Option<usize>) -> Self {
        let max_size = max_buffer_size.unwrap_or(1);
        Self {
            buffer: VecDeque::with_capacity(max_size),
            max_buffer_size: Some(max_size),
        }
    }

    /// Pop the next value from the buffer
    pub fn pop(&mut self) -> Option<T> {
        self.buffer.pop_front()
    }

    /// Consume all values from the buffer
    pub fn consume(&mut self) -> Vec<T> {
        self.buffer.drain(..).collect()
    }

    /// Peek at the next value without consuming it
    pub fn peek(&self) -> Option<&T> {
        self.buffer.front()
    }

    /// Get the number of buffered values
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Check if the buffer is full
    pub fn is_full(&self) -> bool {
        self.max_buffer_size
            .is_some_and(|max| self.buffer.len() >= max)
    }

    /// Add a value to the buffer
    fn push(&mut self, value: T) {
        if let Some(max_size) = self.max_buffer_size
            && self.buffer.len() >= max_size
        {
            self.buffer.pop_front();
        }
        self.buffer.push_back(value);
    }
}

struct StreamState<T: Send + 'static> {
    stream: Box<dyn StreamTrait<Item = T> + Send + Sync + Unpin>,
}

/// Default value to identify unmarked streams in the builder
#[derive(Component, Default)]
pub struct Unmarked;

/// Builder for spawning streams with optional configuration
pub struct StreamBuilder<T, S, M = Unmarked>
where
    T: Send + Sync + 'static,
    S: StreamTrait<Item = T> + Send + Sync + Unpin + 'static,
{
    stream: S,
    taskpool: &'static TaskPool,
    max_buffer_size: Option<usize>,
    _marker: PhantomData<(T, M)>,
}

impl<T, S> StreamBuilder<T, S>
where
    T: Send + Sync + 'static,
    S: StreamTrait<Item = T> + Send + Sync + Unpin + 'static,
{
    /// Create a new stream spawn builder
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            taskpool: AsyncComputeTaskPool::get(),
            max_buffer_size: None,
            _marker: PhantomData,
        }
    }

    /// Add a marker component to the stream entity
    pub fn with_marker<M: Component + Default>(self) -> StreamBuilder<T, S, M> {
        StreamBuilder {
            stream: self.stream,
            taskpool: self.taskpool,
            max_buffer_size: self.max_buffer_size,
            _marker: PhantomData,
        }
    }
}

impl<T, S, M> StreamBuilder<T, S, M>
where
    T: Send + Sync + 'static,
    S: StreamTrait<Item = T> + Send + Sync + Unpin + 'static,
{
    /// Set a custom taskpool for this stream
    pub fn with_taskpool(mut self, taskpool: &'static TaskPool) -> Self {
        self.taskpool = taskpool;
        self
    }

    /// Set a maximum buffer size for this stream
    pub fn with_buffer(mut self, max_buffer_size: Option<usize>) -> Self {
        self.max_buffer_size = max_buffer_size;
        self
    }
}

pub trait SpawnStreamExt {
    /// Spawn a stream from a builder
    fn spawn_stream<T, S, M>(&mut self, builder: StreamBuilder<T, S, M>) -> Entity
    where
        T: Send + Sync + 'static,
        S: StreamTrait<Item = T> + Send + Sync + Unpin + 'static,
        M: Component + Default;
}

impl SpawnStreamExt for Commands<'_, '_> {
    fn spawn_stream<T, S, M>(&mut self, builder: StreamBuilder<T, S, M>) -> Entity
    where
        T: Send + Sync + 'static,
        S: StreamTrait<Item = T> + Send + Sync + Unpin + 'static,
        M: Component + Default,
    {
        let taskpool = builder.taskpool;
        let max_buffer_size = builder.max_buffer_size;

        let mut task = Some(taskpool.spawn(async move {
            StreamState {
                stream: Box::new(builder.stream),
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

            if let Some(mut stream) = world.get_mut::<Stream<T>>(entity) {
                stream.push(new_value);
            }

            task = Some(taskpool.spawn(async move { state }));
        });

        let stream: Stream<T> = Stream::new(max_buffer_size);
        let entity = self.spawn((StreamHandle { poll_fn }, stream)).id();

        if std::any::TypeId::of::<M>() != std::any::TypeId::of::<Unmarked>() {
            self.entity(entity).insert(M::default());
        }

        entity
    }
}

fn poll_streams(world: &mut World) {
    let entities: Vec<Entity> = world
        .query_filtered::<Entity, With<StreamHandle>>()
        .iter(world)
        .collect();

    for entity in entities {
        // take and reinsert handle since we cant hold &mut poll_fn from entity and &mut world at the same time
        let Some(mut handle) = world.entity_mut(entity).take::<StreamHandle>() else {
            continue;
        };

        (handle.poll_fn)(world, entity);
        world.entity_mut(entity).insert(handle);
    }
}
