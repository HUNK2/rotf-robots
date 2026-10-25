#![allow(dead_code)] // Each test uses part of it.

//! What the export tests share: one character written to a folder of its own (removed after),
//! with the source it was written from.

use std::collections::HashMap;
use std::path::PathBuf;

use glam::{Mat4, Quat, Vec3};
use tf2_core::character::CharacterData;
use tf2_core::formats::characterlist::Spec;

pub struct Export {
    pub spec: Spec,
    pub data: CharacterData,
    pub names: rotf_export::names::Names,
    pub out: PathBuf,
    pub written: rotf_export::Written,
}

/// The character written under the temp folder (`ROTF_TEST_CHARACTER` names another than
/// `name`); None without the install (the test skips).
pub fn export(name: &str, tag: &str) -> Option<Export> {
    if tf2_core::install::missing() {
        return None;
    }
    let install = tf2_core::install::dir();
    let roster = tf2_core::formats::characterlist::CharacterList::load(install).unwrap();
    let name = std::env::var("ROTF_TEST_CHARACTER").unwrap_or_else(|_| name.to_owned());
    let spec = rotf_export::spec(&roster, &name);
    let out = std::env::temp_dir().join(format!("rotf-export-{tag}-{}", std::process::id()));
    let written = rotf_export::export(install, &spec, &out).unwrap();
    let data = tf2_core::character::load(install, &spec.pack).unwrap();
    let names = rotf_export::names::Names::load(install, rotf_export::names::data_files(&spec.pack_file(install)), &[]);
    Some(Export { spec, data, names, out, written })
}

impl Export {
    /// The pieces the export wrote, in its order.
    pub fn pieces(&self) -> Vec<rotf_export::Piece<'_>> {
        rotf_export::pieces(&self.data, &self.spec.pack, &self.names)
    }

    pub fn read(&self, file: &str) -> Glb {
        let bytes = std::fs::read(self.out.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        let gltf = gltf::Gltf::from_slice(&bytes).unwrap();
        Glb { blob: gltf.blob.clone().unwrap(), document: gltf.document }
    }
}

impl Drop for Export {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.out);
    }
}

pub struct Glb {
    pub blob: Vec<u8>,
    pub document: gltf::Document,
}

impl Glb {
    /// Each node's parent.
    pub fn parents(&self) -> HashMap<usize, usize> {
        let mut parent = HashMap::new();
        for node in self.document.nodes() {
            for child in node.children() {
                parent.insert(child.index(), node.index());
            }
        }
        parent
    }

    pub fn root(&self) -> usize {
        self.document.scenes().next().unwrap().nodes().next().unwrap().index()
    }

    /// Each node's rest place, turn and scale.
    pub fn rest(&self) -> Vec<(Vec3, Quat, Vec3)> {
        self.document
            .nodes()
            .map(|node| {
                let (t, r, s) = node.transform().decomposed();
                (Vec3::from(t), Quat::from_array(r), Vec3::from(s))
            })
            .collect()
    }

    /// A node's world matrix in the game's space (the chain under the root, without the root's
    /// turn into glTF's space), from each node's local matrix.
    pub fn world(&self, mut i: usize, parent: &HashMap<usize, usize>, local: &dyn Fn(usize) -> Mat4) -> Mat4 {
        let root = self.root();
        let mut m = Mat4::IDENTITY;
        while i != root {
            m = local(i) * m;
            i = parent[&i];
        }
        m
    }
}
