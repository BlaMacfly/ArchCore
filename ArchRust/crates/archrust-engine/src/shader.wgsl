// Pipeline éclairé simple d'ArchRust : lumière directionnelle + ambiante, texture diffuse,
// couleur de sommet (prélit RenderWare), test alpha.

struct Globals {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
    ambient: vec4<f32>,
    camera_pos: vec4<f32>,
};
@group(0) @binding(0) var<uniform> globals: Globals;

struct Instance {
    model: mat4x4<f32>,
    color: vec4<f32>,
};
@group(2) @binding(0) var<uniform> inst: Instance;

@group(1) @binding(0) var t_diffuse: texture_2d<f32>;
@group(1) @binding(1) var s_diffuse: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) world_pos: vec3<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    let world = inst.model * vec4<f32>(v.position, 1.0);
    out.clip = globals.view_proj * world;
    out.normal = normalize((inst.model * vec4<f32>(v.normal, 0.0)).xyz);
    out.uv = v.uv;
    out.color = v.color * inst.color;
    out.world_pos = world.xyz;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let tex = textureSample(t_diffuse, s_diffuse, in.uv);
    if (tex.a * in.color.a < 0.3) {
        discard;
    }
    let n = normalize(in.normal);
    let l = normalize(globals.light_dir.xyz);
    let diffuse = max(dot(n, l), 0.0);
    let light = globals.ambient.rgb + vec3<f32>(diffuse) * (1.0 - globals.ambient.rgb);
    // Léger brouillard de distance pour la profondeur.
    let dist = distance(in.world_pos, globals.camera_pos.xyz);
    let fog = clamp((dist - 30000.0) / 60000.0, 0.0, 0.85);
    let color = tex.rgb * in.color.rgb * light;
    return vec4<f32>(mix(color, vec3<f32>(0.55, 0.65, 0.8), fog), tex.a * in.color.a);
}
