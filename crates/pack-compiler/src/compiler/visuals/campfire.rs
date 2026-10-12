//! Campfire and soul campfire logs, ash bed and crossed flames.
//!
//! The pack carries campfire art but no geometry. Boxes follow the unlit log
//! art's layout: bark in rows 0-3, the 4x4 log end below it at rows 4-7, and
//! the ash bed in rows 8-15. The lit art keeps the ash bed rows and chars the
//! bottom of its bark strip. Lit log sides use that strip: both long sides of
//! the upper logs, so the charring shows beneath them, and the fire-facing
//! sides of the lower logs.
//! The exact native box constants still need measurement.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use {super::super::*, assets::BlockFace};

/// Bark strip along a log, in sprite pixels.
const BARK: [u16; 4] = [0, 0, 16, 4];
/// Log end grain, only present in the unlit art.
const LOG_END: [u16; 4] = [0, 4, 4, 8];
/// Lit bark charred along its lower edge, for lit log sides.
const CHARRED_EDGE: [u16; 4] = [0, 0, 16, 4];
/// Ash bed top, with the bed's long axis along the sprite's width.
const ASH_TOP: [u16; 4] = [0, 8, 16, 14];
/// Ash bed sides and ends.
const ASH_SIDE: [u16; 4] = [0, 8, 16, 9];
const ASH_END: [u16; 4] = [0, 8, 6, 9];

/// Where a box face sits, which selects its sprite region.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    /// Across the box's long axis.
    End,
    /// Top or bottom along the long axis.
    Horizontal,
    /// Vertical side along the long axis, facing the block centre.
    Inner,
    /// Vertical side along the long axis, facing away from the block centre.
    Outer,
}

/// Identifies both campfire blocks before their provisional envelope route.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    matches!(
        record.name.as_ref(),
        "minecraft:campfire" | "minecraft:soul_campfire"
    ) && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

/// Admits the typed extinguished bit and cardinal direction, returning
/// `(extinguished, rotated)` where `rotated` lays the lower logs along X.
fn pose(record: &RegistryRecord) -> Option<(bool, bool)> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 2 {
        return None;
    }
    let extinguished = super::state::exact_tagged_byte(state.get("extinguished")?, 1)? != 0;
    let rotated =
        match super::state::exact_tagged_string(state.get("minecraft:cardinal_direction")?)? {
            "north" | "south" => false,
            "east" | "west" => true,
            _ => return None,
        };
    Some((extinguished, rotated))
}

/// Compiles every campfire pose with the pack's unlit log, lit log and flame materials.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 6], bool, bool), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some((extinguished, rotated)) = pose(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let mut visual = diagnostic_visual(record);
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let key = (materials, extinguished, rotated);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template = push_model_template(
            quads(materials, extinguished, rotated),
            0,
            storage.templates,
            storage.quads,
        )?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, materials, template);
    visual.support = VisualSupport::VanillaFallback;
    Ok(CompileRuleResult::Compiled(visual))
}

/// Emits two lower logs, two upper crossing logs and the ash bed, then two
/// double-sided diagonal flame planes while lit.
fn quads(materials: [u32; 6], extinguished: bool, rotated: bool) -> Vec<ModelQuad> {
    let unlit = materials[BlockFace::Down as usize];
    let lit = if extinguished {
        unlit
    } else {
        materials[BlockFace::South as usize]
    };
    // Each box: min, max, long axis in pixels before the direction swap, and
    // whether its outward side is lit. Upper logs char on both long sides, so
    // their charred edge shows beneath them; lower logs only face the fire.
    let logs = [
        ([1, 0, 0], [5, 4, 16], 2, false),
        ([11, 0, 0], [15, 4, 16], 2, false),
        ([0, 3, 1], [16, 7, 5], 0, true),
        ([0, 3, 11], [16, 7, 15], 0, true),
    ];
    let mut result = Vec::with_capacity(32);
    for (min, max, long, lit_outside) in logs {
        result.extend(textured_box(min, max, long, rotated, |role| match role {
            Role::End => (unlit, LOG_END),
            Role::Inner if !extinguished => (lit, CHARRED_EDGE),
            Role::Outer if lit_outside && !extinguished => (lit, CHARRED_EDGE),
            _ => (unlit, BARK),
        }));
    }
    result.extend(textured_box(
        [5, 0, 0],
        [11, 1, 16],
        2,
        rotated,
        |role| match role {
            Role::Horizontal => (lit, ASH_TOP),
            Role::End => (lit, ASH_END),
            Role::Inner | Role::Outer => (lit, ASH_SIDE),
        },
    ));
    if !extinguished {
        let flame = materials[BlockFace::Up as usize];
        // Diagonal planes from 1 to 16 pixels tall, inset 0.8 pixels from the corners.
        let (low, high, bottom, top) = (13, 243, 16, 256);
        for (start, end) in [([low, low], [high, high]), ([high, low], [low, high])] {
            result.push(ModelQuad {
                positions: [
                    [start[0], bottom, start[1]],
                    [end[0], bottom, end[1]],
                    [end[0], top, end[1]],
                    [start[0], top, start[1]],
                ],
                uvs: [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
                material: flame,
                flags: MODEL_QUAD_FLAG_TWO_SIDED,
            });
        }
    }
    result
}

/// Builds one box in pixel bounds, swapping X and Z when `rotated`. `face`
/// receives each face's role and returns its material and sprite rectangle.
fn textured_box(
    mut min: [i16; 3],
    mut max: [i16; 3],
    mut long: usize,
    rotated: bool,
    face: impl Fn(Role) -> (u32, [u16; 4]),
) -> [ModelQuad; 6] {
    if rotated {
        min.swap(0, 2);
        max.swap(0, 2);
        long = 2 - long;
    }
    let mut quads = super::geometry::cuboid_quads([0; 6], min.map(|p| p * 16), max.map(|p| p * 16));
    for (quad, block_face) in quads.iter_mut().zip(BlockFace::ALL) {
        let normal = normal_axis(block_face);
        let role = if normal == long {
            Role::End
        } else if normal == 1 {
            Role::Horizontal
        } else if matches!(block_face, BlockFace::East | BlockFace::South) == (max[normal] <= 8)
            && (min[normal] >= 8 || max[normal] <= 8)
        {
            Role::Inner
        } else {
            Role::Outer
        };
        let (material, rect) = face(role);
        // Sprite width runs along the long axis on side faces and across the
        // horizontal axis on end faces; sprite V runs downward on vertical faces.
        let (u_axis, v_axis) = if normal == long {
            (2 - long, 1)
        } else if normal == 1 {
            (long, 2 - long)
        } else {
            (long, 1)
        };
        for (uv, position) in quad.uvs.iter_mut().zip(quad.positions) {
            let at = |axis: usize| u16::from(position[axis] == max[axis] * 16);
            let u = at(u_axis);
            let v = if v_axis == 1 { 1 - at(1) } else { at(v_axis) };
            *uv = [
                (rect[0] + u * (rect[2] - rect[0])) * 256,
                (rect[1] + v * (rect[3] - rect[1])) * 256,
            ];
        }
        quad.material = material;
    }
    quads
}

fn normal_axis(face: BlockFace) -> usize {
    match face {
        BlockFace::West | BlockFace::East => 0,
        BlockFace::Down | BlockFace::Up => 1,
        BlockFace::North | BlockFace::South => 2,
    }
}
