# Pomodoro Focus

A fullscreen Pomodoro timer for Windows, built with Rust, eframe, and egui. It
tracks daily study time, records session notes, and provides a PIN-protected
phone remote on your local network.

## Build and run

Install the Rust toolchain with the MSVC build tools, then run from this
directory:

```powershell
cargo run --release
```

The app starts fullscreen.

## Phone remote

The desktop app displays its local URL and a randomly generated eight-digit
PIN. Open the URL on a phone connected to the same Wi-Fi network. The phone
page can start, pause, reset, or skip the timer and update all timer lengths.
PIN-protected requests are served on port `5000`; allow this port through
Windows Firewall for private networks if prompted. The PIN is generated from
the operating system's secure random source, and five incorrect attempts
temporarily lock PIN-protected actions.

## Data files

- `settings.json` stores timer lengths and the session cycle.
- `data/history.json` stores study seconds by local calendar date.
- `data/notes.json` stores completed-session notes, timestamps, durations, and
  optional subject tags.

These files are created in the current working directory. Existing history and
notes are retained across launches.
