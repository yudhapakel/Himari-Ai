use std::path::PathBuf;

pub type HWND = isize;
pub type BOOL = i32;
pub type LPARAM = isize;
pub type HANDLE = isize;

#[repr(C)]
pub struct LASTINPUTINFO {
    pub cb_size: u32,
    pub dw_time: u32,
}

#[repr(C)]
pub struct SYSTEMTIME {
    pub w_year: u16,
    pub w_month: u16,
    pub w_day_of_week: u16,
    pub w_day: u16,
    pub w_hour: u16,
    pub w_minute: u16,
    pub w_second: u16,
    pub w_milliseconds: u16,
}

extern "system" {
    pub fn GetLastInputInfo(plii: *mut LASTINPUTINFO) -> i32;
    pub fn keybd_event(b_vk: u8, b_scan: u8, dw_flags: u32, dw_extra_info: usize);
    pub fn GetTickCount() -> u32;
    pub fn GetLocalTime(lpSystemTime: *mut SYSTEMTIME);
    pub fn EnumWindows(lpEnumFunc: unsafe extern "system" fn(HWND, LPARAM) -> BOOL, lParam: LPARAM) -> BOOL;
    pub fn GetWindowTextW(hWnd: HWND, lpString: *mut u16, nMaxCount: i32) -> i32;
    pub fn PostMessageW(hWnd: HWND, Msg: u32, wParam: usize, lParam: isize) -> BOOL;
    pub fn SetWindowPos(hWnd: HWND, hWndInsertAfter: HWND, X: i32, Y: i32, cx: i32, cy: i32, uFlags: u32) -> BOOL;
    pub fn IsWindowVisible(hWnd: HWND) -> BOOL;
    pub fn GetForegroundWindow() -> HWND;
    pub fn GetWindowThreadProcessId(hWnd: HWND, lpdwProcessId: *mut u32) -> u32;
    pub fn OpenProcess(dwDesiredAccess: u32, bInheritHandle: BOOL, dwProcessId: u32) -> HANDLE;
    pub fn QueryFullProcessImageNameW(hProcess: HANDLE, dwFlags: u32, lpExeName: *mut u16, lpdwSize: *mut u32) -> BOOL;
    pub fn CloseHandle(hObject: HANDLE) -> BOOL;
}

pub fn get_active_window_info() -> (String, String) {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd == 0 {
            return (String::new(), String::new());
        }

        let mut buffer = [0u16; 512];
        let len = GetWindowTextW(hwnd, buffer.as_mut_ptr(), 512);
        let title = if len > 0 {
            String::from_utf16_lossy(&buffer[..len as usize])
        } else {
            String::new()
        };

        let mut process_id: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut process_id);
        let mut exe_name = String::new();
        if process_id != 0 {
            let handle = OpenProcess(0x1000, 0, process_id);
            if handle != 0 {
                let mut path_buf = [0u16; 1024];
                let mut size = 1024u32;
                if QueryFullProcessImageNameW(handle, 0, path_buf.as_mut_ptr(), &mut size) != 0 {
                    let full_path = String::from_utf16_lossy(&path_buf[..size as usize]);
                    if let Some(file_name) = std::path::Path::new(&full_path).file_name() {
                        exe_name = file_name.to_string_lossy().to_string();
                    }
                }
                CloseHandle(handle);
            }
        }

        (title, exe_name)
    }
}

pub fn set_himari_window_topmost() {
    unsafe extern "system" fn enum_window_callback(hwnd: HWND, _: LPARAM) -> BOOL {
        let mut buffer = [0u16; 512];
        let len = GetWindowTextW(hwnd, buffer.as_mut_ptr(), 512);
        if len > 0 {
            let title = String::from_utf16_lossy(&buffer[..len as usize]);
            if title.contains("Himari HUD") && IsWindowVisible(hwnd) != 0 {
                SetWindowPos(hwnd, -1 as HWND, 0, 0, 0, 0, 0x0001 | 0x0002);
                return 0;
            }
        }
        1
    }
    unsafe {
        EnumWindows(enum_window_callback, 0);
    }
}

pub fn get_windows_idle_millis() -> u64 {
    unsafe {
        let mut lii = LASTINPUTINFO {
            cb_size: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dw_time: 0,
        };
        if GetLastInputInfo(&mut lii) != 0 {
            let current_tick = GetTickCount();
            if current_tick >= lii.dw_time {
                (current_tick - lii.dw_time) as u64
            } else {
                0
            }
        } else {
            0
        }
    }
}

pub fn get_local_hour_minute() -> (u16, u16) {
    unsafe {
        let mut st = SYSTEMTIME {
            w_year: 0,
            w_month: 0,
            w_day_of_week: 0,
            w_day: 0,
            w_hour: 0,
            w_minute: 0,
            w_second: 0,
            w_milliseconds: 0,
        };
        GetLocalTime(&mut st);
        (st.w_hour, st.w_minute)
    }
}
