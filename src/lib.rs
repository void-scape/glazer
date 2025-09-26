#![no_std]
extern crate alloc;

mod debug;
mod platform;

pub use debug::*;

pub fn run<Memory, Pixels>(
    mut memory: Memory,
    frame_buffer: &mut [Pixels],
    width: usize,
    height: usize,
    handle_input: fn(PlatformInput<Memory>),
    update_and_render: fn(PlatformUpdate<Memory, Pixels>),
) where
    Pixels: 'static,
    Memory: 'static,
{
    assert!(
        core::mem::size_of::<Pixels>() == 4,
        "`Pixels` must be 4 bytes"
    );
    let pixels_len = frame_buffer.len();
    let update = move |req: platform::PlatformRequest| {
        match req {
            platform::PlatformRequest::Update(state) => {
                assert!(pixels_len >= state.width * state.height);
                update_and_render(PlatformUpdate {
                    memory: &mut memory,
                    delta: state.delta,
                    //
                    frame_buffer: unsafe {
                        core::slice::from_raw_parts_mut(
                            state.frame_buffer as *mut _,
                            state.width * state.height,
                        )
                    },
                    width: state.width,
                    height: state.height,
                    //
                    samples: state.samples,
                    sample_rate: state.sample_rate,
                    channels: state.channels,
                })
            }
            platform::PlatformRequest::Input(input) => handle_input(PlatformInput {
                memory: &mut memory,
                input,
            }),
        }
    };

    // #[cfg(target_arch = "wasm32")]
    // platform::wasm::run(update, process_audio);
    #[cfg(target_os = "macos")]
    platform::appkit::run(update, frame_buffer.as_mut_ptr() as *mut u8, width, height);
}

#[derive(Debug)]
pub struct PlatformUpdate<'a, T, Pixels> {
    // logic
    pub memory: &'a mut T,
    pub delta: f32,

    // graphics
    pub frame_buffer: &'a mut [Pixels],
    pub width: usize,
    pub height: usize,

    // audio
    pub samples: &'a mut [i16],
    pub sample_rate: f32,
    pub channels: usize,
}

#[derive(Debug)]
pub struct PlatformInput<'a, T> {
    pub memory: &'a mut T,
    pub input: Input,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    Key {
        code: KeyCode,
        modifiers: KeyModifiers,
        pressed: bool,
        repeat: bool,
    },
    MouseMoved {
        dx: f32,
        dy: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,

    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,

    Backslash,
    CloseBracket,
    Comma,
    EqualSign,
    Hyphen,
    NonUSBackslash,
    NonUSPound,
    OpenBracket,
    Period,
    Quote,
    Semicolon,
    Separator,
    Slash,
    Spacebar,

    CapsLock,
    LeftAlt,
    LeftControl,
    LeftShift,
    LockingCapsLock,
    LockingNumLock,
    LockingScrollLock,
    RightAlt,
    RightControl,
    RightShift,
    ScrollLock,

    LeftArrow,
    RightArrow,
    UpArrow,
    DownArrow,
    PageUp,
    PageDown,
    Home,
    End,
    DeleteForward,
    DeleteOrBackspace,
    Escape,
    Insert,
    Return,
    Tab,

    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyModifiers(pub u8);

impl KeyModifiers {
    pub const CLEAR: Self = Self(0);
    pub const CAPSLOCK: Self = Self(1);
    pub const SHIFT: Self = Self(1 << 1);
    pub const CONTROL: Self = Self(1 << 2);
    pub const OPTION: Self = Self(1 << 3);
    pub const COMMAND: Self = Self(1 << 4);
    pub const NUMERIC_PAD: Self = Self(1 << 5);
    pub const HELP: Self = Self(1 << 6);
    pub const FUNCTION: Self = Self(1 << 7);
}

impl core::ops::BitOr for KeyModifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitAnd for KeyModifiers {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}
