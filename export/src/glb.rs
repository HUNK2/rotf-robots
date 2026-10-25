//! A minimal binary glTF 2.0 writer: one buffer, one accessor per buffer view, the document's
//! arrays kept as JSON values. Only what a robot needs.

use std::path::Path;

use serde_json::{Value, json};

pub const ARRAY_BUFFER: u32 = 34962;
pub const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const FLOAT: u32 = 5126;
const UNSIGNED_SHORT: u32 = 5123;

#[derive(Default)]
pub struct Glb {
    bin: Vec<u8>,
    views: Vec<Value>,
    pub accessors: Vec<Value>,
    pub nodes: Vec<Value>,
    pub meshes: Vec<Value>,
    pub skins: Vec<Value>,
    pub materials: Vec<Value>,
    pub textures: Vec<Value>,
    pub images: Vec<Value>,
    pub animations: Vec<Value>,
    pub extensions_used: Vec<&'static str>,
}

impl Glb {
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let mut view = json!({ "buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len() });
        if let Some(target) = target {
            view["target"] = json!(target);
        }
        self.bin.extend_from_slice(bytes);
        self.views.push(view);
        self.views.len() - 1
    }

    /// Floats in groups of `width` (`kind` "SCALAR", "VEC2" ... "MAT4"). `bounds` adds the min and
    /// max glTF requires for positions.
    pub fn floats(&mut self, data: &[f32], width: usize, kind: &str, target: Option<u32>, bounds: bool) -> usize {
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&bytes, target);
        let mut accessor = json!({ "bufferView": view, "componentType": FLOAT, "count": data.len() / width, "type": kind });
        if bounds {
            let (mut min, mut max) = (vec![f32::MAX; width], vec![f32::MIN; width]);
            for group in data.chunks(width) {
                for (k, &v) in group.iter().enumerate() {
                    min[k] = min[k].min(v);
                    max[k] = max[k].max(v);
                }
            }
            accessor["min"] = json!(min);
            accessor["max"] = json!(max);
        }
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    pub fn shorts(&mut self, data: &[u16], width: usize, kind: &str, target: Option<u32>) -> usize {
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&bytes, target);
        self.accessors.push(json!({ "bufferView": view, "componentType": UNSIGNED_SHORT, "count": data.len() / width, "type": kind }));
        self.accessors.len() - 1
    }

    pub fn node(&mut self, node: Value) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    /// Writes the `.glb`: the JSON chunk, then the binary one, each padded to four bytes.
    pub fn write(self, path: &Path, root: usize) -> std::io::Result<()> {
        let mut doc = json!({
            "asset": { "version": "2.0", "generator": "rotf-export" },
            "scene": 0,
            "scenes": [{ "nodes": [root] }],
            "buffers": [{ "byteLength": self.bin.len() }],
            "bufferViews": self.views,
            "accessors": self.accessors,
            "nodes": self.nodes,
            "meshes": self.meshes,
            "samplers": [{}],
        });
        for (key, list) in [
            ("skins", self.skins),
            ("materials", self.materials),
            ("textures", self.textures),
            ("images", self.images),
            ("animations", self.animations),
        ] {
            if !list.is_empty() {
                doc[key] = Value::Array(list);
            }
        }
        if !self.extensions_used.is_empty() {
            doc["extensionsUsed"] = json!(self.extensions_used);
        }
        let mut text = serde_json::to_vec(&doc).map_err(std::io::Error::other)?;
        while !text.len().is_multiple_of(4) {
            text.push(b' ');
        }
        let mut bin = self.bin;
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let total = 12 + 8 + text.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&text);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        std::fs::write(path, out)
    }
}
