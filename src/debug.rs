#[macro_export]
macro_rules! log {
    () => {
        $crate::log("\n")
    };
    ($($arg:tt)*) => {{
        $crate::log(&alloc::format!($($arg)*));
        $crate::log("\n")
    }};
}

#[doc = "hidden"]
pub fn log(str: &str) {
    #[cfg(target_os = "macos")]
    crate::platform::appkit::log(str);
}

pub fn debug_time_secs<R>(mut f: impl FnMut() -> R) -> (f32, R) {
    #[cfg(target_os = "macos")]
    {
        extern crate std;
        let start = std::time::Instant::now();
        let result = f();
        let duration = std::time::Instant::now()
            .duration_since(start)
            .as_secs_f32();
        (duration, result)
    }
}

pub fn debug_time_millis<R>(mut f: impl FnMut() -> R) -> (u128, R) {
    #[cfg(target_os = "macos")]
    {
        extern crate std;
        let start = std::time::Instant::now();
        let result = f();
        let duration = std::time::Instant::now().duration_since(start).as_millis();
        (duration, result)
    }
}

pub fn debug_time_nanos<R>(mut f: impl FnMut() -> R) -> (u128, R) {
    #[cfg(target_os = "macos")]
    {
        extern crate std;
        let start = std::time::Instant::now();
        let result = f();
        let duration = std::time::Instant::now().duration_since(start).as_nanos();
        (duration, result)
    }
}
