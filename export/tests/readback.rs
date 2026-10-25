//! Writes Bumblebee and reads every glTF back with an independent reader (the robot, the vehicle,
//! the weapons, the missile, the axe): every mesh part, vertex and triangle of each object is
//! there, every joint resolves, and at rest each skin's inverse bind matrix undoes its joint's
//! place, as the game's drawing needs. Reads the install; skips without it.

mod common;

use glam::{Mat4, Quat, Vec3};
use tf2_core::formats::hash::lower33;

#[test]
fn bumblebee_every_file_reads_back_whole() {
    let Some(export) = common::export("bumblebee", "readback") else { return };
    let pieces = export.pieces();
    assert_eq!(pieces.len(), export.written.files.len());
    let mut joints = 0;
    let mut worst: f32 = 0.0;
    for (piece, (file, summary)) in pieces.iter().zip(&export.written.files) {
        assert_eq!(&format!("{}.glb", piece.file), file);
        // What the object has, counted from the source.
        let (mut parts, mut vertices, mut triangles) = (0, 0, 0);
        for render in &piece.object.renders {
            let mesh = &export.data.library.meshes[&lower33(&render.model)];
            for part in mesh.parts.iter().filter(|p| !p.positions.is_empty() && !p.indices.is_empty()) {
                parts += 1;
                vertices += part.positions.len();
                triangles += part.indices.len() / 3;
            }
        }
        assert_eq!((summary.parts, summary.vertices, summary.triangles), (parts, vertices, triangles), "{file}");
        // A sheared rest matrix is noted, not a fault (glTF cannot hold one; the port draws the same).
        assert!(summary.problems.iter().all(|p| p.contains("is sheared")), "{file}: {:?}", summary.problems);

        let glb = export.read(file);
        let doc = &glb.document;
        assert_eq!(doc.nodes().count(), summary.nodes, "{file}");
        let (mut read_vertices, mut read_triangles) = (0, 0);
        for mesh in doc.meshes() {
            for primitive in mesh.primitives() {
                let reader = primitive.reader(|_| Some(&glb.blob));
                let count = reader.read_positions().unwrap().count();
                read_vertices += count;
                read_triangles += reader.read_indices().unwrap().into_u32().count() / 3;
                if let Some(normals) = reader.read_normals() {
                    assert_eq!(normals.count(), count, "{file}");
                }
            }
        }
        assert_eq!((read_vertices, read_triangles), (vertices, triangles), "{file}");

        let parent = glb.parents();
        let root = glb.root();
        let local = |i: usize| {
            let (t, r, s) = doc.nodes().nth(i).unwrap().transform().decomposed();
            Mat4::from_scale_rotation_translation(Vec3::from(s), Quat::from_array(r), Vec3::from(t))
        };
        for skin in doc.skins() {
            let reader = skin.reader(|_| Some(&glb.blob));
            let inverse: Vec<[[f32; 4]; 4]> = reader.read_inverse_bind_matrices().unwrap().collect();
            assert_eq!(inverse.len(), skin.joints().count(), "{file}");
            for (joint, m) in skin.joints().zip(inverse) {
                if joint.index() == root {
                    continue;
                }
                joints += 1;
                let rest = glb.world(joint.index(), &parent, &local) * Mat4::from_cols_array_2d(&m);
                worst = worst.max((rest - Mat4::IDENTITY).to_cols_array().iter().fold(0.0f32, |a, v| a.max(v.abs())));
            }
        }
        println!("{file}: {parts} parts, {vertices} vertices, {} skins", doc.skins().count());
    }
    println!("{joints} joints; worst rest error {worst:.2e}");
    assert!(joints > 0);
    assert!(worst < 1e-3, "a joint's rest place and its inverse bind disagree by {worst}");
}
