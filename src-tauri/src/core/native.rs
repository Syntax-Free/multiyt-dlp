use tauri::{AppHandle, Manager, Window};

#[cfg(target_os = "windows")]
use windows::Win32::{
    Foundation::HWND,
    System::Com::{CoCreateInstance, CoInitialize, CLSCTX_ALL},
    UI::Shell::{ITaskbarList3, TaskbarList, TBPF_ERROR, TBPF_NOPROGRESS, TBPF_NORMAL},
};

#[cfg(target_os = "macos")]
use cocoa::appkit::{NSApp, NSApplication, NSApplicationActivationPolicyRegular};
#[cfg(target_os = "macos")]
use cocoa::base::{id, nil};
#[cfg(target_os = "macos")]
use cocoa::foundation::NSString;

/// Updates the taskbar progress.
/// `progress` should be between 0.0 and 1.0
/// `is_error` determines if the bar should be colored red (Windows only)
pub fn set_taskbar_progress(app: &AppHandle, progress: f64, is_error: bool) {
    let main_window = match app.get_window("main") {
        Some(w) => w,
        None => return,
    };

    #[cfg(target_os = "windows")]
    let _ = set_windows_progress(&main_window, progress, is_error);

    #[cfg(target_os = "macos")]
    let _ = set_mac_badge(progress);
}

/// Removes progress bar/badge
pub fn clear_taskbar_progress(app: &AppHandle) {
    let main_window = match app.get_window("main") {
        Some(w) => w,
        None => return,
    };

    #[cfg(target_os = "windows")]
    let _ = set_windows_progress_state(&main_window, false);

    #[cfg(target_os = "macos")]
    let _ = clear_mac_badge();
}

#[cfg(target_os = "windows")]
fn set_windows_progress(window: &Window, progress: f64, is_error: bool) -> Result<(), String> {
    let hwnd = window.hwnd().map_err(|e| e.to_string())?;

    unsafe {
        // Ensure COM is initialized on this thread (usually needed for TaskbarList)
        let _ = CoInitialize(None);

        let taskbar_list: ITaskbarList3 = CoCreateInstance(&TaskbarList, None, CLSCTX_ALL)
            .map_err(|e| format!("Failed to create ITaskbarList3: {}", e))?;

        let hwnd_raw = HWND(hwnd.0 as isize);

        let flags = if is_error { TBPF_ERROR } else { TBPF_NORMAL };
        taskbar_list.SetProgressState(hwnd_raw, flags).ok();

        // Scale 0.0-1.0 to 0-100
        let value = (progress * 100.0) as u64;
        taskbar_list.SetProgressValue(hwnd_raw, value, 100).ok();
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn set_windows_progress_state(window: &Window, visible: bool) -> Result<(), String> {
    let hwnd = window.hwnd().map_err(|e| e.to_string())?;
    unsafe {
        let _ = CoInitialize(None);
        let taskbar_list: ITaskbarList3 =
            CoCreateInstance(&TaskbarList, None, CLSCTX_ALL).map_err(|e| e.to_string())?;

        let hwnd_raw = HWND(hwnd.0 as isize);
        let flags = if visible {
            TBPF_NORMAL
        } else {
            TBPF_NOPROGRESS
        };
        taskbar_list.SetProgressState(hwnd_raw, flags).ok();
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn set_mac_badge(progress: f64) -> Result<(), String> {
    let percent = (progress * 100.0) as u32;
    let label = format!("{}%", percent);

    unsafe {
        let dock_tile = NSApp().dockTile();
        let label_ns = NSString::alloc(nil).init_str(&label);
        dock_tile.setBadgeLabel_(label_ns);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn clear_mac_badge() -> Result<(), String> {
    unsafe {
        let dock_tile = NSApp().dockTile();
        dock_tile.setBadgeLabel_(nil);
    }
    Ok(())
}
