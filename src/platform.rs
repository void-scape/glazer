use crate::Input;

pub enum PlatformRequest<'a> {
    Update(PlatformState<'a>),
    Input(Input),
}

pub struct PlatformState<'a> {
    pub delta: f32,
    //
    pub frame_buffer: *mut u8,
    pub width: usize,
    pub height: usize,
    //
    pub samples: &'a mut [i16],
    pub channels: usize,
    pub sample_rate: f32,
}

#[cfg(target_os = "macos")]
#[cfg(feature = "std")]
pub mod appkit {
    extern crate std;

    use std::boxed::Box;
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::ptr::{NonNull, null_mut};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;
    use std::{dbg, format};

    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate,
        NSApplicationTerminateReply, NSBackingStoreType, NSBitmapImageRep, NSColorSpaceName,
        NSEvent, NSEventModifierFlags, NSImage, NSView, NSWindow, NSWindowDelegate,
        NSWindowStyleMask,
    };
    use objc2_audio_toolbox::{
        AURenderCallbackStruct, AudioComponentDescription, AudioComponentFindNext,
        AudioComponentInstance, AudioComponentInstanceNew, AudioOutputUnitStart,
        AudioOutputUnitStop, AudioUnitInitialize, AudioUnitRenderActionFlags, AudioUnitSetProperty,
        kAudioUnitManufacturer_Apple, kAudioUnitProperty_SetRenderCallback,
        kAudioUnitProperty_StreamFormat, kAudioUnitScope_Global, kAudioUnitScope_Input,
        kAudioUnitSubType_DefaultOutput, kAudioUnitType_Output,
    };
    use objc2_core_audio_types::{
        AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp, kAudioFormatLinearPCM,
        kLinearPCMFormatFlagIsSignedInteger,
    };
    use objc2_foundation::{
        MainThreadMarker, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
        NSString, NSTimer, ns_string,
    };

    use crate::platform::{PlatformRequest, PlatformState};
    use crate::{Input, KeyCode, KeyModifiers};

    pub fn run(
        update: impl FnMut(PlatformRequest) + 'static,
        frame_buffer: *mut u8,
        width: usize,
        height: usize,
    ) {
        let app = init_app(update, frame_buffer, width, height);
        init_audio();
        unsafe { app.finishLaunching() };
        app.run();
    }

    pub fn log(str: &str) {
        std::print!("{str}");
    }

    #[derive(Debug, Clone)]
    struct AppDelegateIvars {
        #[expect(unused)]
        window: Retained<NSWindow>,
        _timer: Retained<NSTimer>,
    }

    define_class!(
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[ivars = AppDelegateIvars]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl NSApplicationDelegate for Delegate {
            #[unsafe(method(applicationDidFinishLaunching:))]
            fn did_finish_launching(&self, notification: &NSNotification) {
                dbg!(notification);
                dbg!(self.ivars());
                start_audio();
                NSApplication::main(MainThreadMarker::from(self));
            }

            #[unsafe(method(applicationShouldTerminate:))]
            unsafe fn application_should_terminate(
                &self,
                _sender: &NSApplication,
            ) -> NSApplicationTerminateReply {
                stop_audio();
                NSApplicationTerminateReply::TerminateNow
            }

            #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
            unsafe fn application_should_terminate_after_last_window_closed(
                &self,
                _sender: &NSApplication,
            ) -> bool {
                true
            }
        }

        unsafe impl NSWindowDelegate for Delegate {
            #[unsafe(method(windowWillClose:))]
            fn window_will_close(&self, _notification: &NSNotification) {
                // Quit the application when the window is closed.
                unsafe { NSApplication::sharedApplication(self.mtm()).terminate(None) };
            }
        }
    );

    impl Delegate {
        fn new(
            mtm: MainThreadMarker,
            window: Retained<NSWindow>,
            view: &Retained<GameView>,
        ) -> Retained<Self> {
            let _timer = unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    0.0,
                    view,
                    objc2::sel!(update:),
                    None,
                    true,
                )
            };
            let this = Self::alloc(mtm).set_ivars(AppDelegateIvars { window, _timer });
            unsafe { msg_send![super(this), init] }
        }
    }

    struct GameViewIvars {
        fb: *mut u8,
        update: RefCell<Box<dyn FnMut(PlatformRequest)>>,
        last_time: RefCell<Instant>,
        window: Retained<NSWindow>,
    }

    define_class!(
        #[unsafe(super = NSView)]
        #[thread_kind = MainThreadOnly]
        #[ivars = GameViewIvars]
        struct GameView;

        unsafe impl NSObjectProtocol for GameView {}

        impl GameView {
            #[unsafe(method(drawRect:))]
            fn draw_rect(&self, rect: NSRect) {
                let fb = self.ivars().fb;
                let image_rep = unsafe {

                    let planes: [*const u8; 1] = [fb];
                    NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                        NSBitmapImageRep::alloc(),
                        planes.as_ptr() as *mut _,
                        WIDTH as isize,
                        HEIGHT as isize,
                        8,
                        4,
                        true,
                        false,
                        &*NSColorSpaceName::from_str("NSCalibratedRGBColorSpace"),
                        WIDTH as isize * 4,
                        32,
                    )
                };

                if let Some(image_rep) = image_rep {
                    unsafe {
                        let size = NSSize::new(WIDTH as f64, HEIGHT as f64);
                        let image = NSImage::initWithSize(NSImage::alloc(), size);
                        image.addRepresentation(&image_rep);
                        image.drawInRect(rect);
                    }
                }
            }

            #[unsafe(method(update:))]
            fn update(&self, _timer: &NSTimer) {
                update(self, self.ivars());
            }

            #[unsafe(method(acceptsFirstResponder))]
            fn accepts_first_responder(&self) -> bool {
                true
            }

            #[unsafe(method(keyDown:))]
            fn key_down(&self, event: &NSEvent) {
                let mut update = self.ivars().update.borrow_mut();
                unsafe {
                    update(PlatformRequest::Input(Input::Key {
                        code: KEY_CODE_LUT[event.keyCode() as usize],
                        modifiers: KeyModifiers::from(event.modifierFlags()),
                        pressed: true,
                        repeat: event.isARepeat(),
                    }));
                }
            }

            #[unsafe(method(keyUp:))]
            fn key_up(&self, event: &NSEvent) {
                let mut update = self.ivars().update.borrow_mut();
                unsafe {
                    update(PlatformRequest::Input(Input::Key {
                        code: KEY_CODE_LUT[event.keyCode() as usize],
                        modifiers: KeyModifiers::from(event.modifierFlags()),
                        pressed: false,
                        repeat: event.isARepeat(),
                    }));
                }
            }

            #[unsafe(method(mouseMoved:))]
            fn mouse_moved(&self, event: &NSEvent) {
                let mut update = self.ivars().update.borrow_mut();
                unsafe {
                    update(PlatformRequest::Input(Input::MouseMoved {
                        dx: event.deltaX() as f32,
                        dy: event.deltaY() as f32,
                    }));
                }
            }

            #[unsafe(method(flagsChanged:))]
            fn flags_changed(&self, event: &NSEvent) {
                static mut PREVIOUS_MODIFIER_FLAGS: NSEventModifierFlags = NSEventModifierFlags(0);

                unsafe {
                    let current_flags = event.modifierFlags();
                    #[allow(static_mut_refs)]
                    let changed = current_flags.bits() ^ PREVIOUS_MODIFIER_FLAGS.bits();
                    PREVIOUS_MODIFIER_FLAGS = current_flags;
                    let pressed = (current_flags.bits() & changed) != 0;
                    let mut update = self.ivars().update.borrow_mut();

                    if changed & NSEventModifierFlags::Shift.bits() != 0 {
                        update(PlatformRequest::Input(Input::Key {
                            // TODO: LeftShift vs RightShift
                            code: KeyCode::LeftShift,
                            modifiers: KeyModifiers::from(current_flags),
                            pressed,
                            repeat: false,
                        }));
                    }

                    if changed & NSEventModifierFlags::Control.bits() != 0 {
                        update(PlatformRequest::Input(Input::Key {
                            // TODO: LeftAlt vs RightAlt,
                            code: KeyCode::LeftControl,
                            modifiers: KeyModifiers::from(current_flags),
                            pressed,
                            repeat: false,
                        }));
                    }

                    if changed & NSEventModifierFlags::Option.bits() != 0 {
                        update(PlatformRequest::Input(Input::Key {
                            // TODO: LeftAlt vs RightAlt,
                            code: KeyCode::LeftAlt,
                            modifiers: KeyModifiers::from(current_flags),
                            pressed,
                            repeat: false,
                        }));
                    }

                    if changed & NSEventModifierFlags::Command.bits() != 0 {
                        // TODO: need command
                    }
                }
            }
        }
    );

    impl GameView {
        fn new(
            mtm: MainThreadMarker,
            window: Retained<NSWindow>,
            update: impl FnMut(PlatformRequest) + 'static,
            frame_buffer: *mut u8,
        ) -> Retained<Self> {
            let ivars = GameViewIvars {
                fb: frame_buffer,
                update: RefCell::new(Box::new(update)),
                last_time: RefCell::new(Instant::now()),
                window,
            };
            let this = Self::alloc(mtm).set_ivars(ivars);
            unsafe { msg_send![super(this), init] }
        }
    }

    static mut AUDIO_UNIT: AudioComponentInstance = null_mut();
    const SAMPLE_RATE: f32 = 44_100.0;
    const CHANNELS: usize = 2;

    fn start_audio() {
        unsafe {
            let result = AudioOutputUnitStart(AUDIO_UNIT);
            assert_eq!(result, 0);
        }
    }

    fn stop_audio() {
        unsafe {
            let result = AudioOutputUnitStop(AUDIO_UNIT);
            assert_eq!(result, 0);
        }
    }

    fn init_audio() {
        use core::ptr::{NonNull, null_mut};

        let mut unit = core::ptr::null_mut();
        let desc = AudioComponentDescription {
            componentType: kAudioUnitType_Output,
            componentSubType: kAudioUnitSubType_DefaultOutput,
            componentManufacturer: kAudioUnitManufacturer_Apple,
            componentFlags: 0,
            componentFlagsMask: 0,
        };

        let stream_desc = AudioStreamBasicDescription {
            mSampleRate: SAMPLE_RATE as f64,
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kLinearPCMFormatFlagIsSignedInteger,
            mBytesPerPacket: 4,
            mFramesPerPacket: 1,
            mBytesPerFrame: 4,
            mChannelsPerFrame: 2,
            mBitsPerChannel: 16,
            mReserved: 0,
        };
        let callback = AURenderCallbackStruct {
            inputProc: Some(audio_callback),
            inputProcRefCon: null_mut(),
        };

        unsafe {
            let component = AudioComponentFindNext(null_mut(), NonNull::from(&desc));
            assert!(!component.is_null());
            let result = AudioComponentInstanceNew(component, NonNull::from(&mut unit));
            assert_eq!(result, 0);
            set_property(unit, kAudioUnitProperty_StreamFormat, &stream_desc);
            set_property(unit, kAudioUnitProperty_SetRenderCallback, &callback);
            let result = AudioUnitInitialize(unit);
            assert_eq!(result, 0);
            AUDIO_UNIT = unit;

            fn set_property<T>(unit: AudioComponentInstance, prop: u32, value: &T) {
                unsafe {
                    let result = AudioUnitSetProperty(
                        unit,
                        prop,
                        kAudioUnitScope_Input,
                        kAudioUnitScope_Global,
                        value as *const _ as *const c_void,
                        std::mem::size_of::<T>() as u32,
                    );
                    assert_eq!(result, 0);
                }
            }
        }
    }

    fn init_app(
        update: impl FnMut(PlatformRequest) + 'static,
        frame_buffer: *mut u8,
        width: usize,
        height: usize,
    ) -> Retained<NSApplication> {
        unsafe {
            WIDTH = width;
            HEIGHT = height;
        }

        let mtm = MainThreadMarker::new().unwrap();
        let app = NSApplication::sharedApplication(mtm);

        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(width as f64, height as f64),
                ),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable,
                // | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe {
            window.setReleasedWhenClosed(false);
        }

        window.setTitle(ns_string!("glazer app"));
        window.center();
        window.makeKeyAndOrderFront(None);
        window.setAcceptsMouseMovedEvents(true);

        let custom_view = GameView::new(mtm, window.clone(), update, frame_buffer);
        window.makeFirstResponder(Some(&custom_view));
        let delegate = Delegate::new(mtm, window.clone(), &custom_view);
        window.setContentView(Some(&*custom_view.into_super()));
        app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        // Activate the application.
        // Required when launching unbundled (as is done with Cargo).
        #[expect(deprecated)]
        app.activateIgnoringOtherApps(true);
        app
    }

    static mut WIDTH: usize = 0;
    static mut HEIGHT: usize = 0;

    fn update(view: &GameView, ivars: &GameViewIvars) {
        let now = Instant::now();
        let delta = {
            let mut last_time = ivars.last_time.borrow_mut();
            let delta = now.duration_since(*last_time).as_secs_f32();
            *last_time = now;
            delta
        };

        let fps = if delta > 0.0 { 1.0 / delta } else { 0.0 };
        let title = format!("glazer app - {:.2}", fps);
        ivars.window.setTitle(&*NSString::from_str(&title));

        let fb = ivars.fb;
        let indices = AUDIO_SAMPLES_INDICES.load(Ordering::Acquire);
        let write_index = (indices >> 32) as usize;
        assert_eq!(write_index % CHANNELS, 0);
        let wrapped_read_index = (indices & u32::MAX as u64) as usize;
        assert_eq!(wrapped_read_index % CHANNELS, 0);

        let samples_to_write = if write_index >= wrapped_read_index {
            (wrapped_read_index + AUDIO_SAMPLES_LEN - write_index - CHANNELS) % AUDIO_SAMPLES_LEN
        } else {
            wrapped_read_index - write_index - CHANNELS
        };

        let mut update = ivars.update.borrow_mut();
        unsafe {
            update(PlatformRequest::Update(PlatformState {
                delta,
                //
                frame_buffer: fb,
                width: WIDTH,
                height: HEIGHT,
                //
                samples: &mut GAME_SAMPLES[..samples_to_write],
                channels: CHANNELS,
                sample_rate: SAMPLE_RATE,
            }));
            view.setNeedsDisplay(true);

            let mut index = write_index;
            for sample in GAME_SAMPLES[..samples_to_write].iter() {
                AUDIO_SAMPLES[index] = *sample;
                index = (index + 1) % AUDIO_SAMPLES_LEN;
            }
        }

        AUDIO_SAMPLES_INDICES
            .fetch_update(Ordering::Release, Ordering::Acquire, |current_indices| {
                let current_read_index = current_indices & u32::MAX as u64;
                let new_write_index = ((write_index + samples_to_write) % AUDIO_SAMPLES_LEN) as u64;
                Some((new_write_index << 32) | current_read_index)
            })
            .unwrap();
    }

    const AUDIO_SAMPLES_LEN: usize = 1024 * 4;
    static mut AUDIO_SAMPLES: [i16; AUDIO_SAMPLES_LEN] = [0; AUDIO_SAMPLES_LEN];
    // secondary buffer for the game to write to
    static mut GAME_SAMPLES: [i16; AUDIO_SAMPLES_LEN] = [0; AUDIO_SAMPLES_LEN];
    // write index is packed into top 32 bits, read index in bottom 32 bits
    static AUDIO_SAMPLES_INDICES: AtomicU64 = AtomicU64::new((2 << 32) | 0);

    unsafe extern "C-unwind" fn audio_callback(
        _ref_con: NonNull<c_void>,
        _action_flags: NonNull<AudioUnitRenderActionFlags>,
        _time_stamp: NonNull<AudioTimeStamp>,
        _bus: u32,
        frames: u32,
        data: *mut AudioBufferList,
    ) -> i32 {
        let frames = frames as usize;
        unsafe {
            let len = (*data).mNumberBuffers as usize;
            assert_eq!(len, 1);

            let len = (*data).mBuffers[0].mDataByteSize as usize / 2;
            let samples = (*data).mBuffers[0].mData as *mut i16;
            let data = core::slice::from_raw_parts_mut(samples, len);
            assert!(len > 0);

            let indices = AUDIO_SAMPLES_INDICES.load(Ordering::Acquire);
            let wrapped_write_index = (indices >> 32) as usize;
            assert_eq!(wrapped_write_index % CHANNELS, 0);
            let read_index = (indices & u32::MAX as u64) as usize;
            assert_eq!(read_index % CHANNELS, 0);

            let available_samples = if wrapped_write_index >= read_index {
                wrapped_write_index - read_index
            } else {
                wrapped_write_index + AUDIO_SAMPLES_LEN - read_index
            };

            let samples_needed = frames * CHANNELS;
            let samples_to_read = available_samples.min(samples_needed);

            let frames_to_read = samples_to_read / CHANNELS;
            let mut index = read_index;
            assert_eq!(CHANNELS, 2);
            for frame in data.chunks_mut(CHANNELS).take(frames_to_read) {
                frame[0] = AUDIO_SAMPLES[index];
                frame[1] = AUDIO_SAMPLES[index + 1];
                index = (index + CHANNELS) % AUDIO_SAMPLES_LEN;
            }

            if frames_to_read < frames {
                crate::log!("ERROR: audio underrun {} samples", frames - frames_to_read);
                assert_eq!(CHANNELS, 2);
                for i in frames_to_read..frames {
                    data[i * CHANNELS] = 0;
                    data[i * CHANNELS + 1] = 0;
                }
            }

            AUDIO_SAMPLES_INDICES
                .fetch_update(Ordering::Release, Ordering::Acquire, |current_indices| {
                    let current_write_index = current_indices >> 32;
                    let new_read_index = (read_index + samples_to_read) % AUDIO_SAMPLES_LEN;
                    Some((current_write_index << 32) | new_read_index as u64)
                })
                .unwrap();
        }
        0
    }

    impl From<NSEventModifierFlags> for KeyModifiers {
        fn from(value: NSEventModifierFlags) -> Self {
            let mut mods = 0;
            for modifier in value.iter() {
                mods |= match modifier {
                    NSEventModifierFlags::CapsLock => KeyModifiers::CAPSLOCK,
                    NSEventModifierFlags::Shift => KeyModifiers::SHIFT,
                    NSEventModifierFlags::Control => KeyModifiers::CONTROL,
                    NSEventModifierFlags::Option => KeyModifiers::OPTION,
                    NSEventModifierFlags::Command => KeyModifiers::COMMAND,
                    NSEventModifierFlags::NumericPad => KeyModifiers::NUMERIC_PAD,
                    NSEventModifierFlags::Help => KeyModifiers::HELP,
                    NSEventModifierFlags::Function => KeyModifiers::FUNCTION,
                    NSEventModifierFlags::DeviceIndependentFlagsMask => KeyModifiers::CLEAR,
                    _ => KeyModifiers::CLEAR,
                }
                .0;
            }
            KeyModifiers(mods)
        }
    }

    // https://gist.github.com/eegrok/949034
    const KEY_CODE_LUT: [KeyCode; 128] = {
        let mut lut = [KeyCode::Unknown; 128];
        lut[0x00] = KeyCode::KeyA;
        lut[0x01] = KeyCode::KeyS;
        lut[0x02] = KeyCode::KeyD;
        lut[0x03] = KeyCode::KeyF;
        lut[0x04] = KeyCode::KeyH;
        lut[0x05] = KeyCode::KeyG;
        lut[0x06] = KeyCode::KeyZ;
        lut[0x07] = KeyCode::KeyX;
        lut[0x08] = KeyCode::KeyC;
        lut[0x09] = KeyCode::KeyV;
        lut[0x0A] = KeyCode::NonUSBackslash;
        lut[0x0B] = KeyCode::KeyB;
        lut[0x0C] = KeyCode::KeyQ;
        lut[0x0D] = KeyCode::KeyW;
        lut[0x0E] = KeyCode::KeyE;
        lut[0x0F] = KeyCode::KeyR;
        lut[0x10] = KeyCode::KeyY;
        lut[0x11] = KeyCode::KeyT;
        lut[0x12] = KeyCode::Num1;
        lut[0x13] = KeyCode::Num2;
        lut[0x14] = KeyCode::Num3;
        lut[0x15] = KeyCode::Num4;
        lut[0x16] = KeyCode::Num6;
        lut[0x17] = KeyCode::Num5;
        lut[0x18] = KeyCode::EqualSign;
        lut[0x19] = KeyCode::Num9;
        lut[0x1A] = KeyCode::Num7;
        lut[0x1B] = KeyCode::Hyphen;
        lut[0x1C] = KeyCode::Num8;
        lut[0x1D] = KeyCode::Num0;
        lut[0x1E] = KeyCode::CloseBracket;
        lut[0x1F] = KeyCode::KeyO;
        lut[0x20] = KeyCode::KeyU;
        lut[0x21] = KeyCode::OpenBracket;
        lut[0x22] = KeyCode::KeyI;
        lut[0x23] = KeyCode::KeyP;
        lut[0x24] = KeyCode::Return;
        lut[0x25] = KeyCode::KeyL;
        lut[0x26] = KeyCode::KeyJ;
        lut[0x27] = KeyCode::Quote;
        lut[0x28] = KeyCode::KeyK;
        lut[0x29] = KeyCode::Semicolon;
        lut[0x2A] = KeyCode::Backslash;
        lut[0x2B] = KeyCode::Comma;
        lut[0x2C] = KeyCode::Slash;
        lut[0x2D] = KeyCode::KeyN;
        lut[0x2E] = KeyCode::KeyM;
        lut[0x2F] = KeyCode::Period;
        lut[0x30] = KeyCode::Tab;
        lut[0x31] = KeyCode::Spacebar;
        lut[0x32] = KeyCode::NonUSPound;
        lut[0x33] = KeyCode::DeleteOrBackspace;
        lut[0x34] = KeyCode::Return;
        lut[0x35] = KeyCode::Escape;
        lut[0x5F] = KeyCode::Separator;
        lut[0x72] = KeyCode::Insert;
        lut[0x73] = KeyCode::Home;
        lut[0x74] = KeyCode::PageUp;
        lut[0x75] = KeyCode::DeleteForward;
        lut[0x77] = KeyCode::End;
        lut[0x79] = KeyCode::PageDown;
        lut[0x7B] = KeyCode::LeftArrow;
        lut[0x7C] = KeyCode::RightArrow;
        lut[0x7D] = KeyCode::DownArrow;
        lut[0x7E] = KeyCode::UpArrow;
        lut
    };
}

