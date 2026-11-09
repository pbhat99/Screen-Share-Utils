use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Debug, Clone)]
pub struct KeyDisplayToken {
    pub text: String,
    #[allow(dead_code)]
    pub created_at: Instant,
    pub last_updated: Instant,
}

impl KeyDisplayToken {
    pub fn new(text: String) -> Self {
        let now = Instant::now();
        Self {
            text,
            created_at: now,
            last_updated: now,
        }
    }

    pub fn alpha(&self, stay_ms: u64, fade_ms: u64) -> f32 {
        let elapsed = self.last_updated.elapsed().as_millis() as u64;
        if elapsed < stay_ms {
            1.0
        } else if elapsed < stay_ms + fade_ms {
            1.0 - ((elapsed - stay_ms) as f32 / fade_ms as f32)
        } else {
            0.0
        }
    }

    pub fn is_expired(&self, stay_ms: u64, fade_ms: u64) -> bool {
        self.last_updated.elapsed().as_millis() as u64 >= (stay_ms + fade_ms)
    }
}

pub struct KeystrokeState {
    pub active_tokens: Vec<KeyDisplayToken>,
    pub max_tokens: usize,
}

impl KeystrokeState {
    pub fn new() -> Self {
        Self {
            active_tokens: Vec::new(),
            max_tokens: 4,
        }
    }

    pub fn push_key(&mut self, key_str: String) {
        let now = Instant::now();
        let is_modifier_or_combo = key_str.contains('+') || key_str.len() > 1;

        if is_modifier_or_combo {
            // If the user previously pressed a standalone modifier (e.g. "Shift")
            // and within 1200ms triggers a combination with it (e.g. "Shift + f(F)"),
            // replace the standalone modifier badge so it displays the complete chord!
            if let Some(last) = self.active_tokens.last() {
                let is_standalone_mod = matches!(last.text.as_str(), "Shift" | "Ctrl" | "Alt" | "Win");
                if is_standalone_mod && key_str.contains(&last.text) && last.last_updated.elapsed() < Duration::from_millis(1200) {
                    self.active_tokens.pop();
                }
            }
            self.active_tokens.push(KeyDisplayToken::new(key_str));
        } else {
            if let Some(last) = self.active_tokens.last_mut() {
                if !last.text.contains('+')
                    && last.last_updated.elapsed() < Duration::from_millis(1200)
                    && last.text.len() < 24
                {
                    last.text.push_str(&key_str);
                    last.last_updated = now;
                    return;
                }
            }
            self.active_tokens.push(KeyDisplayToken::new(key_str));
        }

        if self.active_tokens.len() > self.max_tokens {
            self.active_tokens.remove(0);
        }
    }

    pub fn tick_cleanup(&mut self, stay_ms: u64, fade_ms: u64) -> bool {
        self.active_tokens.retain(|t| !t.is_expired(stay_ms, fade_ms));
        !self.active_tokens.is_empty()
    }
}

pub struct KeyHookManager {
    hook_handle: HHOOK,
}

static mut GLOBAL_STATE: Option<Arc<Mutex<KeystrokeState>>> = None;
static mut HOOK_HANDLE: HHOOK = HHOOK(std::ptr::null_mut());

impl KeyHookManager {
    pub fn new(state: Arc<Mutex<KeystrokeState>>) -> Self {
        unsafe {
            GLOBAL_STATE = Some(state);
            let instance: HINSTANCE = match GetModuleHandleW(None) {
                Ok(h) => h.into(),
                Err(_) => HINSTANCE::default(),
            };

            let hook = SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(low_level_keyboard_proc),
                instance,
                0,
            ).unwrap_or(HHOOK(std::ptr::null_mut()));

            HOOK_HANDLE = hook;
            Self { hook_handle: hook }
        }
    }

    pub fn stop(&mut self) {
        unsafe {
            if !self.hook_handle.is_invalid() {
                let _ = UnhookWindowsHookEx(self.hook_handle);
                self.hook_handle = HHOOK(std::ptr::null_mut());
                HOOK_HANDLE = HHOOK(std::ptr::null_mut());
            }
        }
    }
}

impl Drop for KeyHookManager {
    fn drop(&mut self) {
        self.stop();
    }
}

