use tf2_core::{character, special_types::{NORMAL_MESH, SPECIAL_ATTACK_MESH}};

#[test]
fn optimus_pack_switches_complete_normal_and_blue_mesh_sets() {
    let dir = std::path::PathBuf::from(std::env::var("TF2_GAME_DIR").unwrap_or_else(|_| "C:/Games2".into()));
    if !dir.join("characters/optimus.str").exists() { return; }
    let data = character::load(&dir, "optimus").unwrap();
    let group = data.robot.render_sets.iter().find(|g| g.sets.iter().any(|s| s.name == SPECIAL_ATTACK_MESH)).unwrap();
    assert!(group.exclusive);
    assert_eq!(group.sets[0].name, NORMAL_MESH);
    assert_eq!(group.sets[0].nodes.len(), 45);
    assert_eq!(group.sets[1].name, SPECIAL_ATTACK_MESH);
    assert_eq!(group.sets[1].nodes.len(), 45);
    for (i, set) in group.sets.iter().enumerate() {
        let members: Vec<_> = data.robot.renders.iter().filter(|r| group.contains_model(&r.model, i)).collect();
        assert_eq!(members.len(), set.nodes.len(), "every named node matches one render");
        for member in members {
            assert!(group.model_visible(&member.model, set.name));
            assert!(!group.model_visible(&member.model, group.sets[1 - i].name));
        }
    }
    assert!(group.model_visible("Optimus.EyeGlow_L", NORMAL_MESH));
    assert!(group.model_visible("Optimus.EyeGlow_L", SPECIAL_ATTACK_MESH));
    let vehicle = data.vehicle.as_ref().unwrap();
    let group = vehicle.render_sets.iter().find(|g| g.sets.iter().any(|s| s.name == SPECIAL_ATTACK_MESH)).unwrap();
    assert!(group.exclusive);
    assert_eq!(group.sets[0].name, NORMAL_MESH);
    for (i, set) in group.sets.iter().enumerate() {
        assert_eq!(set.nodes.len(), 50);
        let members: Vec<_> = vehicle.renders.iter().filter(|r| group.contains_model(&r.model, i)).collect();
        assert_eq!(members.len(), 50, "vehicle node links match the rendered parts");
        for member in members {
            assert!(group.model_visible(&member.model, set.name));
            assert!(!group.model_visible(&member.model, group.sets[1 - i].name));
        }
    }
}
