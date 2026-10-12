use super::*;
use assets::{BlockFace, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

/// Compiles both campfire blocks with distinct unlit, lit and flame sprites per block.
fn compile_campfires() -> (RuntimeAssets, Vec<RegistryRecord>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let registry = assets::read_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
        protocol,
    )
    .unwrap();
    let records: Vec<_> = registry
        .into_vec()
        .into_iter()
        .filter(|r| {
            matches!(
                r.name.as_ref(),
                "minecraft:air"
                    | "minecraft:stone"
                    | "minecraft:campfire"
                    | "minecraft:soul_campfire"
            )
        })
        .collect();
    let directory = tempfile::tempdir().unwrap();
    let names = ["stone", "log", "lit", "fire", "soul_lit", "soul_fire"];
    for (index, name) in names.into_iter().enumerate() {
        let pixels: Vec<_> = (0..TILE_SIZE * TILE_SIZE)
            .flat_map(|_| [index as u8 * 40, 80, 120, 255])
            .collect();
        write_png(
            directory.path().join(format!("textures/{name}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &pixels,
        );
    }
    write(
        directory.path().join("blocks.json"),
        r#"{"stone":{"textures":"stone"},
            "campfire":{"textures":{"down":"log","side":"lit","up":"fire"}},
            "soul_campfire":{"textures":{"down":"log","side":"soul_lit","up":"soul_fire"}}}"#,
    );
    let texture_data: serde_json::Map<_, _> = names
        .into_iter()
        .map(|name| {
            (
                name.to_owned(),
                serde_json::json!({ "textures": format!("textures/{name}") }),
            )
        })
        .collect();
    write(
        directory.path().join("textures/terrain_texture.json"),
        &serde_json::json!({ "texture_data": texture_data }).to_string(),
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    let count = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &vec![assets::LightProperties::default(); count],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (
        RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap(),
        records,
    )
}

fn span(quads: &[assets::ModelQuad], axis: usize) -> [i16; 2] {
    let coordinates = quads.iter().flat_map(|q| q.positions.map(|p| p[axis]));
    [
        coordinates.clone().min().unwrap(),
        coordinates.max().unwrap(),
    ]
}

#[test]
fn campfires_draw_crossed_logs_and_their_own_flames_instead_of_an_envelope() {
    let (runtime, records) = compile_campfires();
    let mut seen = 0;
    let mut flames = Vec::new();
    for record in records.iter().filter(|r| r.name.contains("campfire")) {
        seen += 1;
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let lit = state["extinguished"]["value"] == 0;
        let along_x = matches!(
            state["minecraft:cardinal_direction"]["value"]
                .as_str()
                .unwrap(),
            "east" | "west"
        );
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(
            visual.kind(),
            VisualKind::Model,
            "{}",
            record.canonical_state
        );
        assert_eq!(visual.support(), VisualSupport::VanillaFallback);
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        let unlit = visual.face(BlockFace::Down).material_id();
        let embers = visual.face(BlockFace::South).material_id();
        let flame = visual.face(BlockFace::Up).material_id();

        // Logs and ash bed stay below 7 pixels and cover the full block footprint.
        let (boxes, planes) = quads.split_at(30);
        assert_eq!(span(boxes, 1), [0, 112]);
        assert_eq!(span(boxes, 0), [0, 256]);
        assert_eq!(span(boxes, 2), [0, 256]);
        // The lower logs run along the axis the direction selects.
        let lower = &boxes[..12];
        assert_eq!(span(lower, 1), [0, 64]);
        assert_eq!(span(lower, if along_x { 0 } else { 2 }), [0, 256]);
        assert_eq!(span(lower, if along_x { 2 } else { 0 }), [16, 240]);
        assert!(boxes.iter().any(|q| q.material == unlit));

        // Log tops, bottoms and the lower logs' outward sides stay plain bark.
        // While lit, both long sides of the upper logs, the lower log sides
        // facing the fire and the ash bed use the lit art.
        for log in boxes[..24].chunks_exact(6) {
            assert_eq!(log[BlockFace::Up as usize].material, unlit);
            assert_eq!(log[BlockFace::Down as usize].material, unlit);
        }
        let lit_faces = boxes.iter().filter(|q| q.material == embers).count();
        if lit {
            assert_eq!(lit_faces, 4 + 2 + 6);
            for face in boxes[..24].iter().filter(|q| q.material == embers) {
                let plane = (0..3)
                    .find(|&axis| {
                        face.positions
                            .iter()
                            .all(|p| p[axis] == face.positions[0][axis])
                    })
                    .unwrap();
                let bottom = face.positions.iter().map(|p| p[1]).min().unwrap();
                if bottom == 0 {
                    assert!(matches!(face.positions[0][plane], 80 | 176));
                }
                // The charred lower edge of the lit bark strip runs along the
                // bottom of the face.
                for (position, uv) in face.positions.iter().zip(face.uvs) {
                    assert_eq!(uv[1] == 4 * 256, position[1] == bottom);
                }
            }
            assert_eq!(planes.len(), 2);
            for plane in planes {
                assert_eq!(plane.material, flame);
                assert_ne!(plane.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED, 0);
            }
            assert_eq!(span(planes, 1), [16, 256]);
            flames.push((record.name.clone(), flame));
        } else {
            assert!(planes.is_empty());
            assert!(boxes.iter().all(|q| q.material == unlit));
        }
    }
    assert_eq!(seen, 16);
    let campfire = flames
        .iter()
        .find(|(name, _)| name.as_ref() == "minecraft:campfire");
    let soul = flames
        .iter()
        .find(|(name, _)| name.as_ref() == "minecraft:soul_campfire");
    assert_ne!(campfire.unwrap().1, soul.unwrap().1);
}
