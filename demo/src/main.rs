#![no_std]
#![cfg_attr(not(feature = "std"), no_main)]

#[panic_handler]
#[cfg(not(feature = "std"))]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
#[cfg(not(feature = "std"))]
#[cfg(target_arch = "wasm32")]
mod alloc_impl {
    use wee_alloc::WeeAlloc;
    #[global_allocator]
    static ALLOC: WeeAlloc = WeeAlloc::INIT;
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen(start))]
fn main() {
    glazer::run(
        demo::memory(),
        demo::frame_buffer(),
        demo::MAX_WIDTH,
        demo::MAX_HEIGHT,
        demo::handle_input,
        demo::update_and_render,
    );
}
