#![no_std]

extern crate alloc;

mod utah_teapot;

pub const MAX_WIDTH: usize = 640;
pub const MAX_HEIGHT: usize = 360;
pub const MAX_PIXELS: usize = MAX_WIDTH * MAX_HEIGHT;

pub fn frame_buffer() -> &'static mut [rast::Srgb] {
    static mut FRAME_BUFFER: [rast::Srgb; MAX_PIXELS] =
        [rast::Srgb::new(255, 255, 255, 255); MAX_PIXELS];
    static mut INIT: bool = false;

    // ## Safety
    //
    // `FRAME_BUFFER` is locally scoped. `INIT` verifies that this function
    // has only been called once. There cannot exist any other mutable references
    // to `FRAME_BUFFER` with safe Rust code.
    unsafe {
        if INIT {
            panic!("tried to call `frame_buffer` twice");
        }
        INIT = true;
        #[allow(static_mut_refs)]
        &mut FRAME_BUFFER
    }
}

pub fn memory() -> Memory<'static> {
    static mut DEPTH_BUFFER: [f32; MAX_PIXELS] = [1.0; MAX_PIXELS];
    static mut INIT: bool = false;

    // ## Safety
    //
    // `DEPTH_BUFFER` is locally scoped. `INIT` verifies that this function
    // has only been called once. There cannot exist any other mutable references
    // to `DEPTH_BUFFER` with safe Rust code.
    unsafe {
        if INIT {
            panic!("tried to call `memory` twice");
        }
        INIT = true;
        Memory {
            #[allow(static_mut_refs)]
            depth_buffer: &mut DEPTH_BUFFER,
            camera: rast::Vec3::new(0.0, 1.5, -5.0),
            ..Default::default()
        }
    }
}

#[derive(Default)]
pub struct Memory<'a> {
    depth_buffer: &'a mut [f32],

    camera: rast::Vec3,
    left_pressed: bool,
    right_pressed: bool,
    forward_pressed: bool,
    back_pressed: bool,
    up_pressed: bool,
    down_pressed: bool,
    pitch: f32,
    yaw: f32,

    t: f32,
    angle: f32,
    phase: f32,
}

pub fn handle_input(glazer::PlatformInput { memory, input }: glazer::PlatformInput<Memory>) {
    match input {
        glazer::Input::Key { code, pressed, .. } => match code {
            glazer::KeyCode::KeyW => {
                memory.forward_pressed = pressed;
            }
            glazer::KeyCode::KeyS => {
                memory.back_pressed = pressed;
            }
            glazer::KeyCode::KeyA => {
                memory.left_pressed = pressed;
            }
            glazer::KeyCode::KeyD => {
                memory.right_pressed = pressed;
            }
            glazer::KeyCode::Spacebar => {
                memory.up_pressed = pressed;
            }
            glazer::KeyCode::LeftShift => {
                memory.down_pressed = pressed;
            }
            _ => {}
        },
        glazer::Input::MouseMoved { dx, dy } => {
            let sensitivity = 0.005;
            memory.yaw += dx * sensitivity;
            memory.pitch += dy * sensitivity;

            // Keep the camera's angle from going too high/low.
            const SAFE_FRAC_PI_2: f32 = core::f32::consts::FRAC_PI_2 - 0.0001;
            if memory.pitch < -SAFE_FRAC_PI_2 {
                memory.pitch = -SAFE_FRAC_PI_2;
            } else if memory.pitch > SAFE_FRAC_PI_2 {
                memory.pitch = SAFE_FRAC_PI_2;
            }
        }
    }
}

pub fn update_and_render(
    glazer::PlatformUpdate {
        memory,
        delta,
        //
        frame_buffer,
        width,
        height,
        //
        samples,
        channels,
        sample_rate,
        ..
    }: glazer::PlatformUpdate<Memory, rast::Srgb>,
) {
    camera(memory, delta);
    audio(memory, samples, channels, sample_rate);
    render(memory, frame_buffer, width, height, delta);
}

