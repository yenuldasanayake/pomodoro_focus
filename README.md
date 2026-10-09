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

The app starts fullscreen. Pressing **Esc** toggles fullscreen only when
“Allow Esc to toggle fullscreen” is enabled; it is disabled by default. You
can change this option in the desktop settings or from the phone page, then
save the phone settings.

## Phone remote

The desktop app displays its local URL and a randomly generated eight-digit
PIN. Open the URL on a phone connected to the same Wi-Fi network. The phone
page can start, pause, reset, or skip the timer and update all timer lengths.
PIN-protected requests are served on port `5000`; allow this port through
Windows Firewall for private networks if prompted. The PIN is generated from
the operating system's secure random source, and five incorrect attempts
temporarily lock PIN-protected actions.

The phone page's **Allow Esc to toggle fullscreen** checkbox is included in
the timer settings and takes effect after **Save settings**.

The phone and desktop have a **Block sites** switch for Instagram, YouTube,
TikTok, and Facebook. The switch writes a clearly marked section in the Windows
hosts file, flushes the DNS cache, and removes only that managed section when
you turn blocking off. **Run Pomodoro Focus as Administrator** to use this
system-wide block. Without elevation, the switch still enables the browser
extension, and the app clearly reports that the system-wide hosts block could
not be applied.

### Browser extension (Chrome and Edge)

The optional Manifest V3 extension applies the same switch to browser
navigation, including subdomains. To install it for local use:

1. Run Pomodoro Focus and open `chrome://extensions` (or `edge://extensions`).
2. Turn on **Developer mode** and select **Load unpacked**.
3. Choose the repository's `extension` folder.

The extension follows the desktop app's switch and syncs about every 30
seconds while the browser is running. Keep the desktop app running for the
extension to receive switch updates. Browser blocking remains useful when
system-wide hosts-file editing is unavailable.

## Data files

- `settings.json` stores timer lengths and the session cycle.
- `data/history.json` stores study seconds by local calendar date.
- `data/notes.json` stores completed-session notes, timestamps, durations, and
  optional subject tags.
- `data/blocking.json` stores the site-blocking switch state.

These files are created in the current working directory. Existing history and
notes are retained across launches.
