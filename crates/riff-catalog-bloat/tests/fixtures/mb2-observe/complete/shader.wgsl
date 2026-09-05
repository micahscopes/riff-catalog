struct Input {
    padding: u32,
}

@group(0) @binding(1) 
var<storage> input: Input;

fn mix_words(a0_: u32, a1_: u32) -> u32 {
    return ((a0_ * 3u) + a1_);
}

@vertex 
fn vs_fullscreen(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>((f32(((vi & 1u) << 2u)) - 1f), (f32(((vi & 2u) << 1u)) - 1f), 0f, 1f);
}

@fragment 
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let _e6 = mix_words(u32(pos.x), u32(pos.y));
    return unpack4x8unorm(_e6);
}
