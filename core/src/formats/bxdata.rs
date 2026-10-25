//! The PC port's own data archive, `Beenox\bxdata.dat`, and the BXML trees in it (`docs/input.md`).
//!
//! [game] archive directory (004f2de0): a u32 entry count, then per entry a u32 UTF-16 character count, the
//! UTF-16LE path, u32 size and u32 absolute offset. BXML (004f5010 / 004f4f70): the string `BXML`, then tokens:
//! 1 opens an element (name, u32 attribute count, name / value pairs), 2 adds text, 3 closes an element (its name),
//! 4 is ignored, 0 ends. Strings are a UTF-16LE byte-order mark, the characters and a 0 character.

use std::collections::HashMap;
use std::path::Path;

/// The archive: its bytes and each entry's (offset, size) by lower-case path (`beenox\refkeys\key_w.png`).
pub struct Archive {
    data: Vec<u8>,
    entries: HashMap<String, (usize, usize)>,
}

/// One BXML element.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Element {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<Element>,
}

impl Element {
    /// An attribute's value.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.attributes.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// Every element in the tree named `name`, this one included, in document order.
    pub fn all<'a>(&'a self, name: &str, out: &mut Vec<&'a Element>) {
        if self.name == name {
            out.push(self);
        }
        for c in &self.children {
            c.all(name, out);
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len()).ok_or_else(|| format!("read past the end at {:#x}", self.pos))?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn string(&mut self) -> Result<String, String> {
        if self.take(2)? != [0xff, 0xfe] {
            return Err(format!("no UTF-16 mark at {:#x}", self.pos - 2));
        }
        let mut units = Vec::new();
        loop {
            let b = self.take(2)?;
            let u = u16::from_le_bytes([b[0], b[1]]);
            if u == 0 {
                break;
            }
            units.push(u);
        }
        Ok(String::from_utf16_lossy(&units))
    }
}

/// Parses one BXML tree.
pub fn bxml(data: &[u8]) -> Result<Element, String> {
    let mut r = Reader { data, pos: 0 };
    if r.string()? != "BXML" {
        return Err("not BXML".into());
    }
    let mut stack: Vec<Element> = Vec::new();
    let mut root = None;
    while r.pos < data.len() {
        match r.u32()? {
            1 => {
                let name = r.string()?;
                let n = r.u32()?;
                let mut attributes = Vec::new();
                for _ in 0..n {
                    attributes.push((r.string()?, r.string()?));
                }
                stack.push(Element { name, attributes, ..Default::default() });
            }
            2 => {
                let t = r.string()?;
                stack.last_mut().ok_or("text outside the root")?.text.push_str(&t);
            }
            3 => {
                let name = r.string()?;
                let e = stack.pop().ok_or("a close without an open")?;
                if e.name != name {
                    return Err(format!("{name} closes {}", e.name));
                }
                match stack.last_mut() {
                    Some(parent) => parent.children.push(e),
                    None => root = Some(e),
                }
            }
            4 => {}
            0 => break,
            t => return Err(format!("unknown BXML token {t} at {:#x}", r.pos - 4)),
        }
    }
    root.ok_or_else(|| "incomplete BXML tree".into())
}

impl Archive {
    pub fn open(path: &Path) -> Result<Archive, String> {
        let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut r = Reader { data: &data, pos: 0 };
        let mut entries = HashMap::new();
        for _ in 0..r.u32()? {
            let n = r.u32()? as usize;
            let units: Vec<u16> = r.take(n * 2)?.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            let name = String::from_utf16_lossy(&units).to_lowercase();
            let (size, offset) = (r.u32()? as usize, r.u32()? as usize);
            if offset + size > data.len() {
                return Err(format!("{name} lies past the end"));
            }
            entries.insert(name, (offset, size));
        }
        Ok(Archive { data, entries })
    }

    /// The install's archive.
    pub fn install(game_dir: &Path) -> Result<Archive, String> {
        Archive::open(&game_dir.join("Beenox").join("bxdata.dat"))
    }

    /// An entry's bytes by path (any case, `\` separators).
    pub fn file(&self, path: &str) -> Option<&[u8]> {
        self.entries.get(&path.to_lowercase()).map(|&(o, n)| &self.data[o..o + n])
    }

    /// An entry parsed as BXML.
    pub fn tree(&self, path: &str) -> Result<Element, String> {
        bxml(self.file(path).ok_or_else(|| format!("{path} is not in the archive"))?)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_install_archive_and_its_bindings_read() {
        // Needs the install. [data] 444 entries; the keyboard's Robot map binds RTrigger to Key_LSHIFT.
        let Ok(a) = Archive::install(crate::install::dir()) else { return };
        assert_eq!(a.len(), 444);
        let tree = a.tree(r"beenox\default_bindings.bin").unwrap();
        let map = tree.children.iter().find(|m| m.get("device") == Some("Keyboard") && m.get("context") == Some("Robot")).unwrap();
        let rt = map.children.iter().find(|b| b.get("action") == Some("RTrigger")).unwrap();
        assert_eq!(rt.get("primary"), Some("Key_LSHIFT"));
        let mut refs = Vec::new();
        let keys = a.tree(r"beenox\keyreference_eng.bin").unwrap();
        keys.all("BXKeyReference", &mut refs);
        assert!(refs.iter().any(|r| r.get("name") == Some("Mouse_Button2") && r.get("ref") == Some("Mouse_Button2.png")));
    }
}
