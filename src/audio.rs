use rodio::{DeviceSinkBuilder, Player, Source, source::SineWave};
use std::{thread, time::Duration};

pub fn play_completion_beep() {
    thread::spawn(|| {
        let stream = match DeviceSinkBuilder::open_default_sink() {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("Could not open audio output: {error}");
                return;
            }
        };
        let sink = Player::connect_new(stream.mixer());
        sink.append(
            SineWave::new(740.0)
                .take_duration(Duration::from_millis(260))
                .amplify(0.18),
        );
        sink.sleep_until_end();
    });
}
