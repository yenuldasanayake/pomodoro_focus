//! Cross-platform system actions: shutdown, restart, lock screen.

use std::io;
use std::process::Command;

const STARTUP_REG_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const STARTUP_VALUE_NAME: &str = "Pomodoro Focus";

pub fn startup_enabled() -> bool {
    #[cfg(windows)]
    {
        Command::new("reg")
            .args(["query", STARTUP_REG_KEY, "/v", STARTUP_VALUE_NAME])
            .output()
            .is_ok_and(|output| output.status.success())
    }

    #[cfg(not(windows))]
    {
        false
    }
}

pub fn set_startup_enabled(enabled: bool) -> io::Result<()> {
    #[cfg(windows)]
    {
        if enabled {
            let executable = std::env::current_exe()?.to_string_lossy().replace('"', "\"\"");
            let command = format!("\"{executable}\"");
            run(Command::new("reg").args([
                "add",
                STARTUP_REG_KEY,
                "/v",
                STARTUP_VALUE_NAME,
                "/t",
                "REG_SZ",
                "/d",
                &command,
                "/f",
            ]))
        } else {
            let output = Command::new("reg")
                .args(["delete", STARTUP_REG_KEY, "/v", STARTUP_VALUE_NAME, "/f"])
                .output()?;
            if output.status.success()
                || String::from_utf8_lossy(&output.stderr).contains("unable to find")
            {
                Ok(())
            } else {
                Err(io::Error::other(String::from_utf8_lossy(&output.stderr).trim().to_owned()))
            }
        }
    }

    #[cfg(not(windows))]
    {
        let _ = enabled;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Launch at startup is only supported on Windows",
        ))
    }
}

pub fn notify(title: &str, message: &str) {
    #[cfg(windows)]
    {
        let title = title.replace('\'', "''");
        let message = message.replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $n=New-Object System.Windows.Forms.NotifyIcon; \
             $n.Icon=[System.Drawing.SystemIcons]::Information; \
             $n.Visible=$true; \
             $n.ShowBalloonTip(5000,'{title}','{message}','Info'); \
             Start-Sleep -Milliseconds 5200; \
             $n.Dispose()"
        );
        let _ = Command::new("powershell")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
            .spawn();
    }

    #[cfg(not(windows))]
    {
        let _ = (title, message);
    }
}

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
        Err(io::Error::new(io::ErrorKind::Unsupported, "Restart is not supported on this platform"))
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
        Err(io::Error::new(io::ErrorKind::Unsupported, "Cancel is not supported on this platform"))
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