#[cfg(target_arch = "wasm32")]
pub mod wasm {
    use alloc::boxed::Box;
    use alloc::vec::Vec;

    use wasm_bindgen::prelude::*;
    use web_sys::{
        AudioContext, AudioProcessingEvent, CanvasRenderingContext2d, HtmlCanvasElement, ImageData,
    };

    use crate::{Audio, platform::PlatformState};

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = console)]
        pub fn log(s: &str);
    }

    fn init_canvas() -> HtmlCanvasElement {
        let window = web_sys::window().unwrap();
        let document = window.document().unwrap();

        let canvas = document
            .create_element("canvas")
            .unwrap()
            .dyn_into::<HtmlCanvasElement>()
            .unwrap();
        canvas.set_width(600);
        canvas.set_height(600);
        document.body().unwrap().append_child(&canvas).unwrap();
        canvas
    }

    fn init_audio(mut audio: impl FnMut(Audio) + 'static) {
        let audio_context = AudioContext::new().unwrap();
        let processor = audio_context.create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(
            2048, 0, 1
        ).unwrap();

        let mut buf = [0.0; 2048];
        let audio_closure = Closure::wrap(Box::new(move |event: AudioProcessingEvent| {
            let output_buffer = event.output_buffer().unwrap();
            let sample_rate = output_buffer.sample_rate();
            audio(Audio {
                samples: &mut buf,
                channels: 1,
                sample_rate,
                delta: 1.0 / 60.0,
            });
            output_buffer.copy_to_channel(&buf, 0).unwrap();
        }) as Box<dyn FnMut(AudioProcessingEvent)>);

        processor.set_onaudioprocess(Some(audio_closure.as_ref().unchecked_ref()));
        processor
            .connect_with_audio_node(&audio_context.destination())
            .unwrap();
        audio_closure.forget();
    }

    fn game_loop(
        mut update: impl FnMut(PlatformState) + 'static,
        context: CanvasRenderingContext2d,
        mut framebuffer: Vec<u8>,
    ) {
        let closure = Closure::once_into_js(move || {
            update(PlatformState {
                frame_buffer: framebuffer.as_mut_slice(),
                width: 600,
                height: 600,
                delta: 1.0 / 60.0,
            });
            let image_data = ImageData::new_with_u8_clamped_array_and_sh(
                wasm_bindgen::Clamped(framebuffer.as_slice()),
                600,
                600,
            )
            .unwrap();
            context.put_image_data(&image_data, 0.0, 0.0).unwrap();
            game_loop(update, context, framebuffer);
        });
        web_sys::window()
            .unwrap()
            .request_animation_frame(closure.as_ref().unchecked_ref())
            .unwrap();
    }

    #[macro_export]
    macro_rules! log {
        () => {
            $crate::platform::wasm::log("")
        };
        ($($arg:tt)*) => {{
            $crate::platform::wasm::log(&alloc::format!($($arg)*));
        }};
    }

    pub fn run(update: impl FnMut(PlatformState) + 'static, audio: impl FnMut(Audio) + 'static) {
        let canvas = init_canvas();
        let context = canvas
            .get_context("2d")
            .unwrap()
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>()
            .unwrap();

        let mut framebuffer = Vec::with_capacity(600 * 600 * 4);
        framebuffer.extend((0..600usize * 600 * 4).map(|i| i as u8));
        init_audio(audio);
        game_loop(update, context, framebuffer);
    }
}