fn camera(memory: &mut Memory, delta: f32) {
    use rast::*;

    let speed = 5.0 * delta;
    let mut camera_delta = Vec3::ZERO;

    if memory.forward_pressed {
        camera_delta += Vec3::z(speed);
    }
    if memory.back_pressed {
        camera_delta -= Vec3::z(speed);
    }
    if memory.right_pressed {
        camera_delta += Vec3::x(speed);
    }
    if memory.left_pressed {
        camera_delta -= Vec3::x(speed);
    }

    if camera_delta != Vec3::ZERO {
        memory.camera += camera_delta.rotate_y(memory.yaw);
    }

    if memory.up_pressed {
        memory.camera.y += speed;
    }
    if memory.down_pressed {
        memory.camera.y -= speed;
    }
}

fn audio(memory: &mut Memory, samples: &mut [i16], channels: usize, sample_rate: f32) {
    use core::f32::consts::TAU;
    let freq = 440.0 + memory.camera.normalize().element_sum() * 50.0;
    for i in 0..samples.len() / channels {
        memory.phase += freq * TAU / sample_rate;
        if memory.phase >= TAU {
            memory.phase -= TAU;
        }

        let s = libm::sinf(memory.phase);
        for c in 0..channels {
            samples[i * channels + c] = (s * 0.1 * i16::MAX as f32) as i16 * 0;
        }
    }
}

fn render(
    memory: &mut Memory,
    frame_buffer: &mut [rast::Srgb],
    width: usize,
    height: usize,
    delta: f32,
) {
    memory.depth_buffer.fill(f32::MAX);
    memory.angle = (memory.angle + delta) % core::f32::consts::TAU;

    for x in -1..=1 {
        draw_model(
            frame_buffer,
            memory.depth_buffer,
            width,
            height,
            memory.camera,
            memory.pitch,
            memory.yaw,
            &crate::utah_teapot::UTAH_TEAPOT,
            rast::Vec3::x(x as f32 * 10.0),
            rast::Vec3::y(memory.angle),
        );
    }

    fill_background(memory, frame_buffer, width, height, delta);
}

fn fill_background(
    memory: &mut Memory,
    frame_buffer: &mut [rast::Srgb],
    width: usize,
    height: usize,
    delta: f32,
) {
    memory.t += delta * 50.0;
    memory.t %= 255.0;
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            if memory.depth_buffer[index] == f32::MAX {
                let r = ((x as f32 + memory.t) % 255.0) as u8;
                let g = 0;
                let b = ((y as f32 + memory.t) % 255.0) as u8;
                frame_buffer[index] = rast::Srgb::new(r, g, b, 255);
            }
        }
    }
}

fn draw_model(
    frame_buffer: &mut [rast::Srgb],
    depth_buffer: &mut [f32],
    width: usize,
    height: usize,
    camera: rast::Vec3,
    pitch: f32,
    yaw: f32,
    vertices: &[rast::Vec3],
    translation: rast::Vec3,
    pitch_yaw_roll: rast::Vec3,
) {
    debug_assert!(vertices.len() % 3 == 0);
    for face in vertices.chunks(3) {
        let v1 = transform_vertex(translation, pitch_yaw_roll, face[0]);
        let v2 = transform_vertex(translation, pitch_yaw_roll, face[1]);
        let v3 = transform_vertex(translation, pitch_yaw_roll, face[2]);

        if let Some((v1, v2, v3)) =
            triangle_world_to_camera_space_clipped(camera, pitch, yaw, v1, v2, v3)
        {
            let (v1, v2, v3) = triangle_camera_to_screen_space(width, height, v1, v2, v3);
            rast::rast_triangle_checked(
                frame_buffer,
                depth_buffer,
                width,
                height,
                v1,
                v2,
                v3,
                rast::LinearRgb::rgb(1.0, 0.0, 0.0),
                rast::LinearRgb::rgb(0.0, 1.0, 0.0),
                rast::LinearRgb::rgb(0.0, 0.0, 1.0),
                rast::ColorShader,
            );
        }
    }
}