unsafe extern "system" fn low_level_keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && (wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN) {
        let kbd = *(lparam.0 as *const KBDLLHOOKSTRUCT);
        let vk = kbd.vkCode as u16;

        let ctrl_down = (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
        let alt_down = (kbd.flags.0 & 0x20) != 0 || (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0;
        let shift_down = (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
        let win_down = (GetAsyncKeyState(VK_LWIN.0 as i32) as u16 & 0x8000) != 0
            || (GetAsyncKeyState(VK_RWIN.0 as i32) as u16 & 0x8000) != 0;

        let is_modifier_key = matches!(
            VIRTUAL_KEY(vk),
            VK_LCONTROL | VK_RCONTROL | VK_CONTROL |
            VK_LMENU | VK_RMENU | VK_MENU |
            VK_LSHIFT | VK_RSHIFT | VK_SHIFT |
            VK_LWIN | VK_RWIN
        );

        if is_modifier_key {
            let mod_name = match VIRTUAL_KEY(vk) {
                VK_LSHIFT | VK_RSHIFT | VK_SHIFT => "Shift",
                VK_LCONTROL | VK_RCONTROL | VK_CONTROL => "Ctrl",
                VK_LMENU | VK_RMENU | VK_MENU => "Alt",
                VK_LWIN | VK_RWIN => "Win",
                _ => "",
            };
            if !mod_name.is_empty() {
                if let Some(ref state_mutex) = GLOBAL_STATE {
                    if let Ok(mut state) = state_mutex.lock() {
                        state.push_key(mod_name.to_string());
                    }
                }
            }
        } else {
            let key_name = format_key(VIRTUAL_KEY(vk), shift_down);
            let mut chord = String::new();

            if win_down { chord.push_str("Win + "); }
            if ctrl_down { chord.push_str("Ctrl + "); }
            if alt_down { chord.push_str("Alt + "); }
            if shift_down { chord.push_str("Shift + "); }

            chord.push_str(&key_name);

            if let Some(ref state_mutex) = GLOBAL_STATE {
                if let Ok(mut state) = state_mutex.lock() {
                    state.push_key(chord);
                }
            }
        }
    }

    CallNextHookEx(HOOK_HANDLE, code, wparam, lparam)
}

fn format_key(vk: VIRTUAL_KEY, shift: bool) -> String {
    match vk {
        VK_RETURN => "Enter".into(),
        VK_ESCAPE => "Esc".into(),
        VK_BACK => "⌫".into(),
        VK_TAB => "Tab".into(),
        VK_SPACE => "Space".into(),
        VK_DELETE => "Del".into(),
        VK_INSERT => "Ins".into(),
        VK_HOME => "Home".into(),
        VK_END => "End".into(),
        VK_PRIOR => "PgUp".into(),
        VK_NEXT => "PgDn".into(),
        VK_UP => "↑".into(),
        VK_DOWN => "↓".into(),
        VK_LEFT => "←".into(),
        VK_RIGHT => "→".into(),
        VK_CAPITAL => "Caps".into(),
        VK_SNAPSHOT => "PrtSc".into(),
        VK_VOLUME_UP => "Vol+".into(),
        VK_VOLUME_DOWN => "Vol-".into(),
        VK_VOLUME_MUTE => "Mute".into(),

        // Function keys
        VK_F1 => "F1".into(),
        VK_F2 => "F2".into(),
        VK_F3 => "F3".into(),
        VK_F4 => "F4".into(),
        VK_F5 => "F5".into(),
        VK_F6 => "F6".into(),
        VK_F7 => "F7".into(),
        VK_F8 => "F8".into(),
        VK_F9 => "F9".into(),
        VK_F10 => "F10".into(),
        VK_F11 => "F11".into(),
        VK_F12 => "F12".into(),

        // Alphanumeric keys
        VIRTUAL_KEY(0x30..=0x39) => {
            let num = (vk.0 - 0x30) as u8;
            if shift {
                let sym = match num {
                    1 => "!",
                    2 => "@",
                    3 => "#",
                    4 => "$",
                    5 => "%",
                    6 => "^",
                    7 => "&",
                    8 => "*",
                    9 => "(",
                    0 => ")",
                    _ => "",
                };
                if !sym.is_empty() {
                    format!("{}({})", num, sym)
                } else {
                    num.to_string()
                }
            } else {
                num.to_string()
            }
        }
        VIRTUAL_KEY(0x41..=0x5A) => {
            let ch = (b'A' + (vk.0 - 0x41) as u8) as char;
            if shift {
                format!("{}({})", ch.to_lowercase(), ch)
            } else {
                ch.to_lowercase().to_string()
            }
        }
        // Numpad
        VIRTUAL_KEY(0x60..=0x69) => format!("Num {}", vk.0 - 0x60),
        VK_MULTIPLY => "Num *".into(),
        VK_ADD => "Num +".into(),
        VK_SUBTRACT => "Num -".into(),
        VK_DECIMAL => "Num .".into(),
        VK_DIVIDE => "Num /".into(),

        // OEM keys
        VK_OEM_1 => if shift { ";(:)".into() } else { ";".into() },
        VK_OEM_PLUS => if shift { "=(+)".into() } else { "=".into() },
        VK_OEM_COMMA => if shift { ",(<)".into() } else { ",".into() },
        VK_OEM_MINUS => if shift { "-(_)".into() } else { "-".into() },
        VK_OEM_PERIOD => if shift { ".(>)".into() } else { ".".into() },
        VK_OEM_2 => if shift { "/(?)".into() } else { "/".into() },
        VK_OEM_3 => if shift { "`(~)".into() } else { "`".into() },
        VK_OEM_4 => if shift { "[({)".into() } else { "[".into() },
        VK_OEM_5 => if shift { "\\(|)".into() } else { "\\".into() },
        VK_OEM_6 => if shift { "](})".into() } else { "]".into() },
        VK_OEM_7 => if shift { "'(\")".into() } else { "'".into() },

        _ => format!("Key_{}", vk.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_key() {
        assert_eq!(format_key(VK_RETURN, false), "Enter");
        assert_eq!(format_key(VK_SPACE, false), "Space");
        assert_eq!(format_key(VIRTUAL_KEY(0x41), false), "a");
        assert_eq!(format_key(VIRTUAL_KEY(0x41), true), "a(A)");
        assert_eq!(format_key(VIRTUAL_KEY(0x46), true), "f(F)");
        assert_eq!(format_key(VIRTUAL_KEY(0x31), true), "1(!)");
    }

    #[test]
    fn test_hook_creation() {
        unsafe {
            let hinstance: HINSTANCE = match GetModuleHandleW(None) {
                Ok(h) => h.into(),
                Err(_) => HINSTANCE::default(),
            };
            let kbd_hook = SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(low_level_keyboard_proc),
                hinstance,
                0,
            );
            assert!(kbd_hook.is_ok(), "SetWindowsHookExW should succeed: {:?}", kbd_hook.err());
            let hook = kbd_hook.unwrap();
            assert!(!hook.is_invalid(), "Hook handle should not be invalid");
            let _ = UnhookWindowsHookEx(hook);
        }
    }
}
