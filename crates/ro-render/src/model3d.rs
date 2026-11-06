//! 3D monsters: the Granny (.gr2) models the original client draws instead of sprites (the
//! castle guardians, the Emperium, guild flags, treasure boxes). Posed and skinned on the
//! CPU each frame (a few hundred triangles each) and drawn with the map's model shader.
//!
//! Curve sampling (non-uniform quadratic B-splines) and skinning (bone world transform times
//! the file's inverse world transform) follow granny-ro-js (MIT, Copyright (c) 2026
//! Stitchuuuu).

use glam::{Mat3, Mat4, Quat, Vec3};
use ro_formats::gr2::{Bone, Curve, Gr2, Track};

use crate::scene::Vertex;

/// What the model is doing; each has its own clip (3dmob_bone\<id>_<clip>.gr2), falling
/// back to the model's own idle animation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clip {
    Idle = 0,
    Move = 1,
    Attack = 2,
    Damage = 3,
    Dead = 4,
}

impl Clip {
    pub const FILES: [(Clip, &'static str); 4] = [(Clip::Move, "move"), (Clip::Attack, "attack"), (Clip::Damage, "damage"), (Clip::Dead, "dead")];
}

struct Mesh {
    /// Into the model's textures.
    texture: Option<usize>,
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<[f32; 2]>,
    weights: Vec<[f32; 4]>,
    /// Skeleton bone indices.
    bones: Vec<[usize; 4]>,
    indices: Vec<u32>,
}

struct Anim {
    duration: f32,
    /// Per skeleton bone.
    tracks: Vec<Option<Track>>,
}

pub struct Model3d {
    meshes: Vec<Mesh>,
    bones: Vec<Bone>,
    clips: [Option<Anim>; 5],
    /// Model space to a y-up frame: the file's initial placement, then z-up to y-up (and
    /// mirrored: the world's east-north-up basis is left-handed).
    base: Mat4,
    /// Lifts the bind pose's lowest point onto the ground.
    ground: f32,
}

/// World units per model unit (the guardians come out about two and a half cells tall).
pub const MODEL_SCALE: f32 = 1.0;

impl Model3d {
    /// A model and its clips (the model's own first animation is the idle).
    pub fn new(model: &Gr2, clips: &[(Clip, Gr2)]) -> Self {
        let skeleton = model.skeletons.first();
        let bones: Vec<Bone> = skeleton.map(|s| s.bones.clone()).unwrap_or_default();
        let index_of = |name: &str| bones.iter().position(|b| b.name == name);
        let meshes = model
            .meshes
            .iter()
            .map(|m| {
                let remap: Vec<usize> = m.bone_names.iter().map(|n| index_of(n).unwrap_or(0)).collect();
                Mesh {
                    texture: m.texture,
                    positions: m.positions.iter().map(|&p| Vec3::from(p)).collect(),
                    normals: m.normals.iter().map(|&n| Vec3::from(n)).collect(),
                    uvs: m.uvs.clone(),
                    weights: m.bone_weights.clone(),
                    bones: m.bone_indices.iter().map(|b| b.map(|i| remap.get(i as usize).copied().unwrap_or(0))).collect(),
                    indices: m.indices.clone(),
                }
            })
            .collect();
        let anim = |a: &ro_formats::gr2::Animation| Anim {
            duration: a.duration,
            tracks: bones.iter().map(|b| a.tracks.iter().find(|t| t.bone == b.name).cloned()).collect(),
        };
        let mut slots: [Option<Anim>; 5] = Default::default();
        slots[Clip::Idle as usize] = model.animations.first().filter(|a| a.duration > 0.0).map(anim);
        for (clip, g) in clips {
            slots[*clip as usize] = g.animations.first().filter(|a| a.duration > 0.0).map(anim);
        }
        let ip = model.placement.map_or(Mat4::IDENTITY, |t| {
            Mat4::from_rotation_translation(Quat::from_array(t.rotation), Vec3::from(t.translation)) * Mat4::from_mat3(Mat3::from_cols_array(&t.scale_shear).transpose())
        });
        // Mirrored in x: the guardians then hold their swords in the right hand.
        let base = Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)) * Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2) * ip;
        let low = model.meshes.iter().flat_map(|m| &m.positions).map(|&p| base.transform_point3(Vec3::from(p)).y).fold(f32::MAX, f32::min);
        let ground = if low == f32::MAX { 0.0 } else { -low };
        Self { meshes, bones, clips: slots, base, ground }
    }

    /// Seconds one pass of a clip takes (its fallback's when it has none).
    pub fn duration(&self, clip: Clip) -> f32 {
        self.anim(clip).map_or(0.0, |a| a.duration)
    }

    pub fn has_clip(&self, clip: Clip) -> bool {
        self.clips[clip as usize].is_some()
    }

    fn anim(&self, clip: Clip) -> Option<&Anim> {
        self.clips[clip as usize].as_ref().or(self.clips[Clip::Idle as usize].as_ref())
    }

    /// The skinning matrices at `t` seconds into `clip` (looping, or held at its end).
    fn skin(&self, clip: Clip, t: f32, once: bool) -> Vec<Mat4> {
        let anim = self.anim(clip);
        let t = match anim {
            Some(a) if once => t.min(a.duration),
            Some(a) => t.rem_euclid(a.duration.max(1e-3)),
            None => 0.0,
        };
        let mut world: Vec<Mat4> = Vec::with_capacity(self.bones.len());
        for (i, bone) in self.bones.iter().enumerate() {
            let track = anim.and_then(|a| a.tracks.get(i)).and_then(|t| t.as_ref());
            let local = local_transform(bone, track, t);
            let w = match usize::try_from(bone.parent) {
                Ok(p) if p < i => world[p] * local,
                _ => local,
            };
            world.push(w);
        }
        world.iter().zip(&self.bones).map(|(w, b)| *w * Mat4::from_cols_array(&b.inverse_world)).collect()
    }

    /// The model posed at `t` seconds into `clip`, standing at `pos` (world, y-up) and turned
    /// `yaw` radians about the vertical (0 faces north, clockwise seen from above): triangles
    /// per model texture.
    pub fn pose(&self, clip: Clip, t: f32, once: bool, pos: Vec3, yaw: f32, scale: f32, tint: [f32; 3]) -> Vec<(usize, Vec<Vertex>)> {
        let skin = self.skin(clip, t, once);
        // Granny models are z-up; the world is y-up. The model's origin stands on `pos`.
        let place = Mat4::from_translation(pos)
            * Mat4::from_rotation_y(yaw)
            * Mat4::from_scale(Vec3::splat(MODEL_SCALE * scale))
            * Mat4::from_translation(Vec3::Y * self.ground)
            * self.base;
        let place_n = Mat3::from_mat4(place);
        let mut out: Vec<(usize, Vec<Vertex>)> = Vec::new();
        for m in &self.meshes {
            let Some(tex) = m.texture else { continue };
            let posed: Vec<(Vec3, Vec3)> = (0..m.positions.len())
                .map(|v| {
                    let w = m.weights[v];
                    let sum: f32 = w.iter().sum();
                    let (p, n) = (m.positions[v], m.normals[v]);
                    let (p, n) = if sum > 1e-4 {
                        let mut pp = Vec3::ZERO;
                        let mut nn = Vec3::ZERO;
                        for k in 0..4 {
                            if w[k] > 0.0 {
                                let s = skin.get(m.bones[v][k]).copied().unwrap_or(Mat4::IDENTITY);
                                pp += s.transform_point3(p) * w[k];
                                nn += s.transform_vector3(n) * w[k];
                            }
                        }
                        (pp / sum, nn)
                    } else {
                        (p, n)
                    };
                    (place.transform_point3(p), (place_n * n).normalize_or(Vec3::Y))
                })
                .collect();
            let slot = match out.iter().position(|(t, _)| *t == tex) {
                Some(i) => i,
                None => {
                    out.push((tex, Vec::new()));
                    out.len() - 1
                }
            };
            for &i in &m.indices {
                let i = i as usize;
                let Some(&(p, n)) = posed.get(i) else { continue };
                out[slot].1.push(Vertex { pos: p.into(), normal: n.into(), uv: m.uvs.get(i).copied().unwrap_or_default(), lightmap_uv: [0.5, 0.5], color: tint, sway: 0.0 });
            }
        }
        out
    }
}

