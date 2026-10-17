
use pomodoro_focus::app;

fn main() -> eframe::Result {
    eframe::run_native(
        "Pomodoro Focus",
        app::native_options(),
        Box::new(|creation_context| {
            let app = app::create_app(creation_context).map_err(|error| {
                std::io::Error::other(format!("Could not start the app: {error}"))
            })?;
            Ok(Box::new(app))
        }),
    )
}