//! Extra mouse buttons (middle, side, ...) as a dictation trigger.
//!
//! macOS only: a session event tap sees `otherMouse` events, and swallows the configured
//! button so it does not also act as "back" or "forward" in the focused app. Left and right
//! buttons are never used.

use std::sync::{Arc, Mutex};
use tauri_plugin_global_shortcut::ShortcutState;

/// Button numbers follow `CGMouseButton`: 0 left, 1 right, 2 middle, 3 and up are extra.
pub const MIN_BUTTON: u8 = 2;
pub const MAX_BUTTON: u8 = 31;

pub type MouseHotkeyHandler = Arc<dyn Fn(ShortcutState) + Send + Sync>;

pub fn is_valid_button(button: u8) -> bool {
    (MIN_BUTTON..=MAX_BUTTON).contains(&button)
}

#[derive(Clone, Default)]
pub struct MouseHotkeyRuntime {
    monitor: Arc<Mutex<Option<platform::Monitor>>>,
}

impl MouseHotkeyRuntime {
    /// Replaces the running monitor. `button == 0` (or an invalid value) turns it off.
    pub fn install(&self, button: u8, handler: MouseHotkeyHandler) -> Result<(), String> {
        let mut monitor = self.monitor.lock().unwrap_or_else(|e| e.into_inner());
        let _ = monitor.take();
        if !is_valid_button(button) {
            return Ok(());
        }
        let started = platform::Monitor::start(Arc::new(move |pressed_button, pressed| {
            if pressed_button != button {
                return false;
            }
            handler(if pressed {
                ShortcutState::Pressed
            } else {
                ShortcutState::Released
            });
            true
        }))?;
        *monitor = Some(started);
        Ok(())
    }
}

