use std::{ffi::OsString, os::{raw::c_void, windows::ffi::OsStringExt}};

use windows::Win32::{Foundation::{BOOL, FALSE, HANDLE, HWND, LPARAM, RECT, TRUE}, Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoA, HDC, HMONITOR, MONITORINFO}, System::{ProcessStatus::GetModuleFileNameExW, Threading::{GetProcessId, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ}}, UI::WindowsAndMessaging::{EnumWindows, GetWindowRect, GetWindowTextA, GetWindowTextLengthA, GetWindowThreadProcessId, IsWindow, IsWindowVisible}};

use crate::{prelude::{CapturableContentError, CapturableContentFilter}, util::{Point, Rect, Size}};

use super::AutoHandle;

#[derive(Debug, Clone)]
pub struct WindowsCapturableWindow(pub(crate) HWND);

fn hwnd_process(hwnd: HWND) -> HANDLE {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut _));
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid).unwrap()
    }
}

impl WindowsCapturableWindow {
    pub fn from_impl(hwnd: HWND) -> Self {
        Self(hwnd)
    }

    pub fn title(&self) -> String {
        unsafe {
            let text_length = GetWindowTextLengthA(self.0);
            if text_length == 0 {
                return "".into();
            }
            let mut text_buffer = vec![0u8; text_length as usize + 1];
            let text_length = GetWindowTextA(self.0, &mut text_buffer[..]);
            if (text_length as usize) < text_buffer.len() {
                text_buffer.truncate(text_length as usize);
            }
            String::from_utf8_lossy(&text_buffer).to_string()
        }
    }

    pub fn rect(&self) -> Rect {
        unsafe {
            let mut rect = RECT::default();
            let _ = GetWindowRect(self.0, &mut rect);
            Rect {
                origin: Point {
                    x: rect.left as f64,
                    y: rect.top as f64
                },
                size: Size {
                    width: (rect.right - rect.left) as f64,
                    height: (rect.bottom - rect.top) as f64,
                }
            }
        }
    }

    pub fn application(&self) -> WindowsCapturableApplication {
        WindowsCapturableApplication(hwnd_process(self.0))
    }
}

#[derive(Clone, Debug)]
pub struct WindowsCapturableDisplay(pub(crate) HMONITOR);

impl WindowsCapturableDisplay {
    pub fn from_impl(hmonitor: HMONITOR) -> Self {
        Self(hmonitor)
    }

    pub fn rect(&self) -> Rect {
        unsafe {
            let mut monitor_info = MONITORINFO::default();
            GetMonitorInfoA(self.0, &mut monitor_info as *mut _);
            Rect {
                origin: Point {
                    x: monitor_info.rcMonitor.left as f64,
                    y: monitor_info.rcMonitor.top as f64,
                },
                size: Size {
                    width: (monitor_info.rcMonitor.right - monitor_info.rcMonitor.left) as f64,
                    height: (monitor_info.rcMonitor.bottom - monitor_info.rcMonitor.top) as f64,
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct WindowsCapturableApplication(pub(crate) HANDLE);

impl WindowsCapturableApplication {
    pub fn from_impl(handle: HANDLE) -> Self {
        Self(handle)
    }

    pub fn identifier(&self) -> String {
        unsafe {
            let pid = GetProcessId(self.0);
            let process = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid);
            if process.is_err() {
                return "".into();
            }
            let process = process.unwrap();
            // TODO: If OpenProcess fails we could fall back to GetProcessHandleFromHwnd, in oleacc.dll
            //       Alternatively, it might be better to use the accessibility APIs.
            let process = AutoHandle(process);
            let mut process_name = vec![0u16; 64];
            let mut len = GetModuleFileNameExW (process.0, None, process_name.as_mut_slice()) as usize;
            while len == process_name.len() - 1 {
                process_name = vec![0u16; process_name.len() * 2];
                len = GetModuleFileNameExW (process.0, None, process_name.as_mut_slice()) as usize;
            }

            if len == 0 {
                return "".into();
            }

            let os_string = OsString::from_wide(&process_name[..len as usize]);
            let path = std::path::Path::new(&os_string);
            let file_name = path.file_name();

            if let Some(file_name) = file_name {
                if let Some(name_str) = file_name.to_str() {
                    return name_str.to_string()
                }
            }

            let result = String::from_utf16(&process_name[..len as usize]);
            result.unwrap_or("".into())
        }
    }
}

pub struct WindowsCapturableContent {
    pub(crate) windows: Vec<HWND>,
    pub(crate) displays: Vec<HMONITOR>,
    pub(crate) applications: Vec<HANDLE>,
}

unsafe extern "system" fn enum_windows_callback(window: HWND, windows_ptr_raw: LPARAM) -> BOOL {
    let windows: &mut Vec<HWND> = &mut *(windows_ptr_raw.0 as *mut c_void as *mut _);
    windows.push(window);
    TRUE
}

unsafe extern "system" fn enum_monitors_callback(monitor: HMONITOR, _: HDC, rect: *mut RECT, monitors_ptr_raw: LPARAM) -> BOOL {
    let monitors: &mut Vec<HMONITOR> = &mut *(monitors_ptr_raw.0 as *mut c_void as *mut _);
    monitors.push(monitor);
    TRUE
}

impl WindowsCapturableContent {
    pub async fn new(filter: CapturableContentFilter) -> Result<Self, CapturableContentError> {
        let mut displays = Vec::<HMONITOR>::new();
        let mut windows = Vec::<HWND>::new();
        unsafe {
            if filter.displays {
                EnumDisplayMonitors(HDC(0), None, Some(enum_monitors_callback), LPARAM(&mut displays as *mut _ as *mut c_void as isize));
            }
            if let Some(window_filter) = filter.windows {
                let _ = EnumWindows(Some(enum_windows_callback), LPARAM(&mut windows as *mut _ as *mut c_void as isize));
                windows = windows.iter().filter(|hwnd| {
                    if !IsWindow(**hwnd).as_bool() {
                        return false;
                    }
                    if window_filter.onscreen_only && !IsWindowVisible(**hwnd).as_bool() {
                        return false;
                    }
                    // TODO: filter desktop windows
                    true
                }).map(|hwnd| *hwnd).collect();
            }
        }
        let applications = windows.iter().map(|hwnd| {
            hwnd_process(*hwnd)
        }).collect();
        Ok(WindowsCapturableContent {
            windows,
            displays,
            applications,
        })
    }
}