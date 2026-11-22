//! Safe RAII wrappers around the Win32 registry and service APIs.

#![cfg(windows)]
#![allow(dead_code)]

use crate::winapi as w;
use std::ffi::c_void;
use std::ptr;

/// Owns an HKEY and closes it on drop.
pub struct RegKey(pub w::HKEY);

impl RegKey {
    pub fn open(root: w::HKEY, subkey: &str) -> Option<Self> {
        Self::open_ex(root, subkey, w::KEY_READ)
    }

    /// Open with write access (for disabling entries).
    pub fn open_with_write(root: w::HKEY, subkey: &str) -> Option<Self> {
        Self::open_ex(root, subkey, w::KEY_READ | w::KEY_SET_VALUE)
    }

    fn open_ex(root: w::HKEY, subkey: &str, access: w::DWORD) -> Option<Self> {
        let path = w::to_wide(subkey);
        let mut handle: w::HKEY = ptr::null_mut();
        let rc = unsafe {
            w::RegOpenKeyExW(
                root,
                path.as_ptr(),
                0,
                access,
                &mut handle,
            )
        };
        if rc == w::ERROR_SUCCESS as i32 && !handle.is_null() {
            Some(Self(handle))
        } else {
            None
        }
    }

    /// Read a REG_SZ / REG_EXPAND_SZ value as a String.
    pub fn read_string(&self, name: &str) -> Option<String> {
        let name_w = w::to_wide(name);
        let mut kind: w::DWORD = 0;
        let mut len: w::DWORD = 64 * 1024;
        let mut buf = vec![0u8; len as usize];
        let rc = unsafe {
            w::RegQueryValueExW(
                self.0,
                name_w.as_ptr(),
                ptr::null_mut(),
                &mut kind,
                buf.as_mut_ptr(),
                &mut len,
            )
        };
        if rc != w::ERROR_SUCCESS as i32 || len == 0 {
            return None;
        }
        if kind != w::REG_SZ && kind != w::REG_EXPAND_SZ && kind != w::REG_MULTI_SZ {
            return None;
        }
        buf.truncate(len as usize);
        let units: Vec<u16> = buf
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Some(w::from_wide(&units))
    }

    /// Enumerate all value names under this key.
    pub fn value_names(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut index: w::DWORD = 0;
        loop {
            let mut name_buf = [0u16; 256];
            let mut name_len = name_buf.len() as w::DWORD;
            let rc = unsafe {
                w::RegEnumValueW(
                    self.0,
                    index,
                    name_buf.as_mut_ptr(),
                    &mut name_len,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            };
            if rc != w::ERROR_SUCCESS as i32 {
                break;
            }
            out.push(w::from_wide(&name_buf[..name_len as usize]));
            index += 1;
        }
        out
    }

    pub fn set_string(&self, name: &str, value: &str) -> bool {
        let name_w = w::to_wide(name);
        let data = w::to_wide(value);
        let bytes: Vec<u8> = data.iter().flat_map(|u| u.to_le_bytes()).collect();
        let rc = unsafe {
            w::RegSetValueExW(
                self.0,
                name_w.as_ptr(),
                0,
                w::REG_SZ,
                bytes.as_ptr(),
                bytes.len() as w::DWORD,
            )
        };
        rc == w::ERROR_SUCCESS as i32
    }

    pub fn delete_value(&self, name: &str) -> bool {
        let name_w = w::to_wide(name);
        unsafe { w::RegDeleteValueW(self.0, name_w.as_ptr()) == w::ERROR_SUCCESS as i32 }
    }

    /// Read a REG_DWORD value.
    pub fn read_dword(&self, name: &str) -> Option<u32> {
        let name_w = w::to_wide(name);
        let mut kind: w::DWORD = 0;
        let mut value: u32 = 0;
        let mut len = std::mem::size_of::<u32>() as w::DWORD;
        let rc = unsafe {
            w::RegQueryValueExW(
                self.0,
                name_w.as_ptr(),
                ptr::null_mut(),
                &mut kind,
                &mut value as *mut u32 as *mut u8,
                &mut len,
            )
        };
        if rc == w::ERROR_SUCCESS as i32 && kind == w::REG_DWORD {
            Some(value)
        } else {
            None
        }
    }

    /// Write a REG_DWORD value.
    pub fn write_dword(&self, name: &str, value: u32) -> bool {
        let name_w = w::to_wide(name);
        let rc = unsafe {
            w::RegSetValueExW(
                self.0,
                name_w.as_ptr(),
                0,
                w::REG_DWORD,
                &value as *const u32 as *const u8,
                std::mem::size_of::<u32>() as w::DWORD,
            )
        };
        rc == w::ERROR_SUCCESS as i32
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { w::RegCloseKey(self.0) };
        }
    }
}

