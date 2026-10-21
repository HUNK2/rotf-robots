use tf2_core::{character, formats::hash::lower33};

fn main() {
    use tf2_core::formats::{pack::{self, Pack}, lxb::DataFile};
    let pack = Pack::open(std::path::Path::new("C:/Games2/characters/optimus.str")).unwrap();
    let chunk = pack.of_type(pack::DATA).next().unwrap();
    let file = DataFile::parse(pack.data(chunk).to_vec()).unwrap();
    for (_, root) in file.root().fields() {
        for object in root.get("things").into_iter().flat_map(|n| n.items()).filter(|n| n.type_name() == Some("Object")) {
            for attachment in object.get("attachments").into_iter().flat_map(|n| n.items()) {
                let Some(thing) = attachment.get("thing").filter(|n| n.type_name() == Some("RenderThing")) else { continue };
                let names: Vec<_> = thing.get("names").into_iter().flat_map(|n| n.items())
                    .map(|n| n.text().unwrap_or_else(|| n.int().map(|h| format!("{:08x}:{}",h as u32,file.name(h as u32).unwrap_or("?"))).unwrap_or_else(|| format!("{}",n.type_name().unwrap_or("untyped")))))
                    .collect();
                let models: Vec<_> = thing.get("meshes").into_iter().flat_map(|n| n.items()).filter_map(|n| n.get("model").and_then(|n| n.text())).collect();
                if models.iter().any(|m| m == "Optimus.Base_SFX") {
                    println!("SFX attachment fields:");
                    dump(attachment, 0);
                }
                println!("GROUP {:08x} {:?}: {:?}", thing.label().unwrap_or(0), names, models);
            }
            for thing in object.get("attachments").into_iter().flat_map(|n| n.items()).filter_map(|n| n.get("thing"))
                .filter(|n| n.type_name() == Some("RenderSetEnabler")) {
                for set in thing.get("sets").into_iter().flat_map(|n| n.items()) {
                    let name = set.get("name").and_then(|n| n.int()).unwrap_or(0) as u32;
                    let labels: Vec<_> = set.get("nodes").into_iter().flat_map(|n| n.items())
                        .map(|n| format!("{:08x}:{}", n.label().unwrap_or(0), file.name(n.label().unwrap_or(0)).unwrap_or("?"))).collect();
                    println!("SET {name:08x} {:?}: {:?}",file.name(name),labels);
                }
            }
        }
    }
    let data = character::load(std::path::Path::new("C:/Games2"), "optimus").unwrap();
    if let Some(vehicle) = &data.vehicle {
        for group in &vehicle.render_sets {
            println!("VEHICLE sets: {:?}",group.sets.iter().map(|s| (s.name,s.nodes.len())).collect::<Vec<_>>());
        }
    }
    for render in &data.robot.renders {
        let Some(mesh) = data.library.meshes.get(&lower33(&render.model)) else { continue };
        println!("render {} node {} {}", render.model, render.node, data.robot.nodes[render.node].name);
        for part in &mesh.parts {
            if let Some(mat) = part.material.and_then(|id| data.library.materials.get(&id)) {
                println!("  {} tech {:08x} blend {:?} diffuse {:?} edge {:?} reflect {:?}", mat.name, mat.technique,
                    mat.word("blendmode"), mat.vector("diffuse_color"), mat.vector("edge_color"), mat.vector("reflection_color"));
            }
        }
    }
}

fn dump(node: tf2_core::formats::lxb::Node, depth: usize) {
    if depth > 4 { return; }
    for (h,n) in node.fields() {
        println!("{}{}: type {:?} int {:?} text {:?} len {}", "  ".repeat(depth),node.file().name(h).unwrap_or("?"), n.type_name(),n.int(),n.text(),n.len());
        if n.text().is_none() {dump(n,depth+1);}
    }
    for n in node.items().take(30) {
        println!("{}item: type {:?} int {:?} text {:?}","  ".repeat(depth),n.type_name(),n.int(),n.text());
        dump(n,depth+1);
    }
}