fn local_transform(bone: &Bone, track: Option<&Track>, t: f32) -> Mat4 {
    let pos = track.and_then(|tr| sample(tr.position.as_ref()?, t)).map_or(Vec3::from(bone.translation), |v| Vec3::new(v[0], v[1], v[2]));
    let rot = track
        .and_then(|tr| sample(tr.orientation.as_ref()?, t))
        .filter(|v| v.len() == 4)
        .map_or(Quat::from_array(bone.rotation), |v| {
            // granny's quick renormalisation
            let k = (3.0 - (v[0] * v[0] + v[1] * v[1] + v[2] * v[2] + v[3] * v[3])) * 0.5;
            Quat::from_xyzw(v[0] * k, v[1] * k, v[2] * k, v[3] * k)
        });
    let ss = track
        .and_then(|tr| sample(tr.scale_shear.as_ref()?, t))
        .map(|v| match v.len() {
            9 => [v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8]],
            3 => [v[0], 0.0, 0.0, 0.0, v[1], 0.0, 0.0, 0.0, v[2]],
            _ => bone.scale_shear,
        })
        .unwrap_or(bone.scale_shear);
    // Row-major scale/shear.
    let ss = Mat3::from_cols_array(&ss).transpose();
    Mat4::from_rotation_translation(rot, pos) * Mat4::from_mat3(ss)
}

