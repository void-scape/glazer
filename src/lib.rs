pub struct App<T: 'static> {
    platform: fn(T, fn(Platform<T>)),
    memory: T,
}

impl<T> App<T> {
    pub fn new(memory: T) -> Self {
        #[cfg(not(target_os = "macos"))]
        panic!("unsupported target platform");

        #[cfg(target_os = "macos")]
        Self {
            platform: appkit::run,
            memory,
        }
    }

    pub fn run(self, update_and_render: fn(Platform<T>)) {
        (self.platform)(self.memory, update_and_render)
    }
}

pub struct Platform<'a, T> {
    pub memory: &'a mut T,
    pub frame_buffer: &'a mut [u8],
    pub width: usize,
    pub height: usize,
    pub delta: f32,
}

#[cfg(target_os = "macos")]
mod appkit {
    use std::cell::RefCell;
    use std::time::Instant;

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

    use crate::Platform;

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
        update: RefCell<Box<dyn FnMut(&mut [u8], usize, usize, f32)>>,
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
                    update(fb.as_mut_slice(), 600, 600, delta);
                    unsafe { self.setNeedsDisplay(true) };
                }
            }
        }
    );

    impl GameView {
        fn new<Memory: 'static>(
            mtm: MainThreadMarker,
            window: Retained<NSWindow>,
            mut memory: Memory,
            update_and_render: fn(Platform<Memory>),
        ) -> Retained<Self> {
            let ivars = GameViewIvars {
                fb: RefCell::new(Box::new([255; 600 * 600 * 4])),
                update: RefCell::new(Box::new(move |fb, w, h, delta| {
                    update_and_render(Platform {
                        memory: &mut memory,
                        frame_buffer: fb,
                        width: w,
                        height: h,
                        delta,
                    });
                })),
                last_time: RefCell::new(Instant::now()),
                window,
            };
            let this = Self::alloc(mtm).set_ivars(ivars);
            unsafe { msg_send![super(this), init] }
        }
    }

    pub fn run<Memory: 'static>(memory: Memory, update_and_render: fn(Platform<Memory>)) {
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

        let custom_view = GameView::new(mtm, window.clone(), memory, update_and_render);
        let view = window.contentView().unwrap();
        let delegate = Delegate::new(mtm, window, &custom_view);
        unsafe { view.addSubview(&*custom_view.into_super()) };
        app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        // Activate the application.
        // Required when launching unbundled (as is done with Cargo).
        #[expect(deprecated)]
        app.activateIgnoringOtherApps(true);

        app.run();
    }
}
