struct Camera { view_projection: mat4x4<f32> };
@group(0) @binding(0) var<uniform> camera: Camera;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) translation: vec3<f32>,
    @location(3) size: vec3<f32>,
    @location(4) color: vec3<f32>,
    @location(5) yaw: f32,
) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = camera.view_projection * vec4<f32>(rotate_yaw(position * size, yaw) + translation, 1.0);
    output.normal = rotate_yaw(normal, yaw);
    output.color = color;
    return output;
}

// World yaw convention: yaw 0 keeps local +Z on world +Z; increasing yaw turns +Z toward +X.
fn rotate_yaw(value: vec3<f32>, yaw: f32) -> vec3<f32> {
    let c = cos(yaw);
    let s = sin(yaw);
    return vec3<f32>(value.x * c + value.z * s, value.y, value.z * c - value.x * s);
}

@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let light = normalize(vec3<f32>(0.4, 0.8, 0.3));
    let intensity = 0.3 + 0.7 * max(dot(normalize(input.normal), light), 0.0);
    return vec4<f32>(input.color * intensity, 1.0);
}
