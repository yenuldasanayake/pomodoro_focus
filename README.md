# Kronari

Kronari is a fullscreen Pomodoro timer for Windows, built with Rust, eframe,
and egui. It tracks daily study time, records session notes, and provides a
PIN-protected phone remote on your local network.

## Build and run

Install the Rust toolchain with the MSVC build tools, then run from this
directory:

```powershell
cargo run --release
```

The app opens in a large, resizable window. On first launch, a short setup lets
you choose a focus length and optional daily goal.

## Phone remote

The desktop app displays its local URL, a QR code, and a randomly generated
eight-digit PIN. Scan the QR code on a phone connected to the same Wi-Fi to
pair automatically; the QR code contains a one-time code that expires after
two minutes. You can also open a copied URL and enter the PIN manually. Paired
sessions expire after eight hours, can be revoked individually from the PC,
and are all revoked when the PIN changes or Kronari closes. The phone page can
start, pause, reset, or skip the timer and update all timer lengths.
The remote uses the port shown in the desktop app; allow it through Windows
Firewall for private networks if prompted. The PIN is generated from
the operating system's secure random source, and five incorrect attempts
temporarily lock PIN pairing. Remote traffic is unencrypted HTTP, so only use
the phone remote on a trusted network.

## Flexible focus rhythm

Set a weekly focus-day target from 1 to 7 in Preferences (5 by default). Your
weekly rhythm advances when you meet that target, so unplanned gaps or rest days
do not reset it. The current week stays in progress until it ends.

Use **Wrap up day** on the Today card whenever you are ready to review notes and
choose an optional first step for tomorrow. The note summary is not a prompt that
interrupts the timer. The plan is saved with history and included in exports.

Preferences also offer a compact timer window and an always-on-top option. The
compact window can be expanded from its **Expand** button.

## Data files

- `settings.json` stores timer lengths and the session cycle.
- `data/history.json` stores study seconds and completed sessions by local calendar date.
- `data/notes.json` stores completed-session notes, timestamps, durations, and
  optional subject tags.
- `data/reflections.json` stores your weekly reflections.
- `timer.json` stores timer progress so it can be restored after a restart.

These files are created in the current working directory. Existing history,
notes, reflections, and timer progress are retained across launches.

## Credits

Created by Yenul Dasanayake.
