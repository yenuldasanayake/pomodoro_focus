fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("icon.ico");
        if let Err(error) = res.compile() {
            eprintln!("Could not embed exe icon: {error}");
        }
    }
}