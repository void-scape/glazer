#![no_std]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub struct App<T: 'static> {
    memory: T,
}

impl<T> App<T> {
    pub fn new(memory: T) -> Self {
        Self { memory }
    }

    pub fn run(self, update_and_render: fn(Platform<T>), process_audio: fn(Audio)) {
        let mut memory = self.memory;
        let update = move |state: PlatformState| {
            update_and_render(Platform {
                memory: &mut memory,
                frame_buffer: state.frame_buffer,
                width: state.width,
                height: state.height,
                delta: state.delta,
            })
        };

        #[cfg(target_arch = "wasm32")]
        wasm::run(update, process_audio);
        #[cfg(target_os = "macos")]
        appkit::run(update, process_audio);
    }
}

pub struct Platform<'a, T> {
    pub memory: &'a mut T,
    pub frame_buffer: &'a mut [u8],
    pub width: usize,
    pub height: usize,
    pub delta: f32,
}

pub struct Audio<'a> {
    pub samples: &'a mut [f32],
    pub sample_rate: f32,
    pub channels: usize,
    pub delta: f32,
}

struct PlatformState<'a> {
    frame_buffer: &'a mut [u8],
    width: usize,
    height: usize,
    delta: f32,
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use alloc::boxed::Box;
    use alloc::vec::Vec;

    use wasm_bindgen::prelude::*;
    use web_sys::{
        AudioContext, AudioProcessingEvent, CanvasRenderingContext2d, HtmlCanvasElement, ImageData,
    };

    use crate::{Audio, PlatformState};

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = console)]
        fn log(s: &str);
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

#[cfg(target_os = "macos")]
#[cfg(feature = "std")]
mod appkit {
    use std::boxed::Box;
    use std::cell::RefCell;
    use std::time::Instant;
    use std::{dbg, format, println};

    use coreaudio::audio_unit::SampleFormat;
    use coreaudio::audio_unit::render_callback::{self, data};
    use coreaudio::audio_unit::{AudioUnit, IOType};

    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate,
        NSApplicationTerminateReply, NSBackingStoreType, NSBitmapImageRep, NSColorSpaceName,
        NSImage, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
    };
    use objc2_foundation::{
        MainThreadMarker, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
        NSString, NSTimer, ns_string,
    };

    use crate::{Audio, PlatformState};

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
                NSApplication::main(MainThreadMarker::from(self));
            }

            #[unsafe(method(applicationShouldTerminate:))]
            unsafe fn application_should_terminate(
                &self,
                _sender: &NSApplication,
            ) -> NSApplicationTerminateReply {
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
        fb: RefCell<Box<[u8; 600 * 600 * 4]>>,
        update: RefCell<Box<dyn FnMut(PlatformState)>>,
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
                let Ok(fb) = self.ivars().fb.try_borrow() else {
                    return;
                };

                let image_rep = unsafe {
                    let planes: [*const u8; 1] = [fb.as_ptr()];
                    NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                        NSBitmapImageRep::alloc(),
                        planes.as_ptr() as *mut _,
                        600,
                        600,
                        8,
                        4,
                        true,
                        false,
                        &*NSColorSpaceName::from_str("NSCalibratedRGBColorSpace"),
                        600 * 4,
                        32,
                    )
                };

                if let Some(image_rep) = image_rep {
                    unsafe {
                        let size = NSSize::new(600.0, 600.0);
                        let image = NSImage::initWithSize(NSImage::alloc(), size);
                        image.addRepresentation(&image_rep);
                        image.drawInRect(rect);
                    }
                }
            }

            #[unsafe(method(update:))]
            fn update(&self, _timer: &NSTimer) {
                let ivars = self.ivars();

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

                if let Ok(mut fb) = ivars.fb.try_borrow_mut() {
                    let mut update = ivars.update.borrow_mut();
                    update(
                        PlatformState {
                            frame_buffer:
                            fb.as_mut_slice(),
                            width: 600,
                            height: 600,
                            delta,
                        },
                    );
                    unsafe { self.setNeedsDisplay(true) };
                }
            }
        }
    );

    impl GameView {
        fn new(
            mtm: MainThreadMarker,
            window: Retained<NSWindow>,
            update: impl FnMut(PlatformState) + 'static,
        ) -> Retained<Self> {
            let ivars = GameViewIvars {
                fb: RefCell::new(Box::new([255; 600 * 600 * 4])),
                update: RefCell::new(Box::new(update)),
                last_time: RefCell::new(Instant::now()),
                window,
            };
            let this = Self::alloc(mtm).set_ivars(ivars);
            unsafe { msg_send![super(this), init] }
        }
    }

    fn init_app(update: impl FnMut(PlatformState) + 'static) -> Retained<NSApplication> {
        let mtm = MainThreadMarker::new().unwrap();
        let app = NSApplication::sharedApplication(mtm);

        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(600.0, 600.0)),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };

        window.setTitle(ns_string!("glazer app"));
        window.center();
        window.makeKeyAndOrderFront(None);

        let custom_view = GameView::new(mtm, window.clone(), update);
        let view = window.contentView().unwrap();
        let delegate = Delegate::new(mtm, window, &custom_view);
        unsafe { view.addSubview(&*custom_view.into_super()) };
        app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        // Activate the application.
        // Required when launching unbundled (as is done with Cargo).
        #[expect(deprecated)]
        app.activateIgnoringOtherApps(true);
        app
    }

    fn init_audio(mut audio: impl FnMut(Audio) + 'static) -> AudioUnit {
        let mut audio_unit = AudioUnit::new(IOType::DefaultOutput).unwrap();
        let stream_format = audio_unit.output_stream_format().unwrap();
        println!("{:#?}", &stream_format);
        assert!(SampleFormat::F32 == stream_format.sample_format);

        let sample_rate = stream_format.sample_rate as f32;
        let channels = stream_format.channels as usize;
        let mut buf = alloc::vec::Vec::new();
        type Args = render_callback::Args<data::NonInterleaved<f32>>;
        audio_unit
            .set_render_callback(move |args| {
                let Args {
                    mut data,
                    num_frames,
                    ..
                } = args;
                buf.reserve_exact(num_frames * channels);
                if buf.len() < num_frames * channels {
                    buf.extend((0..num_frames * channels - buf.len()).map(|_| 0.0));
                }
                audio(Audio {
                    samples: buf.as_mut_slice(),
                    channels,
                    sample_rate,
                    delta: 1.0 / 60.0,
                });
                let mut j = 0;
                for i in 0..num_frames {
                    for channel in data.channels_mut() {
                        channel[i] = buf[j];
                        j += 1;
                    }
                }
                Ok(())
            })
            .unwrap();
        audio_unit.start().unwrap();
        audio_unit
    }

    pub fn run(update: impl FnMut(PlatformState) + 'static, audio: impl FnMut(Audio) + 'static) {
        let app = init_app(update);
        let _audio = init_audio(audio);
        unsafe { app.finishLaunching() };
        app.run();
    }
}
