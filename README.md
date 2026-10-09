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
can change this option from the phone page, then save the phone settings.

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

The phone page can also block **Alt+Tab**, **Win+Tab**, and left-clicks on the
taskbar to prevent switching apps while Pomodoro Focus is running. This is a
focus aid, not a kiosk lock: other Windows system interfaces may still allow
switching. The block is released when the desktop app exits and will be
enabled again on the next launch if the setting is still on.

The phone page has a **Block sites** switch for Instagram, YouTube, TikTok, and
Facebook. The switch writes a clearly marked section in the Windows hosts file,
flushes the DNS cache, and removes only that managed section when you turn
blocking off. **Run Pomodoro Focus as Administrator** to use this system-wide
block. Without elevation, the switch still enables the browser extension, and
the app clearly reports that the system-wide hosts block could not be applied.

### Browser extension (Chrome, Edge, and Firefox)

The optional extensions apply the same switch to browser navigation, including
subdomains. To install the Chrome or Edge extension for local use:

1. Run Pomodoro Focus and open `chrome://extensions` (or `edge://extensions`).
2. Turn on **Developer mode** and select **Load unpacked**.
3. Choose the repository's `extension` folder.

For Firefox, run Pomodoro Focus and open `about:debugging#/runtime/this-firefox`.
Select **Load Temporary Add-on…** and choose `extension/firefox/manifest.json`.
Firefox temporary add-ons are removed when Firefox closes.

The extensions follow the phone page's switch and sync periodically while the
browser is running (about 30 seconds in Chrome and Edge, and once a minute in
Firefox). Keep the desktop app running for the extension to receive switch
updates. Browser blocking remains useful when system-wide hosts-file editing
is unavailable.

## Data files

- `settings.json` stores timer lengths, the session cycle, and keyboard options.
- `data/history.json` stores study seconds by local calendar date.
- `data/notes.json` stores completed-session notes, timestamps, durations, and
  optional subject tags.
- `data/blocking.json` stores the site-blocking switch state.

These files are created in the current working directory. Existing history and
notes are retained across launches.
