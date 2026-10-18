use pomodoro_focus::{app, storage::Storage};

fn main() -> eframe::Result {
    let storage = Storage::new(".");
    let settings = storage.load_settings().unwrap_or_default();
    eframe::run_native(
        "Pomodoro Focus",
        app::native_options(&settings),
        Box::new(|creation_context| {
            let app = app::create_app(creation_context).map_err(|error| {
                std::io::Error::other(format!("Could not start the app: {error}"))
            })?;
            Ok(Box::new(app))
        }),
    )
}