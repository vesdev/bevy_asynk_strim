use bevy::{prelude::*, tasks::futures_lite::Stream};
use bevy_asynk_strim::{SpawnStreamExt, StreamPlugin, StreamValue};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(StreamPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, print_counter)
        .run();
}

#[derive(Component, Default)]
struct CounterStream;

impl CounterStream {
    fn new_stream() -> impl Stream<Item = u32> {
        Box::pin(asynk_strim::stream_fn(|mut yielder| async move {
            let mut count = 0;
            loop {
                yielder.yield_item(count).await;
                count += 1;
                async_io::Timer::after(std::time::Duration::from_secs(1)).await;
            }
        }))
    }
}

fn setup(mut commands: Commands) {
    commands.spawn_stream_marked::<u32, _, CounterStream>(CounterStream::new_stream());
}

#[allow(clippy::type_complexity)]
fn print_counter(
    mut counter_stream: Query<
        &mut StreamValue<u32>,
        (With<CounterStream>, Changed<StreamValue<u32>>),
    >,
) {
    if let Some(mut stream_value) = counter_stream.iter_mut().next()
        && let Some(value) = stream_value.consume()
    {
        info!("Counter: {}", value);
    }
}
