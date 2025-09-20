use glazer::Platform;

struct Memory {}

fn main() {
    glazer::App::new(Memory {}).run(update_and_render);
}

fn update_and_render(platform: Platform<Memory>) {
    for chunk in platform.frame_buffer.chunks_mut(4) {
        chunk[0] += ((platform.delta * 5.0 * u8::MAX as f32) % 255.0).clamp(0.0, 255.0) as u8;
        chunk[1] += ((platform.delta * 5.0 * u8::MAX as f32) % 255.0).clamp(0.0, 255.0) as u8;
        chunk[2] += ((platform.delta * 5.0 * u8::MAX as f32) % 255.0).clamp(0.0, 255.0) as u8;
        chunk[3] = 255;
    }
}
