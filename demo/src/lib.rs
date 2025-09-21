#![no_std]
extern crate alloc;

#[derive(Default)]
pub struct Memory {
    t: f32,
}

pub fn update_and_render(
    glazer::Platform {
        memory,
        frame_buffer,
        width,
        height,
        delta,
    }: glazer::Platform<Memory>,
) {
    memory.t += delta * 50.0;
    memory.t %= 255.0;
    let t = memory.t;

    for y in 0..height {
        for x in 0..width {
            let index = (y * width + x) * 4;
            frame_buffer[index] = ((x as f32 + t) % 255.0) as u8;
            frame_buffer[index + 1] = 0;
            frame_buffer[index + 2] = ((y as f32 + t) % 255.0) as u8;
            frame_buffer[index + 3] = 255;
        }
    }
}

pub fn process_audio(
    glazer::Audio {
        samples,
        channels,
        sample_rate,
        ..
    }: glazer::Audio,
) {
    use core::f32::consts::TAU;
    let freq = 440.0;
    static mut PHASE: f32 = 0.0;
    unsafe {
        for i in 0..samples.len() / channels {
            PHASE += freq * TAU / sample_rate;
            if PHASE >= TAU {
                PHASE -= TAU;
            }

            let s = libm::sinf(PHASE);
            for c in 0..channels {
                samples[i * channels + c] = s * 0.1;
            }
        }
    }
}
