"""Check the add-on against an exported robot, without a window:

    blender -b --factory-startup --python blender/check_import.py -- exported/<pack>/<pack>.json

It imports the robot twice (with and without holding the vehicle's unkeyed nodes) and checks what
the add-on promises against the JSON. Prints one line per check and exits non-zero on a failure.
"""

import os
import sys

import bpy

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rotf_import as addon  # noqa: E402

failures = []


def check(ok, text):
    print(("ok    " if ok else "FAIL  ") + text)
    if not ok:
        failures.append(text)


def fresh():
    try:
        addon.unregister()
    except (RuntimeError, ValueError):
        pass
    bpy.ops.wm.read_factory_settings(use_empty=True)
    addon.register()


def import_robot(path, hold):
    return addon.import_robot(
        bpy.context, path, "ALL", True, True, hold, True, lambda kind, text: print(f"  {kind}: {text}")
    )


def unkeyed_counts(pack_col, vehicle):
    piece = addon.pieces_of(pack_col)[vehicle]
    out = {}
    for name, action in piece.actions.items():
        out[name] = sum(1 for n in piece.nodes if piece.target(n) and not addon.curves(action, *piece.target(n)))
    return piece, out


def main(path):
    data = addon.load_robot_json(path)
    pack = data["character"]["pack"]
    vehicle = (data.get("vehicle") or {}).get("file")
    clips = {(c["file"], c["name"]): c for c in data["clips"]}

    fresh()
    for line in import_robot(path, hold=False):
        print("  " + line)
    pack_col = bpy.data.collections[pack]
    check(bpy.context.scene.render.fps == 60, "the scene runs at 60 fps")

    # The clips' unkeyed nodes are among the nodes the glb leaves unanimated (which also has the
    # nodes whose channel keys nothing).
    if vehicle:
        piece = addon.pieces_of(pack_col)[vehicle]
        wrong = []
        for name, action in piece.actions.items():
            loose = {n for n in piece.nodes if piece.target(n) and not addon.curves(action, *piece.target(n))}
            listed = set(clips[(vehicle, name)].get("unkeyed_node_names", []))
            if len(listed) != clips[(vehicle, name)].get("unkeyed_nodes", 0) or not listed <= loose:
                wrong.append((name, sorted(listed - loose)[:3]))
        check(not wrong, f"each vehicle clip's unkeyed_node_names are unanimated in the glb {wrong[:3]}")

    # Hidden meshes.
    hidden = [o for o in addon.pack_objects(pack_col) if o.get("render_set_hidden")]
    check(all(o.hide_render and o.hide_get() for o in hidden), f"{len(hidden)} render-set meshes start hidden")

    # Actions: numbers and markers.
    actions = [a for a in bpy.data.actions if a.get("rotf_pack") == pack]
    with_anim = [c for c in data["clips"] if c.get("animation") and c["file"] in {f["file"] for f in data["files"]}]
    check(len(actions) == len(with_anim), f"one action per clip with an animation ({len(actions)} of {len(with_anim)})")
    bad = [
        a.name
        for a in actions
        if len(a.pose_markers) != len(clips[(a["rotf_file"], a["rotf_clip"])]["events"])
        or a.frame_end != max(clips[(a["rotf_file"], a["rotf_clip"])]["length_ticks"], 1.0)
    ]
    check(not bad, f"markers and frame ranges on every action {bad[:3]}")

    # Collision shapes.
    bodies = sum(len(e["bodies"]) for e in data["collision"]["files"])
    shapes = [o for o in bpy.data.objects if o.type == "MESH" and "rotf_collision" in o]
    check(len(shapes) == bodies, f"a shape for each collision body ({len(shapes)} of {bodies})")

    # Put everything at rest by the add-on's own rest (taking the actions off leaves the last pose)
    # and show every collection (a hidden one is not evaluated). Then every node stands where the
    # glb's node chain puts it under its file's root.
    addon.clear_play(pack_col)
    for c in pack_col.children_recursive:
        c.hide_viewport = False
    rests, roots = {}, {}
    for f in data["files"]:
        gltf = addon.read_glb_json(os.path.join(os.path.dirname(path), f["file"]))
        top = gltf["scenes"][0]["nodes"][0]
        rests[f["file"]] = addon.rest_in_root(gltf, top)
        roots[f["file"]] = gltf["nodes"][top]["name"]
        objects = [o for o in addon.pack_objects(pack_col) if o["rotf_file"] == f["file"]]
        bases = addon.rest_bases(objects, rests[f["file"]])
        for o in objects:
            if o.type == "ARMATURE":
                for pb in o.pose.bones:
                    pb.matrix_basis = addon.Matrix.Identity(4)
            hung_root = o.parent is not None and o.parent.get("rotf_file") != o["rotf_file"]
            if o["rotf_node"] in bases and not hung_root:
                trs = bases[o["rotf_node"]]
                o.rotation_mode = "QUATERNION"
                o.location, o.rotation_quaternion, o.scale = trs["location"], trs["rotation_quaternion"], trs["scale"]
    bpy.context.view_layer.update()
    worst, compared = 0.0, 0
    root_of = {o["rotf_file"]: o for o in addon.pack_objects(pack_col) if o["rotf_node"] == roots[o["rotf_file"]]}
    for o in addon.pack_objects(pack_col):
        root = root_of[o["rotf_file"]]
        if o is root:
            continue
        want = root.matrix_world @ rests[o["rotf_file"]][o["rotf_node"]]
        worst = max(worst, max(abs(a - b) for ra, rb in zip(o.matrix_world, want) for a, b in zip(ra, rb)))
        compared += 1
    check(compared > 0 and worst < 1e-3, f"at rest every node is where the glb puts it ({compared} nodes, worst {worst:.2e})")

    # Weapons hang: each hung root is where the glb's own node chain puts the `.frame`.
    worst = 0.0
    hung = 0
    seen = set()
    for w in data["weapons"] + data["melee_weapons"]:
        if w.get("file") in seen:
            continue  # weapons of one list share a model: it hangs where the first one does
        seen.add(w.get("file"))
        root = next(
            (o for o in addon.pack_objects(pack_col) if o["rotf_file"] == w.get("file") and o.parent and o.parent.get("rotf_file") != o["rotf_file"]),
            None,
        )
        if root is None:
            continue
        host = root.parent
        gltf = addon.read_glb_json(os.path.join(os.path.dirname(path), host["rotf_file"]))
        rest = addon.rest_in_root(gltf, gltf["scenes"][0]["nodes"][0])
        top = host
        while top.parent:
            top = top.parent
        want = top.matrix_world @ rest[f"{w['start_node']}.frame"]
        worst = max(worst, max(abs(a - b) for ra, rb in zip(root.matrix_world, want) for a, b in zip(ra, rb)))
        hung += 1
    hangable = {w["file"] for w in data["weapons"] + data["melee_weapons"] if w.get("file") and w.get("hangs_on")}
    check(
        hung == len(hangable) and worst < 1e-4,
        f"{hung} of {len(hangable)} weapon models hang on their start node's .frame (worst {worst:.2e})",
    )

    if not vehicle:
        return
    # Holding: every vehicle clip keys every node, and a held node ends its lead-in where the clip
    # starts it.
    fresh()
    import_robot(path, hold=True)
    pack_col = bpy.data.collections[pack]
    piece, counts = unkeyed_counts(pack_col, vehicle)
    check(all(n == 0 for n in counts.values()), "after holding, every vehicle clip keys every node")
    lead_ins = addon.vehicle_lead_ins(data["clips"], vehicle)
    gaps = []
    for name, action in piece.actions.items():
        for lead, cut in lead_ins.get(name, [])[:1]:
            for node in action.get("rotf_held_nodes", []):
                obj, bone = piece.target(node)
                ours = addon.curves(action, obj, bone)["rotation_quaternion"]
                theirs = addon.curves(piece.actions[lead], obj, bone).get("rotation_quaternion")
                if not theirs:
                    continue
                frame = clips[(vehicle, lead)]["length_ticks"] if cut is None else cut
                q0 = addon.Quaternion([c.evaluate(0.0) for c in ours])
                q1 = addon.Quaternion([c.evaluate(frame) for c in theirs])
                gaps.append((q0.rotation_difference(q1).angle, name, node))
    if gaps:
        worst = max(gaps)
        print(f"  {len(gaps)} held nodes checked against their first lead-in; worst {worst[0]:.3f} rad ({worst[1]} {worst[2]})")
    for name, action in sorted(piece.actions.items()):
        if action.get("rotf_held_nodes") or action.get("rotf_rest_nodes"):
            print(f"  {name}: held {len(action['rotf_held_nodes'])}, at rest {len(action['rotf_rest_nodes'])}")

    # Playing a change of form shows the vehicle and hides the robot at the model events.
    change = next((c for c in data["clips"] if any(e["type"] == "model" and e.get("vehicle_then") for e in c["events"])), None)
    if change:
        problem = addon.play_clip(bpy.context, pack_col, addon.clip_label(change, pack))
        check(problem is None, f"plays {change['name']}")
        scene = bpy.context.scene
        last = [e for e in change["events"] if e["type"] == "model"][-1]
        scene.frame_set(int(round(last["tick"])))
        cols = {c.get("rotf_kind"): c for c in pack_col.children}
        check(
            cols["robot"].hide_viewport != last["show_robot"] and cols["vehicle"].hide_viewport != last["show_vehicle"],
            f"at tick {last['tick']:g} the robot is {'shown' if last['show_robot'] else 'hidden'}, the vehicle {'shown' if last['show_vehicle'] else 'hidden'}",
        )


if __name__ == "__main__":
    main(sys.argv[sys.argv.index("--") + 1])
    print("FAILED: " + "; ".join(failures) if failures else "all checks passed")
    sys.stdout.flush()
    os._exit(1 if failures else 0)
