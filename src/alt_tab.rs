use std::io;

#[cfg(windows)]
mod windows {
    use std::{
        io,
        sync::{
            OnceLock,
            atomic::{AtomicBool, Ordering},
            mpsc::sync_channel,
        },
        thread,
    };
    use windows_sys::Win32::{
        Foundation::{LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{VK_LMENU, VK_LWIN, VK_MENU, VK_RMENU, VK_RWIN, VK_TAB},
            WindowsAndMessaging::{
                CallNextHookEx, DispatchMessageW, GA_ROOT, GetAncestor, GetClassNameW, GetMessageW,
                HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_ALTDOWN, MSG, MSLLHOOKSTRUCT, SetWindowsHookExW,
                TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN,
                WM_KEYUP, WM_LBUTTONDOWN, WM_NCLBUTTONDOWN, WM_SYSKEYDOWN, WM_SYSKEYUP,
                WindowFromPoint,
            },
        },
    };

    static BLOCKED: AtomicBool = AtomicBool::new(false);
    static ALT_DOWN: AtomicBool = AtomicBool::new(false);
    static WINDOWS_KEY_DOWN: AtomicBool = AtomicBool::new(false);
    static HOOK_STARTED: OnceLock<Result<(), String>> = OnceLock::new();

    pub fn set_blocked(blocked: bool) -> io::Result<()> {
        if blocked {
            HOOK_STARTED
                .get_or_init(start_hook)
                .as_ref()
                .map_err(|error| io::Error::other(error.clone()))?;
        }
        BLOCKED.store(blocked, Ordering::SeqCst);
        Ok(())
    }

    fn start_hook() -> Result<(), String> {
        let (ready_sender, ready_receiver) = sync_channel(0);
        thread::Builder::new()
            .name("alt-tab-keyboard-hook".to_owned())
            .spawn(move || unsafe {
                let module = GetModuleHandleW(std::ptr::null());
                let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), module, 0);
                if hook.is_null() {
                    let _ = ready_sender.send(Err(io::Error::last_os_error().to_string()));
                    return;
                }
                let mouse_hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0);
                if mouse_hook.is_null() {
                    let error = io::Error::last_os_error();
                    UnhookWindowsHookEx(hook);
                    let _ = ready_sender.send(Err(error.to_string()));
                    return;
                }
                if ready_sender.send(Ok(())).is_err() {
                    UnhookWindowsHookEx(hook);
                    UnhookWindowsHookEx(mouse_hook);
                    return;
                }

                let mut message = MSG::default();
                loop {
                    let result = GetMessageW(&mut message, std::ptr::null_mut(), 0, 0);
                    if result <= 0 {
                        if result < 0 {
                            eprintln!(
                                "Alt+Tab keyboard hook message loop failed: {}",
                                io::Error::last_os_error()
                            );
                        }
                        break;
                    }
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                UnhookWindowsHookEx(hook);
                UnhookWindowsHookEx(mouse_hook);
            })
            .map_err(|error| format!("Could not start keyboard hook thread: {error}"))?;
        ready_receiver
            .recv()
            .map_err(|error| format!("Could not initialize keyboard hook: {error}"))?
    }

    unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            let key = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };

            let is_down = wparam == WM_KEYDOWN as WPARAM || wparam == WM_SYSKEYDOWN as WPARAM;
            let is_up = wparam == WM_KEYUP as WPARAM || wparam == WM_SYSKEYUP as WPARAM;
            let is_alt = key.vkCode == VK_MENU as u32
                || key.vkCode == VK_LMENU as u32
                || key.vkCode == VK_RMENU as u32;
            let is_windows_key = key.vkCode == VK_LWIN as u32 || key.vkCode == VK_RWIN as u32;

            if is_alt {
                if is_down {
                    ALT_DOWN.store(true, Ordering::SeqCst);
                } else if is_up {
                    ALT_DOWN.store(false, Ordering::SeqCst);
                }
            } else if is_windows_key {
                if is_down {
                    WINDOWS_KEY_DOWN.store(true, Ordering::SeqCst);
                } else if is_up {
                    WINDOWS_KEY_DOWN.store(false, Ordering::SeqCst);
                }
            }

            if BLOCKED.load(Ordering::SeqCst)
                && is_down
                && key.vkCode == VK_TAB as u32
                && (key.flags & LLKHF_ALTDOWN != 0
                    || ALT_DOWN.load(Ordering::SeqCst)
                    || WINDOWS_KEY_DOWN.load(Ordering::SeqCst))
            {
                return 1;
            }
        }
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32
            && BLOCKED.load(Ordering::SeqCst)
            && (wparam == WM_LBUTTONDOWN as WPARAM || wparam == WM_NCLBUTTONDOWN as WPARAM)
        {
            let click = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
            let window = unsafe { WindowFromPoint(click.pt) };
            if !window.is_null() {
                let root = unsafe { GetAncestor(window, GA_ROOT) };
                if is_task_switcher(root) {
                    return 1;
                }
            }
        }
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    fn is_task_switcher(window: windows_sys::Win32::Foundation::HWND) -> bool {
        let mut class_name = [0_u16; 64];
        let length =
            unsafe { GetClassNameW(window, class_name.as_mut_ptr(), class_name.len() as i32) };
        if length <= 0 {
            return false;
        }
        matches!(
            String::from_utf16_lossy(&class_name[..length as usize]).as_str(),
            "Shell_TrayWnd"
                | "Shell_SecondaryTrayWnd"
                | "MultitaskingViewFrame"
                | "TaskSwitcherWnd"
        )
    }
}

pub fn set_blocked(blocked: bool) -> io::Result<()> {
    #[cfg(windows)]
    {
        windows::set_blocked(blocked)
    }

    #[cfg(not(windows))]
    {
        let _ = blocked;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Alt+Tab blocking is only supported on Windows",
        ))
    }
}
