use bevy::prelude::*;
use bevy_asynk_strim::{SpawnStreamExt, Stream, StreamBuilder, StreamPlugin};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(StreamPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, print_counter)
        .run();
}

#[derive(Component, Default)]
struct Counter;

fn setup(mut commands: Commands) {
    let stream = Box::pin(asynk_strim::stream_fn(|mut yielder| async move {
        let mut count: u32 = 0;
        loop {
            yielder.yield_item(count).await;
            count += 1;
            async_io::Timer::after(std::time::Duration::from_secs(1)).await;
        }
    }));

    let builder = StreamBuilder::new(stream).with_marker::<Counter>();

    commands.spawn_stream(builder);
}

fn print_counter(streams: Query<&mut Stream<u32>, With<Counter>>) {
    for mut stream in streams {
        if let Some(value) = stream.pop() {
            info!("Counter: {}", value);
        }
    }
}
