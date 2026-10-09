use std::io;
#[cfg(windows)]
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

const BEGIN_MARKER: &str = "# BEGIN POMODORO_FOCUS SITE BLOCK";
const END_MARKER: &str = "# END POMODORO_FOCUS SITE BLOCK";
const DOMAINS: &[&str] = &[
    "facebook.com",
    "fb.com",
    "fbcdn.net",
    "instagram.com",
    "cdninstagram.com",
    "threads.net",
    "tiktok.com",
    "tiktokcdn.com",
    "tiktokv.com",
    "youtube.com",
    "youtu.be",
    "googlevideo.com",
    "ytimg.com",
];

pub fn set_blocked(blocked: bool) -> io::Result<()> {
    #[cfg(windows)]
    {
        let path = hosts_path();
        let original = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if !blocked && error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let updated = update_hosts(&original, blocked)?;
        if updated == original {
            return Ok(());
        }
        replace_contents(&path, &updated)?;
        flush_dns_cache();
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = blocked;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows hosts-file blocking is only supported on Windows",
        ))
    }
}

#[cfg(windows)]
fn flush_dns_cache() {
    match std::process::Command::new("ipconfig")
        .arg("/flushdns")
        .output()
    {
        Ok(output) if output.status.success() => {}
        Ok(output) => eprintln!(
            "The hosts file was updated, but Windows DNS cache flush failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
        Err(error) => eprintln!("The hosts file was updated, but DNS cache flush failed: {error}"),
    }
}

#[cfg(windows)]
fn hosts_path() -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    PathBuf::from(root)
        .join("System32")
        .join("drivers")
        .join("etc")
        .join("hosts")
}

fn update_hosts(original: &str, blocked: bool) -> io::Result<String> {
    let mut output = String::new();
    let mut in_managed_section = false;
    let mut found_section = false;
    for line in original.lines() {
        if line.trim() == BEGIN_MARKER {
            if in_managed_section {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "The managed hosts-file section has nested start markers",
                ));
            }
            in_managed_section = true;
            found_section = true;
            continue;
        }
        if line.trim() == END_MARKER {
            if !in_managed_section {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "The managed hosts-file section has an unmatched end marker",
                ));
            }
            in_managed_section = false;
            continue;
        }
        if !in_managed_section {
            output.push_str(line);
            output.push_str("\r\n");
        }
    }

    if in_managed_section {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The managed hosts-file section is missing its end marker",
        ));
    }
    if !blocked && !found_section {
        return Ok(original.to_owned());
    }

    if blocked {
        if !output.is_empty() && !output.ends_with("\n\n") {
            output.push_str("\r\n");
        }
        output.push_str(BEGIN_MARKER);
        output.push_str("\r\n");
        for domain in DOMAINS {
            output.push_str("0.0.0.0 ");
            output.push_str(domain);
            output.push_str("\r\n0.0.0.0 www.");
            output.push_str(domain);
            output.push_str("\r\n");
        }
        output.push_str(END_MARKER);
        output.push_str("\r\n");
    }
    Ok(output)
}

#[cfg(windows)]
fn replace_contents(path: &Path, contents: &str) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = path.with_extension(format!("pomodoro-{}-{nonce}.tmp", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        let wide_source: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let wide_target: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // Replace the file atomically while preserving the existing hosts-file ACL.
        if unsafe {
            ReplaceFileW(
                wide_target.as_ptr(),
                wide_source.as_ptr(),
                std::ptr::null(),
                REPLACEFILE_WRITE_THROUGH,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
#[cfg(windows)]
const REPLACEFILE_WRITE_THROUGH: u32 = 0x1;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn ReplaceFileW(
        replaced_file_name: *const u16,
        replacement_file_name: *const u16,
        backup_file_name: *const u16,
        flags: u32,
        exclude: *mut std::ffi::c_void,
        reserved: *mut std::ffi::c_void,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabling_replaces_managed_block_and_preserves_other_hosts() {
        let original = format!(
            "127.0.0.1 localhost\r\n{BEGIN_MARKER}\r\n0.0.0.0 old.example\r\n{END_MARKER}\r\n192.0.2.1 custom.example\r\n"
        );
        let updated = update_hosts(&original, true).unwrap();

        assert!(updated.contains("127.0.0.1"));
        assert!(updated.contains("192.0.2.1 custom.example"));
        assert!(updated.contains("0.0.0.0 instagram.com"));
        assert!(!updated.contains("old.example"));
        assert_eq!(updated.matches(BEGIN_MARKER).count(), 1);
    }

    #[test]
    fn disabling_removes_only_the_managed_block() {
        let original = format!(
            "127.0.0.1 localhost\r\n{BEGIN_MARKER}\r\n0.0.0.0 instagram.com\r\n{END_MARKER}\r\n192.0.2.1 custom.example\r\n"
        );
        let updated = update_hosts(&original, false).unwrap();

        assert!(updated.contains("127.0.0.1"));
        assert!(updated.contains("192.0.2.1 custom.example"));
        assert!(!updated.contains(BEGIN_MARKER));
        assert!(!updated.contains("instagram.com"));
    }

    #[test]
    fn malformed_managed_section_is_not_destructively_rewritten() {
        let missing_end = format!("127.0.0.1 localhost\r\n{BEGIN_MARKER}\r\ncustom entry\r\n");

        assert_eq!(
            update_hosts(&missing_end, false).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
