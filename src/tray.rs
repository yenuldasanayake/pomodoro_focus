//! System tray icon.

use std::sync::mpsc::{self, Receiver};

#[derive(Debug, Clone, Copy)]
pub enum TrayEvent {
    ShowWindow,
    HideWindow,
    ToggleTimer,
    Quit,
}

pub struct TrayHandle {
    rx: Receiver<TrayEvent>,
    #[cfg(windows)]
    _icon: tray_icon::TrayIcon,
}

impl TrayHandle {
    pub fn new() -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();

        #[cfg(windows)]
        {
            use tray_icon::{
                TrayIconBuilder,
                menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
            };

            let menu = Menu::new();
            let show = MenuItem::new("Show window", true, None);
            let hide = MenuItem::new("Hide to tray", true, None);
            let toggle = MenuItem::new("Start / pause", true, None);
            let separator = PredefinedMenuItem::separator();
            let quit = MenuItem::new("Quit", true, None);

            menu.append(&show)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            menu.append(&hide)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            menu.append(&toggle)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            menu.append(&separator)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            menu.append(&quit)
                .map_err(|e| std::io::Error::other(e.to_string()))?;

            let show_id = show.id().clone();
            let hide_id = hide.id().clone();
            let toggle_id = toggle.id().clone();
            let quit_id = quit.id().clone();

            let icon = build_icon();

            let tray = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("Pomodoro Focus")
                .with_icon(icon)
                .build()
                .map_err(|e| std::io::Error::other(e.to_string()))?;

            let tx = tx.clone();
            std::thread::Builder::new()
                .name("tray-event-pump".to_owned())
                .spawn(move || {
                    let event_rx = MenuEvent::receiver();
                    loop {
                        let Ok(event) = event_rx.recv() else {
                            break;
                        };
                        let mapped = if event.id == show_id {
                            Some(TrayEvent::ShowWindow)
                        } else if event.id == hide_id {
                            Some(TrayEvent::HideWindow)
                        } else if event.id == toggle_id {
                            Some(TrayEvent::ToggleTimer)
                        } else if event.id == quit_id {
                            Some(TrayEvent::Quit)
                        } else {
                            None
                        };
                        if let Some(ev) = mapped
                            && tx.send(ev).is_err()
                        {
                            break;
                        }
                    }
                })
                .map_err(|e| std::io::Error::other(e.to_string()))?;

            return Ok(Self {
                rx,
                _icon: tray,
            });
        }

        #[cfg(not(windows))]
        {
            let _ = tx;
            Ok(Self { rx })
        }
    }

    pub fn poll(&self) -> Vec<TrayEvent> {
        self.rx.try_iter().collect()
    }
}

#[cfg(windows)]
fn build_icon() -> tray_icon::Icon {
    // Same PNG as the window icon. Include from the project root.
    let bytes = include_bytes!("../icon.png");
    match eframe::icon_data::from_png_bytes(bytes) {
        Ok(data) => tray_icon::Icon::from_rgba(data.rgba, data.width, data.height)
            .unwrap_or_else(|error| {
                eprintln!("Could not build tray icon from PNG: {error}");
                fallback_icon()
            }),
        Err(error) => {
            eprintln!("Could not decode tray icon PNG: {error}");
            fallback_icon()
        }
    }
}

#[cfg(windows)]
fn fallback_icon() -> tray_icon::Icon {
    // Programmatic fallback in case the PNG is missing or invalid.
    const SIZE: u32 = 32;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - 16.0;
            let dy = y as f32 - 16.0;
            let dist = (dx * dx + dy * dy).sqrt();

            let (r, g, b, a) = if dist < 11.0 && dist > 8.0 {
                (0x8a, 0xb4, 0xf8, 255)
            } else if dist < 8.0 {
                (0x20, 0x21, 0x24, 255)
            } else {
                (0x20, 0x21, 0x24, 255)
            };
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    tray_icon::Icon::from_rgba(rgba, SIZE, SIZE)
        .unwrap_or_else(|_| tray_icon::Icon::from_rgba(vec![255; 4], 1, 1).unwrap())
}