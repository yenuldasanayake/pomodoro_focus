//! Cross-platform system actions: shutdown, restart, lock screen.

use std::io;
use std::process::Command;

pub fn shutdown(delay_seconds: u32) -> io::Result<()> {
    #[cfg(windows)]
    {
        run(Command::new("shutdown").args(["/s", "/t", &delay_seconds.to_string()]))
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let minutes = delay_seconds.div_ceil(60).max(1);
        run(Command::new("shutdown").args(["-h", &format!("+{minutes}")]))
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = delay_seconds;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Shutdown is not supported on this platform",
        ))
    }
}

pub fn restart(delay_seconds: u32) -> io::Result<()> {
    #[cfg(windows)]
    {
        run(Command::new("shutdown").args(["/r", "/t", &delay_seconds.to_string()]))
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let minutes = delay_seconds.div_ceil(60).max(1);
        run(Command::new("shutdown").args(["-r", &format!("+{minutes}")]))
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = delay_seconds;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Restart is not supported on this platform",
        ))
    }
}

pub fn cancel_pending() -> io::Result<()> {
    #[cfg(windows)]
    {
        run(Command::new("shutdown").arg("/a"))
    }

    #[cfg(target_os = "linux")]
    {
        run(Command::new("shutdown").arg("-c"))
    }

    #[cfg(target_os = "macos")]
    {
        run(Command::new("killall").arg("shutdown"))
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Cancel is not supported on this platform",
        ))
    }
}

pub fn lock_screen() -> io::Result<()> {
    #[cfg(windows)]
    {
        run(Command::new("rundll32.exe").args(["user32.dll,LockWorkStation"]))
    }

    #[cfg(target_os = "linux")]
    {
        for (bin, args) in [
            ("loginctl", vec!["lock-session"]),
            ("xdg-screensaver", vec!["lock"]),
            ("gnome-screensaver-command", vec!["-l"]),
            ("dm-tool", vec!["lock"]),
        ] {
            if run(Command::new(bin).args(&args)).is_ok() {
                return Ok(());
            }
        }
        Err(io::Error::other("No lock command succeeded"))
    }

    #[cfg(target_os = "macos")]
    {
        run(Command::new(
            "/System/Library/CoreServices/Menu Extras/User.menu/Contents/Resources/CGSession",
        )
        .arg("-suspend"))
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Lock screen is not supported on this platform",
        ))
    }
}

fn run(command: &mut Command) -> io::Result<()> {
    let program = format!("{:?}", command.get_program());
    let output = command.output()?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(io::Error::other(if stderr.is_empty() {
            format!("Command {program} exited with status {}", output.status)
        } else {
            stderr
        }))
    }
}