/// A curve's value at `t`: the control at the knot for degree 0, else the B-spline over the
/// knots around `t`.
fn sample(c: &Curve, t: f32) -> Option<Vec<f32>> {
    let (k, dim) = (&c.knots, c.dimension.max(1));
    if k.is_empty() || c.controls.is_empty() {
        return None;
    }
    let count = c.controls.len() / dim;
    let dim = if count == k.len() { dim } else { c.controls.len() / k.len().max(1) };
    if dim == 0 {
        return None;
    }
    let control = |i: usize| -> &[f32] {
        let i = i.min(c.controls.len() / dim - 1);
        &c.controls[i * dim..i * dim + dim]
    };
    let after = k.iter().position(|&x| x > t).unwrap_or(k.len() - 1);
    let degree = c.degree.max(0) as usize;
    if degree == 0 || k.len() == 1 {
        return Some(control(after).to_vec());
    }
    let s = after.min(k.len() - 1) as isize;
    let at = |e: isize| (s - degree as isize + e).clamp(0, k.len() as isize - 1) as usize;
    let div = |a: f32, b: f32| if b.abs() < 1e-12 { 0.0 } else { a / b };
    let out: Vec<f32> = if degree == 1 {
        let (u0, u1) = (k[at(0)], k[at(1)]);
        let r = div(t - u0, u1 - u0);
        let (a, b) = (control(at(0)), control(at(1)));
        (0..dim).map(|i| (1.0 - r) * a[i] + r * b[i]).collect()
    } else if degree == 2 {
        let (u0, u1, u2, u3) = (k[at(0)], k[at(1)], k[at(2)], k[at(3)]);
        let a = div(t - u1, u2 - u1);
        let o = div(t - u0, u2 - u0);
        let s2 = div(t - u1, u3 - u1);
        let cc = o + a - a * o;
        let l = a * s2;
        let w = [1.0 - cc, cc - l, l];
        let (p0, p1, p2) = (control(at(0)), control(at(1)), control(at(2)));
        (0..dim).map(|i| w[0] * p0[i] + w[1] * p1[i] + w[2] * p2[i]).collect()
    } else {
        control(at(degree as isize)).to_vec()
    };
    Some(out)
}
