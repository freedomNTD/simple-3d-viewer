//! Per-user file associations so double-clicking a .glb/.gltf/.obj/.stl/.3mf
//! opens it in the viewer. On Windows this writes HKCU\Software\Classes keys
//! (no admin needed) and nudges the shell to refresh; on other platforms it is
//! a documented no-op since the ecosystem has no equivalent one-click step.

use std::path::Path;

pub enum Action {
    Register,
    Unregister,
}

pub fn run(action: Action, exe: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::run(action, exe)
    }
    #[cfg(not(windows))]
    {
        let _ = (action, exe);
        Err("file association is configured through your desktop environment on this platform (only Windows supports --register)".into())
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::c_void;
    use std::mem::size_of;

    const HKEY_CURRENT_USER: isize = 0x8000_0001;
    const KEY_SET_VALUE: u32 = 0x0002;
    const KEY_WRITE: u32 = 0x20006;
    const REG_SZ: u32 = 1;
    const SHCNE_ASSOCCHANGED: u32 = 0x0800_0000;
    const SHCNF_FLUSH: u32 = 0x1000;

    const PROGID: &str = "Simple3DViewer.Model";
    const APPID: &str = "Applications\\simple-3d-viewer.exe";
    const DESCRIPTION: &str = "3D Model";

    extern "system" {
        fn RegCreateKeyExW(
            key: isize,
            subkey: *const u16,
            reserved: u32,
            class: *const u16,
            options: u32,
            sam: u32,
            sa: *mut c_void,
            result: *mut isize,
            disp: *mut u32,
        ) -> i32;
        fn RegSetValueExW(
            key: isize,
            name: *const u16,
            reserved: u32,
            kind: u32,
            data: *const u8,
            cb: u32,
        ) -> i32;
        fn RegCloseKey(key: isize) -> i32;
        fn RegOpenKeyExW(
            key: isize,
            subkey: *const u16,
            options: u32,
            sam: u32,
            result: *mut isize,
        ) -> i32;
        fn RegDeleteValueW(key: isize, name: *const u16) -> i32;
        fn RegDeleteTreeW(key: isize, subkey: *const u16) -> i32;
        fn SHChangeNotify(eventid: u32, flags: u32, item1: *const c_void, item2: *const c_void);
    }

    fn wide(s: impl AsRef<str>) -> Vec<u16> {
        s.as_ref().encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn set_str(key: isize, name: Option<&str>, value: &str) -> Result<(), String> {
        let name_w = name.map(wide).unwrap_or_else(|| vec![0u16]);
        let val_w: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = unsafe {
            std::slice::from_raw_parts(val_w.as_ptr() as *const u8, val_w.len() * size_of::<u16>())
        };
        let code =
            unsafe { RegSetValueExW(key, name_w.as_ptr(), 0, REG_SZ, bytes.as_ptr(), bytes.len() as u32) };
        if code != 0 {
            return Err(format!(
                "registry write failed (code {code}) for {:?}",
                name.unwrap_or("(default)")
            ));
        }
        Ok(())
    }

    fn create_key(root: isize, subkey: &str) -> Result<isize, String> {
        let sw = wide(subkey);
        let mut h = 0isize;
        let code = unsafe {
            RegCreateKeyExW(
                root,
                sw.as_ptr(),
                0,
                std::ptr::null(),
                0,
                KEY_WRITE,
                std::ptr::null_mut(),
                &mut h,
                std::ptr::null_mut(),
            )
        };
        if code != 0 {
            return Err(format!("cannot open/create HKCU\\{subkey} (code {code})"));
        }
        Ok(h)
    }

    pub fn run(action: Action, exe: &Path) -> Result<(), String> {
        let exe = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
        let exe_str = exe.to_string_lossy().to_string();
        // canonicalize() yields \\?\C:\... which the shell cannot launch
        let exe_str = exe_str
            .strip_prefix(r"\\?\")
            .unwrap_or(&exe_str)
            .to_string();
        let exts = crate::EXTENSIONS;

        match action {
            Action::Register => {
                let cmd = format!("\"{exe_str}\" \"%1\"");
                unsafe {
                    // ProgID with the open command
                    let progid = create_key(HKEY_CURRENT_USER, &format!("Software\\Classes\\{PROGID}"))?;
                    set_str(progid, None, DESCRIPTION)?;
                    let icon = create_key(progid, "DefaultIcon");
                    if let Ok(icon) = icon {
                        set_str(icon, None, &format!("\"{exe_str}\",0"))?;
                        RegCloseKey(icon);
                    }
                    let shell = create_key(progid, &format!("shell\\open\\command"))?;
                    set_str(shell, None, &cmd)?;
                    RegCloseKey(shell);
                    RegCloseKey(progid);

                    // the same command under Applications so it shows in "Open with"
                    let app = create_key(HKEY_CURRENT_USER, &format!("Software\\Classes\\{APPID}"))?;
                    let _ = set_str(app, None, DESCRIPTION);
                    let shell = create_key(app, "shell\\open\\command");
                    if let Ok(shell) = shell {
                        set_str(shell, None, &cmd)?;
                        RegCloseKey(shell);
                    }
                    RegCloseKey(app);

                    // associate each extension via ProgIDs (does not override an
                    // existing UserChoice — pick the viewer once in "Open with"
                    // and it sticks)
                    for ext in exts {
                        let k = create_key(HKEY_CURRENT_USER, &format!("Software\\Classes\\.{ext}\\OpenWithProgids"));
                        if let Ok(k) = k {
                            set_str(k, Some(PROGID), "")?;
                            RegCloseKey(k);
                        }
                    }
                    SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_FLUSH, std::ptr::null(), std::ptr::null());
                }
                println!("registered for: {}", exts.join(", "));
                println!("double-click a model (or right-click → Open with) to open it here");
                Ok(())
            }
            Action::Unregister => {
                unsafe {
                    for ext in exts {
                        let sub = wide(format!("Software\\Classes\\.{ext}\\OpenWithProgids"));
                        let mut k = 0isize;
                        if RegOpenKeyExW(
                            HKEY_CURRENT_USER,
                            sub.as_ptr(),
                            0,
                            KEY_SET_VALUE,
                            &mut k,
                        ) == 0
                        {
                            // remove only our value, never the whole key
                            RegDeleteValueW(k, wide(PROGID).as_ptr());
                            RegCloseKey(k);
                        }
                    }
                    let progid = wide(format!("Software\\Classes\\{PROGID}"));
                    RegDeleteTreeW(HKEY_CURRENT_USER, progid.as_ptr());
                    let app = wide(format!("Software\\Classes\\{APPID}"));
                    RegDeleteTreeW(HKEY_CURRENT_USER, app.as_ptr());
                    SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_FLUSH, std::ptr::null(), std::ptr::null());
                }
                println!("removed the associations this app created (existing \"Open with\" choices may remain)");
                Ok(())
            }
        }
    }

}
