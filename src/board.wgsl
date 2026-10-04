struct Camera {
 view_projection: mat4x4<f32>, eye: vec4<f32>, background: vec4<f32>,
 light: vec4<f32>, lighting: vec4<f32>, viewport: vec4<f32>,
};
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var surfaces: texture_2d_array<f32>;
@group(0) @binding(2) var surface_sampler: sampler;
@group(0) @binding(3) var reflections: texture_2d<f32>;
struct Input {
 @location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>,
 @location(3) m0: vec4<f32>, @location(4) m1: vec4<f32>, @location(5) m2: vec4<f32>, @location(6) m3: vec4<f32>,
 @location(7) tint: vec4<f32>, @location(8) params: vec4<f32>, @location(9) material: vec4<f32>,
};
struct Output {
 @builtin(position) clip: vec4<f32>, @location(0) world: vec3<f32>, @location(1) normal: vec3<f32>,
 @location(2) uv: vec2<f32>, @location(3) tint: vec4<f32>,
 @location(4) @interpolate(flat) params: vec4<f32>,
 @location(5) @interpolate(flat) material: vec4<f32>,
};
@vertex fn vs(v: Input) -> Output {
 let model = mat4x4<f32>(v.m0,v.m1,v.m2,v.m3);
 var out: Output; let world=model*vec4<f32>(v.position,1.0);
 out.clip=camera.view_projection*world;out.world=world.xyz;
 out.normal=normalize((model*vec4<f32>(v.normal,0.0)).xyz);
 out.uv=v.uv;out.tint=v.tint;out.params=v.params;out.material=v.material;return out;
}
@fragment fn fs(v: Output) -> @location(0) vec4<f32> {
 var base=v.tint;
 if v.params.x>=0.0 {base*=textureSample(surfaces,surface_sampler,v.uv,i32(v.params.x));}
 base.a*=v.material.w;
 if v.params.z>0.5 {return base;}
 let n=normalize(v.normal);let light=normalize(camera.light.xyz-v.world);
 let view=normalize(camera.eye.xyz-v.world);let h=normalize(light+view);
 let nl=max(dot(n,light),0.0);let nv=max(dot(n,view),0.001);let nh=max(dot(n,h),0.0);let vh=max(dot(view,h),0.0);
 let rough=clamp(sqrt(2.0/(v.material.z+2.0)),0.06,0.95);
 let a2=rough*rough*rough*rough;
 let d=a2/(3.14159265*pow(nh*nh*(a2-1.0)+1.0,2.0));
 let k=(rough+1.0)*(rough+1.0)/8.0;
 let geometry=(nv/(nv*(1.0-k)+k))*(nl/(nl*(1.0-k)+k));
 let metallic=select(0.0,0.75,v.params.y<0.25);
 let f0=mix(vec3<f32>(0.04),base.rgb,metallic);
 let fresnel=f0+(vec3<f32>(1.0)-f0)*pow(1.0-vh,5.0);
 let specular=d*geometry*fresnel/(4.0*nv*max(nl,0.001))*v.material.y;
 let fill=max(dot(n,normalize(vec3<f32>(0.8,0.4,-0.8))),0.0);
 let diffuse=base.rgb*(camera.lighting.x+nl*v.material.x+fill*0.12)*(1.0-metallic*0.5);
 let linear=diffuse+specular*nl;
 var color=pow(max(linear,vec3<f32>(0.0)),vec3<f32>(1.0/2.2));
 // Reflected geometry is rendered through the same camera below the board plane.
 // Sampling it in screen space gives a planar reflection clipped by each tile.
 let reflected=textureSampleLevel(reflections,surface_sampler,v.clip.xy/camera.viewport.xy,0.0);
 if v.params.w>0.5 {color=mix(color,reflected.rgb,reflected.a*camera.lighting.y);}
 return vec4<f32>(color,base.a);
}