/// Owns an SC_HANDLE and closes it on drop.
pub struct ScHandle(pub w::SC_HANDLE);

impl ScHandle {
    pub fn open_scm() -> Option<Self> {
        let h = unsafe {
            w::OpenSCManagerW(ptr::null(), ptr::null(), w::SC_MANAGER_CONNECT | w::SC_MANAGER_ENUMERATE_SERVICE)
        };
        if h.is_null() { None } else { Some(Self(h)) }
    }

    pub fn open_service(&self, name: &str, access: w::DWORD) -> Option<Self> {
        let n = w::to_wide(name);
        let h = unsafe { w::OpenServiceW(self.0, n.as_ptr(), access) };
        if h.is_null() { None } else { Some(Self(h)) }
    }

    pub fn query_config(&self) -> Option<w::QUERY_SERVICE_CONFIGW> {
        let mut needed: w::DWORD = 0;
        unsafe {
            w::QueryServiceConfigW(self.0, ptr::null_mut(), 0, &mut needed);
        }
        if needed == 0 {
            return None;
        }
        let mut buf = vec![0u8; needed as usize];
        let ok = unsafe {
            w::QueryServiceConfigW(
                self.0,
                buf.as_mut_ptr() as *mut u8,
                needed,
                &mut needed,
            )
        };
        if ok == 0 {
            return None;
        }
        Some(unsafe { ptr::read(buf.as_ptr() as *const w::QUERY_SERVICE_CONFIGW) })
    }

    pub fn set_start_type(&self, start: w::DWORD) -> bool {
        unsafe {
            w::ChangeServiceConfigW(
                self.0,
                w::SERVICE_NO_CHANGE,
                start,
                w::SERVICE_NO_CHANGE,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
            ) != 0
        }
    }

    /// Query whether the service is configured for delayed auto-start.
    pub fn is_delayed_auto_start(&self) -> bool {
        let mut needed: w::DWORD = 0;
        unsafe {
            w::QueryServiceConfig2W(
                self.0,
                w::SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
                ptr::null_mut(),
                0,
                &mut needed,
            );
        }
        if needed == 0 {
            return false;
        }
        let mut buf = vec![0u8; needed as usize];
        let ok = unsafe {
            w::QueryServiceConfig2W(
                self.0,
                w::SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
                buf.as_mut_ptr(),
                needed,
                &mut needed,
            )
        };
        if ok == 0 {
            return false;
        }
        // Struct is a single BOOL (DWORD).
        unsafe { ptr::read_unaligned(buf.as_ptr() as *const w::DWORD) != 0 }
    }

    /// Set or clear delayed auto-start (requires SERVICE_CHANGE_CONFIG access).
    pub fn set_delayed_auto_start(&self, enable: bool) -> bool {
        let value: w::DWORD = if enable { 1 } else { 0 };
        unsafe {
            w::ChangeServiceConfig2W(
                self.0,
                w::SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
                &value as *const w::DWORD as *mut c_void,
            ) != 0
        }
    }
}

impl Drop for ScHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { w::CloseServiceHandle(self.0) };
        }
    }
}
