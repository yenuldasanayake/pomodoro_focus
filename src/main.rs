mod alt_tab;
mod app;
mod audio;
mod charts;
mod server;
mod site_blocker;
mod storage;
mod theme;
mod timer;

fn main() -> eframe::Result {
    eframe::run_native(
        "Pomodoro Focus",
        app::native_options(),
        Box::new(|creation_context| {
            let app = app::create_app(creation_context).map_err(|error| {
                std::io::Error::other(format!("Could not generate remote PIN: {error}"))
            })?;
            Ok(Box::new(app))
        }),
    )
}