#[expect(unused)]
fn draw_model_backface_culled(
    frame_buffer: &mut [rast::Srgb],
    depth_buffer: &mut [f32],
    width: usize,
    height: usize,
    camera: rast::Vec3,
    yaw: f32,
    pitch: f32,
    vertices: &[rast::Vec3],
    translation: rast::Vec3,
    pitch_yaw_roll: rast::Vec3,
) {
    debug_assert!(vertices.len() % 3 == 0);
    for face in vertices.chunks(3) {
        let v1 = transform_vertex(translation, pitch_yaw_roll, face[0]);
        let v2 = transform_vertex(translation, pitch_yaw_roll, face[1]);
        let v3 = transform_vertex(translation, pitch_yaw_roll, face[2]);

        if let Some((v1, v2, v3)) =
            triangle_world_to_camera_space_clipped(camera, yaw, pitch, v1, v2, v3)
        {
            // https://en.wikipedia.org/wiki/Back-face_culling#Implementation
            let normal = (v3 - v1).cross(v2 - v1);
            if v1.dot(normal) >= 0.0 {
                let (v1, v2, v3) = triangle_camera_to_screen_space(width, height, v1, v2, v3);
                rast::rast_triangle_checked(
                    frame_buffer,
                    depth_buffer,
                    width,
                    height,
                    v1,
                    v2,
                    v3,
                    rast::LinearRgb::rgb(1.0, 0.0, 0.0),
                    rast::LinearRgb::rgb(0.0, 1.0, 0.0),
                    rast::LinearRgb::rgb(0.0, 0.0, 1.0),
                    rast::ColorShader,
                );
            }
        }
    }
}

fn transform_vertex(
    translation: rast::Vec3,
    pitch_yaw_roll: rast::Vec3,
    v: rast::Vec3,
) -> rast::Vec3 {
    let mut rotated = v;
    if pitch_yaw_roll.z != 0.0 {
        rotated = rotated.rotate_z(pitch_yaw_roll.z);
    }
    if pitch_yaw_roll.y != 0.0 {
        rotated = rotated.rotate_y(pitch_yaw_roll.y);
    }
    if pitch_yaw_roll.x != 0.0 {
        rotated = rotated.rotate_x(pitch_yaw_roll.x);
    }
    rotated + translation
}

fn triangle_world_to_camera_space_clipped(
    camera: rast::Vec3,
    pitch: f32,
    yaw: f32,
    v1: rast::Vec3,
    v2: rast::Vec3,
    v3: rast::Vec3,
) -> Option<(rast::Vec3, rast::Vec3, rast::Vec3)> {
    vertex_world_to_camera_space_clipped(camera, pitch, yaw, v1).and_then(|v1| {
        vertex_world_to_camera_space_clipped(camera, pitch, yaw, v2).and_then(|v2| {
            vertex_world_to_camera_space_clipped(camera, pitch, yaw, v3).map(|v3| (v1, v2, v3))
        })
    })
}

fn vertex_world_to_camera_space_clipped(
    camera: rast::Vec3,
    pitch: f32,
    yaw: f32,
    v: rast::Vec3,
) -> Option<rast::Vec3> {
    let near_clip = 0.5;
    let camera_space = (v - camera).rotate_y(-yaw).rotate_x(-pitch);
    (camera_space.z > near_clip).then_some(camera_space)
}

fn triangle_camera_to_screen_space(
    width: usize,
    height: usize,
    v1: rast::Vec3,
    v2: rast::Vec3,
    v3: rast::Vec3,
) -> (rast::Vec3, rast::Vec3, rast::Vec3) {
    (
        vertex_camera_to_screen_space(width, height, v1),
        vertex_camera_to_screen_space(width, height, v2),
        vertex_camera_to_screen_space(width, height, v3),
    )
}

fn vertex_camera_to_screen_space(width: usize, height: usize, v: rast::Vec3) -> rast::Vec3 {
    let focal_length = 1.5;
    let mut proj = v.to_vec2() * focal_length / v.z;
    proj.x *= height as f32 / width as f32;
    rast::Vec3::new(
        (proj.x + 1.0) / 2.0 * width as f32,
        (1.0 - (proj.y + 1.0) / 2.0) * height as f32,
        v.z,
    )
}