/// Waits for the next extra-button press and returns its number. The press is swallowed.
pub fn capture_next_button(timeout: std::time::Duration) -> Result<u8, String> {
    let (tx, rx) = std::sync::mpsc::channel::<u8>();
    let tx = Mutex::new(tx);
    let _monitor = platform::Monitor::start(Arc::new(move |button, pressed| {
        if !is_valid_button(button) {
            return false;
        }
        if pressed {
            let _ = tx.lock().unwrap_or_else(|e| e.into_inner()).send(button);
        }
        true
    }))?;
    rx.recv_timeout(timeout)
        .map_err(|_| "No mouse button was pressed in time".to_string())
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    /// `(button, pressed) -> swallow`
    pub type Callback = Arc<dyn Fn(u8, bool) -> bool + Send + Sync>;

    type CgEventTapLocation = u32;
    type CgEventTapPlacement = u32;
    type CgEventTapOptions = u32;
    type CgEventMask = u64;
    type CgEventType = u32;
    type CgEventField = u32;

    #[repr(C)]
    struct Opaque(c_void);
    type CgEventRef = *mut Opaque;
    type CfMachPortRef = *mut Opaque;
    type CfRunLoopRef = *mut Opaque;
    type CfRunLoopSourceRef = *mut Opaque;
    type CfStringRef = *const c_void;
    type CfAllocatorRef = *const c_void;

    const SESSION_EVENT_TAP: CgEventTapLocation = 1;
    const HEAD_INSERT: CgEventTapPlacement = 0;
    const TAP_OPTION_DEFAULT: CgEventTapOptions = 0;
    const OTHER_MOUSE_DOWN: CgEventType = 25;
    const OTHER_MOUSE_UP: CgEventType = 26;
    const TAP_DISABLED_BY_TIMEOUT: CgEventType = 0xFFFF_FFFE;
    const TAP_DISABLED_BY_USER_INPUT: CgEventType = 0xFFFF_FFFF;
    const MOUSE_EVENT_BUTTON_NUMBER: CgEventField = 3;
    const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);

    type TapCallback =
        extern "C" fn(*mut c_void, CgEventType, CgEventRef, *mut c_void) -> CgEventRef;

    #[allow(clashing_extern_declarations)]
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: CgEventTapLocation,
            place: CgEventTapPlacement,
            options: CgEventTapOptions,
            events_of_interest: CgEventMask,
            callback: TapCallback,
            user_info: *mut c_void,
        ) -> CfMachPortRef;
        fn CGEventTapEnable(tap: CfMachPortRef, enable: bool);
        fn CGEventGetIntegerValueField(event: CgEventRef, field: CgEventField) -> i64;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFMachPortCreateRunLoopSource(
            allocator: CfAllocatorRef,
            port: CfMachPortRef,
            order: isize,
        ) -> CfRunLoopSourceRef;
        fn CFRunLoopGetCurrent() -> CfRunLoopRef;
        fn CFRunLoopAddSource(rl: CfRunLoopRef, source: CfRunLoopSourceRef, mode: CfStringRef);
        fn CFRunLoopRun();
        fn CFRunLoopStop(rl: CfRunLoopRef);
        fn CFRelease(cf: *const c_void);
        static kCFRunLoopCommonModes: CfStringRef;
    }

    struct Handles {
        tap: Mutex<Option<CfMachPortRef>>,
        runloop: Mutex<Option<CfRunLoopRef>>,
        cancelled: AtomicBool,
    }

    // SAFETY: the raw CF pointers are only used to disable the tap and stop the run loop,
    // both of which CoreFoundation allows from any thread.
    unsafe impl Send for Handles {}
    unsafe impl Sync for Handles {}

    impl Handles {
        fn shutdown(&self) {
            self.cancelled.store(true, Ordering::SeqCst);
            if let Some(tap) = self.tap.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                unsafe { CGEventTapEnable(*tap, false) };
            }
            if let Some(rl) = self
                .runloop
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
            {
                unsafe { CFRunLoopStop(*rl) };
            }
        }
    }

    pub struct Monitor {
        handles: Arc<Handles>,
    }

    impl Drop for Monitor {
        fn drop(&mut self) {
            self.handles.shutdown();
        }
    }

    struct Context {
        callback: Callback,
        handles: Arc<Handles>,
        /// Buttons whose press was swallowed, so their release is swallowed too.
        swallowed: Mutex<u32>,
    }

    impl Monitor {
        pub fn start(callback: Callback) -> Result<Self, String> {
            let handles = Arc::new(Handles {
                tap: Mutex::new(None),
                runloop: Mutex::new(None),
                cancelled: AtomicBool::new(false),
            });
            let thread_handles = Arc::clone(&handles);
            let (status_tx, status_rx) = mpsc::channel();
            thread::Builder::new()
                .name("mouse-hotkey-mac".to_string())
                .spawn(move || run_loop(callback, thread_handles, status_tx))
                .map_err(|e| format!("Failed to spawn mouse hotkey thread: {e}"))?;

            match status_rx.recv_timeout(STARTUP_TIMEOUT) {
                Ok(Ok(())) => Ok(Self { handles }),
                Ok(Err(error)) => {
                    handles.shutdown();
                    Err(error)
                }
                Err(_) => {
                    handles.shutdown();
                    Err("Timed out starting the mouse button monitor".to_string())
                }
            }
        }
    }

    fn run_loop(
        callback: Callback,
        handles: Arc<Handles>,
        status: mpsc::Sender<Result<(), String>>,
    ) {
        let context = Box::into_raw(Box::new(Context {
            callback,
            handles: Arc::clone(&handles),
            swallowed: Mutex::new(0),
        }));
        let mask: CgEventMask = (1u64 << OTHER_MOUSE_DOWN) | (1u64 << OTHER_MOUSE_UP);
        unsafe {
            let tap = CGEventTapCreate(
                SESSION_EVENT_TAP,
                HEAD_INSERT,
                TAP_OPTION_DEFAULT,
                mask,
                tap_callback,
                context as *mut c_void,
            );
            if tap.is_null() {
                drop(Box::from_raw(context));
                let _ = status.send(Err(
                    "Failed to create the mouse button EventTap; Accessibility permission may be denied"
                        .to_string(),
                ));
                return;
            }
            *handles.tap.lock().unwrap_or_else(|e| e.into_inner()) = Some(tap);
            let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
            if source.is_null() {
                CGEventTapEnable(tap, false);
                *handles.tap.lock().unwrap_or_else(|e| e.into_inner()) = None;
                CFRelease(tap as *const c_void);
                drop(Box::from_raw(context));
                let _ = status.send(Err("Failed to create the mouse EventTap source".to_string()));
                return;
            }
            let runloop = CFRunLoopGetCurrent();
            *handles.runloop.lock().unwrap_or_else(|e| e.into_inner()) = Some(runloop);
            CFRunLoopAddSource(runloop, source, kCFRunLoopCommonModes);
            CFRelease(source as *const c_void);
            CGEventTapEnable(tap, true);

            if status.send(Ok(())).is_err() || handles.cancelled.load(Ordering::SeqCst) {
                handles.shutdown();
            }
            if !handles.cancelled.load(Ordering::SeqCst) {
                CFRunLoopRun();
            }

            if let Some(tap) = handles.tap.lock().unwrap_or_else(|e| e.into_inner()).take() {
                CGEventTapEnable(tap, false);
                CFRelease(tap as *const c_void);
            }
            *handles.runloop.lock().unwrap_or_else(|e| e.into_inner()) = None;
            drop(Box::from_raw(context));
        }
    }

    extern "C" fn tap_callback(
        _proxy: *mut c_void,
        event_type: CgEventType,
        event: CgEventRef,
        user_info: *mut c_void,
    ) -> CgEventRef {
        if user_info.is_null() {
            return event;
        }
        let context = unsafe { &*(user_info as *const Context) };
        match event_type {
            TAP_DISABLED_BY_TIMEOUT | TAP_DISABLED_BY_USER_INPUT => {
                if let Some(tap) = context
                    .handles
                    .tap
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .as_ref()
                {
                    unsafe { CGEventTapEnable(*tap, true) };
                }
                event
            }
            OTHER_MOUSE_DOWN | OTHER_MOUSE_UP => {
                let raw = unsafe { CGEventGetIntegerValueField(event, MOUSE_EVENT_BUTTON_NUMBER) };
                let Ok(button) = u8::try_from(raw) else {
                    return event;
                };
                let pressed = event_type == OTHER_MOUSE_DOWN;
                let bit = 1u32.checked_shl(u32::from(button)).unwrap_or(0);
                let mut swallowed = context.swallowed.lock().unwrap_or_else(|e| e.into_inner());
                let swallow = if pressed {
                    let swallow = (context.callback)(button, true);
                    if swallow {
                        *swallowed |= bit;
                    }
                    swallow
                } else if *swallowed & bit != 0 {
                    *swallowed &= !bit;
                    (context.callback)(button, false);
                    true
                } else {
                    false
                };
                if swallow {
                    std::ptr::null_mut()
                } else {
                    event
                }
            }
            _ => event,
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use std::sync::Arc;

    pub type Callback = Arc<dyn Fn(u8, bool) -> bool + Send + Sync>;

    pub struct Monitor;

    impl Monitor {
        pub fn start(_callback: Callback) -> Result<Self, String> {
            Ok(Self)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_extra_buttons_are_valid() {
        assert!(!is_valid_button(0));
        assert!(!is_valid_button(1));
        assert!(is_valid_button(2));
        assert!(is_valid_button(3));
        assert!(!is_valid_button(32));
    }

    #[test]
    fn disabled_button_installs_nothing() {
        let runtime = MouseHotkeyRuntime::default();
        assert!(runtime.install(0, Arc::new(|_| {})).is_ok());
    }
}
