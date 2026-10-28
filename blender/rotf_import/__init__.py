"""Import a robot written by rotf-export into Blender, ready to animate.

File > Import > RotF robot (.json) reads `<pack>.json` and imports the robot's glTF files with
what the JSON says and glTF cannot (docs/FORMAT.md):

- the scene runs at 60 fps, so a frame is a clip tick, and the clips start at frame 0;
- the meshes the game starts hidden (`render_set_hidden`) are hidden;
- each action carries its clip's numbers (looping, speed, crossfade...) as custom properties, its
  frame range, and its events as pose markers;
- a vehicle clip's unkeyed nodes are keyed with the pose the clip before it leaves them in, as
  the game plays it (glTF would put them back at rest);
- the weapons hang on their start nodes;
- the collision bodies get wire shapes.

The RotF tab of the 3D view's sidebar plays a clip: on a change of form it runs the vehicle's
clips with it and shows and hides the robot and the vehicle at the clip's `model` events.
"""

import json
import math
import os
import re
import struct
from collections import defaultdict

import bmesh
import bpy
from bpy.app.handlers import persistent
from bpy.props import BoolProperty, EnumProperty, StringProperty
from bpy_extras import anim_utils
from bpy_extras.io_utils import ImportHelper
from mathutils import Matrix, Quaternion

bl_info = {
    "name": "RotF robot import",
    "author": "HUNK2",
    "version": (0, 2, 0),
    "blender": (4, 4, 0),
    "location": "File > Import > RotF robot (.json); 3D view sidebar > RotF",
    "description": "Import a robot written by rotf-export, ready to animate",
    "category": "Import-Export",
}

FORMAT_VERSION = 1
TICKS_PER_SECOND = 60
SUFFIX = re.compile(r"\.\d{3}$")
OBJECT_PATHS = ("location", "rotation_quaternion", "scale")
REST = {"location": (0.0, 0.0, 0.0), "rotation_quaternion": (1.0, 0.0, 0.0, 0.0), "scale": (1.0, 1.0, 1.0)}
# Files that are thrown or fired, not held: hidden after import.
PROJECTILE_KINDS = {"missile", "grenade", "turret"}


# --- reading -----------------------------------------------------------------------------------


def read_glb_json(path):
    with open(path, "rb") as f:
        magic, _version, _length = struct.unpack("<III", f.read(12))
        if magic != 0x46546C67:
            raise ValueError(f"{path} is not a .glb")
        length, kind = struct.unpack("<II", f.read(8))
        if kind != 0x4E4F534A:
            raise ValueError(f"{path}: the first chunk is not JSON")
        return json.loads(f.read(length))


_json_cache = {}


def load_robot_json(path):
    stamp = os.path.getmtime(path)
    cached = _json_cache.get(path)
    if cached and cached[0] == stamp:
        return cached[1]
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    _json_cache[path] = (stamp, data)
    return data


def stem(file_name, pack):
    """`bumblebee_weapon_SolarPlasmaCannon.glb` -> `weapon_SolarPlasmaCannon`."""
    base = os.path.splitext(file_name)[0]
    return base[len(pack) + 1 :] if base.startswith(pack + "_") else base


def clip_label(clip, pack):
    return f"{stem(clip['file'], pack)}/{clip['name']}"


def event_label(e):
    t = e["type"]
    if t == "model":
        robot = "robot" if e.get("show_robot") else "-robot"
        vehicle = "vehicle" if e.get("show_vehicle") else "-vehicle"
        tail = f" {e['vehicle_animation']}" if e.get("vehicle_animation") else ""
        return f"model {robot} {vehicle}{tail}"
    detail = {
        "sound_play": lambda: e.get("event"),
        "sound_stop": lambda: e.get("event"),
        "effect_start": lambda: f"{e.get('effect')}@{e.get('node')}",
        "effect_stop": lambda: f"{e.get('id')}@{e.get('node')}",
        "footstep": lambda: e.get("side"),
        "climb_step": lambda: e.get("limb"),
        "weapon_attach": lambda: f"{e.get('weapon') or e.get('index')}@{e.get('node')}",
        "weapon_fire_enabled": lambda: "on" if e.get("on") else "off",
        "special_fire": lambda: e.get("node"),
        "camera_shake": lambda: e.get("shake"),
        "attack_button_window": lambda: "open" if e.get("open") else "shut",
        "attack_hit": lambda: f"{'on' if e.get('on') else 'off'} {e.get('node')}",
        "trigger": lambda: e.get("name"),
        "motion_mode": lambda: e.get("mode"),
        "partial_animation": lambda: e.get("animation"),
    }.get(t)
    text = detail() if detail else None
    return f"{t} {text}" if text else t


# --- the imported pieces ----------------------------------------------------------------------


def pack_objects(pack_col):
    return [o for o in pack_col.all_objects if "rotf_file" in o]


class Piece:
    """One imported .glb: its nodes as Blender objects or bones, and its actions by clip name."""

    def __init__(self, file_name, objects, actions):
        self.file = file_name
        self.objects = {}  # node name -> object
        self.bones = {}  # node name -> (armature object, bone name)
        for o in objects:
            if o.get("rotf_file") != file_name:
                continue
            self.objects[o["rotf_node"]] = o
            if o.type == "ARMATURE":
                for b in o.data.bones:
                    self.bones[b.name] = (o, b.name)
        self.actions = {a["rotf_clip"]: a for a in actions if a.get("rotf_file") == file_name}
        self.nodes = []  # the file's nodes, as unkeyed_nodes counts them
        self.rest = {}  # node name -> {path: value} for nodes that are objects

    def target(self, node):
        """(object, bone name or None) for a node."""
        if node in self.bones:
            return self.bones[node]
        o = self.objects.get(node)
        return (o, None) if o else None

    def animated_ids(self):
        seen = []
        for o, _ in [self.target(n) for n in self.nodes if self.target(n)]:
            if o not in seen:
                seen.append(o)
        return seen


def pieces_of(pack_col):
    objects = pack_objects(pack_col)
    actions = [a for a in bpy.data.actions if a.get("rotf_pack") == pack_col.get("rotf_pack")]
    files = sorted({o["rotf_file"] for o in objects})
    pieces = {f: Piece(f, objects, actions) for f in files}
    for f, piece in pieces.items():
        piece.nodes = list(pack_col.get("rotf_nodes", {}).get(f, []))
    return pieces


def slot_of(obj, action):
    """The slot that drives `obj` in `action`: the importer files every clip as an NLA strip."""
    ad = obj.animation_data
    if not ad:
        return None
    for track in ad.nla_tracks:
        for strip in track.strips:
            if strip.action == action:
                return strip.action_slot
    return None


def paths_of(bone):
    if bone is None:
        return {p: p for p in OBJECT_PATHS}
    return {p: f'pose.bones["{bone}"].{p}' for p in OBJECT_PATHS}


def curves(action, obj, bone):
    """{path: [fcurve or None per index]} for one node in one action; empty when unkeyed."""
    slot = slot_of(obj, action)
    if slot is None:
        return {}
    bag = anim_utils.action_get_channelbag_for_slot(action, slot)
    if bag is None:
        return {}
    out = {}
    for path, data_path in paths_of(bone).items():
        found = [bag.fcurves.find(data_path, index=i) for i in range(len(REST[path]))]
        if any(found):
            out[path] = found
    return out


# --- holding a vehicle clip's unkeyed nodes ----------------------------------------------------


def vehicle_lead_ins(clips, vehicle_file):
    """For each vehicle clip, the clips that play right before it in the game, with the tick at
    which they are cut (None: they run to their end).

    Read from the robot clips' `model` events: within one clip, an event starting B after an
    event that started A means A is cut at B's start; the clip's last started vehicle clip runs
    to its end and then its `vehicle_then` follows."""
    lead_ins = defaultdict(list)
    for clip in clips:
        current = None  # (vehicle clip, start tick, then)
        for e in clip["events"]:
            if e["type"] != "model" or not e.get("vehicle_animation"):
                continue
            if e.get("vehicle_file") not in (None, vehicle_file):
                continue
            name = e["vehicle_animation"]
            if current and current[0] == name and current[1] == e["tick"]:
                continue
            if current:
                lead_ins[name].append((current[0], e["tick"] - current[1]))
            current = (name, e["tick"], e.get("vehicle_then"))
        if current and current[2]:
            lead_ins[current[2]].append((current[0], None))
    return lead_ins


def hold_unkeyed(piece, clips):
    """Key every node a vehicle clip leaves unanimated, so that each clip poses the whole vehicle:
    a node the clip has no channel for (`unkeyed_node_names`) with the pose its lead-in leaves it
    in, or rest when no clip leads in; a node whose channel keys nothing with its rest.

    Returns a summary per clip: (held from a lead-in, at rest for want of one, at rest by rule)."""
    by_name = {c["name"]: c for c in clips if c["file"] == piece.file and c.get("animation")}
    lead_ins = vehicle_lead_ins(clips, piece.file)
    targets = {n: piece.target(n) for n in piece.nodes if piece.target(n)}
    keyed = {}
    for name in by_name:
        action = piece.actions.get(name)
        if action is None:
            continue
        keyed[name] = {n: curves(action, *t) for n, t in targets.items()}

    def holds(clip, node):
        names = by_name[clip].get("unkeyed_node_names")
        # An export from before the names were written: every unanimated node holds.
        return node in names if names is not None else True

    def value(clip, node, path, at, seen):
        """The node's `path` where `clip` leaves it at tick `at` (None: the clip's end), or None
        when no clip leads in."""
        own = keyed.get(clip, {}).get(node)
        if own:
            if path not in own:
                return rest_for(node, path)
            frame = by_name[clip]["length_ticks"] if at is None else at
            return tuple(c.evaluate(frame) if c else rest_for(node, path)[i] for i, c in enumerate(own[path]))
        if not holds(clip, node):
            return rest_for(node, path)
        for lead, cut in choose(clip, seen):
            v = value(lead, node, path, cut, seen | {clip})
            if v is not None:
                return v
        return None

    def rest_for(node, path):
        return piece.rest.get(node, REST)[path] if targets[node][1] is None else REST[path]

    def choose(clip, seen):
        """The lead-ins to try, the one that resolves most nodes first."""
        options = [o for o in lead_ins.get(clip, []) if o[0] in keyed and o[0] not in seen]
        unkeyed = [n for n in targets if not keyed.get(clip, {}).get(n) and holds(clip, n)]

        def unresolved(option):
            lead, cut = option
            return sum(value(lead, n, "rotation_quaternion", cut, seen | {clip}) is None for n in unkeyed)

        return sorted(options, key=unresolved) if len(options) > 1 else options

    plan = {}
    for name in keyed:
        unkeyed = [n for n in targets if not keyed[name].get(n)]
        if not unkeyed:
            continue
        values_of, held, rest, empty = {}, [], [], []
        for n in unkeyed:
            if not holds(name, n):
                values_of[n] = {p: rest_for(n, p) for p in OBJECT_PATHS}
                empty.append(n)
                continue
            values = {p: value(name, n, p, None, frozenset()) for p in OBJECT_PATHS}
            if any(v is None for v in values.values()):
                values = {p: rest_for(n, p) for p in OBJECT_PATHS}
                rest.append(n)
            else:
                held.append(n)
            values_of[n] = values
        plan[name] = (values_of, held, rest, empty)

    summary = {}
    for name, (values_of, held, rest, empty) in plan.items():
        action = piece.actions[name]
        for n, values in values_of.items():
            obj, bone = targets[n]
            slot = ensure_slot(obj, action)
            bag = ensure_channelbag(action, slot)
            for path, data_path in paths_of(bone).items():
                for i, v in enumerate(values[path]):
                    fc = bag.fcurves.find(data_path, index=i) or bag.fcurves.new(data_path, index=i)
                    key = fc.keyframe_points.insert(0.0, v)
                    key.interpolation = "CONSTANT"
        action["rotf_held_nodes"] = sorted(held)
        action["rotf_rest_nodes"] = sorted(rest + empty)
        summary[name] = (len(held), len(rest), len(empty))
    return summary


def ensure_channelbag(action, slot):
    """The slot's channels in the action's first keyframe strip (made when missing). Blender 5 has
    `anim_utils.action_ensure_channelbag_for_slot`; 4.4 and 4.5 do not."""
    layer = action.layers[0] if action.layers else action.layers.new("Layer")
    strip = layer.strips[0] if layer.strips else layer.strips.new(type="KEYFRAME")
    return strip.channelbag(slot, ensure=True)


def ensure_slot(obj, action):
    slot = slot_of(obj, action)
    if slot is not None:
        return slot
    slot = action.slots.new(id_type=obj.id_type, name=obj.name)
    # File it as the importer files every clip, so it is found again by slot_of.
    ad = obj.animation_data_create()
    track = ad.nla_tracks.new()
    track.name = action.name
    strip = track.strips.new(action.name, 0, action)
    strip.action_slot = slot
    track.mute = True
    return slot


# --- import ------------------------------------------------------------------------------------


def layer_collection(layer, col):
    if layer.collection == col:
        return layer
    for child in layer.children:
        found = layer_collection(child, col)
        if found:
            return found
    return None


# The glTF importer turns every node from y up to z up: (x, y, z) -> (x, -z, y), a turn of 90
# degrees about x. A node's local matrix M becomes Y_UP @ M @ Y_UP⁻¹.
Y_UP = Matrix.Rotation(math.radians(90.0), 4, "X")


def gltf_to_blender(node):
    """A glTF node's rest TRS in Blender's axes, as the glTF importer converts it."""
    t = node.get("translation", [0.0, 0.0, 0.0])
    r = node.get("rotation", [0.0, 0.0, 0.0, 1.0])
    s = node.get("scale", [1.0, 1.0, 1.0])
    return {
        "location": (t[0], -t[2], t[1]),
        "rotation_quaternion": (r[3], r[0], -r[2], r[1]),
        "scale": (s[0], s[2], s[1]),
    }


def rest_in_root(gltf, root):
    """Each node's rest matrix relative to the file's root node, in Blender's axes."""
    nodes = gltf["nodes"]
    out = {}

    def walk(i, above):
        n = nodes[i]
        trs = gltf_to_blender(n)
        local = Matrix.LocRotScale(trs["location"], Quaternion(trs["rotation_quaternion"]), trs["scale"])
        here = above @ local if i != root else Matrix.Identity(4)
        out[n["name"]] = here
        for c in n.get("children", []):
            walk(c, here)

    walk(root, Matrix.Identity(4))
    return out


def rest_bases(objects, rest_root):
    """The rest TRS of each node object, as Blender holds it: what puts the object at its glb rest
    place under its actual parent. An object on a bone hangs from the bone's tail, as Blender
    parents to bones, so its local is not the glb's. {node: {path: value}}"""
    out = {}
    for o in objects:
        node = o.get("rotf_node")
        if node not in rest_root or o.parent is None:
            continue
        parent = o.parent
        above = rest_root.get(parent.get("rotf_node"), Matrix.Identity(4))
        if o.parent_type == "BONE" and o.parent_bone in parent.data.bones:
            bone = parent.data.bones[o.parent_bone]
            above = above @ bone.matrix_local @ Matrix.Translation((0.0, bone.length, 0.0))
        loc, rot, scale = ((above @ o.matrix_parent_inverse).inverted() @ rest_root[node]).decompose()
        out[node] = {"location": tuple(loc), "rotation_quaternion": tuple(rot), "scale": tuple(scale)}
    return out


def import_file(context, path, pack, file_name, col):
    """Import one glb into `col`; tag its objects and actions. Returns the file's nodes (as
    `unkeyed_nodes` counts them), their rest TRS as Blender holds them, and every node's rest
    matrix under the file's root."""
    gltf = read_glb_json(path)
    names = {n.get("name") for n in gltf.get("nodes", [])}
    clip_names = {a.get("name") for a in gltf.get("animations", [])}
    root = gltf["scenes"][gltf.get("scene", 0)]["nodes"][0]
    nodes = [
        n["name"]
        for i, n in enumerate(gltf["nodes"])
        if i != root and "mesh" not in n and not n["name"].endswith(".frame") and ".collision." not in n["name"]
    ]

    # Object names are unique in Blender: a node named like an object already there would get a
    # number, and `Flipper.collision.0` would become `Flipper.collision.001`, its name lost. The
    # objects in the way step aside while the file comes in; its clashing nodes then take the
    # file's name in front (`bumblebee_vehicle:Flipper.collision.0`).
    aside = {}
    for o in bpy.data.objects:
        if o.name in names:
            aside[o] = o.name
    for o in aside:
        o.name = f"{o.name} (aside)"
    before_objects = set(bpy.data.objects)
    before_actions = set(bpy.data.actions)
    context.view_layer.active_layer_collection = layer_collection(context.view_layer.layer_collection, col)
    try:
        bpy.ops.import_scene.gltf(filepath=path)
    finally:
        new_objects = [o for o in bpy.data.objects if o not in before_objects]
        taken = set(aside.values())
        for o in new_objects:
            if o.name in taken:
                o["rotf_node"] = o.name
                o.name = f"{os.path.splitext(file_name)[0]}:{o.name}"
        for o, name in aside.items():
            o.name = name
    new_actions = [a for a in bpy.data.actions if a not in before_actions]

    for o in new_objects:
        node = o.get("rotf_node") or (o.name if o.name in names else SUFFIX.sub("", o.name))
        if node not in names:
            continue  # the importer's own helpers (a bone shape)
        o["rotf_file"] = file_name
        o["rotf_node"] = node
        if o.get("render_set_hidden"):
            o.hide_set(True)
            o.hide_render = True
    for a in new_actions:
        clip = a.name if a.name in clip_names else SUFFIX.sub("", a.name)
        if clip in clip_names:
            a["rotf_pack"] = pack
            a["rotf_file"] = file_name
            a["rotf_clip"] = clip
    rest_root = rest_in_root(gltf, root)
    tagged = [o for o in new_objects if o.get("rotf_file") == file_name]
    return nodes, rest_bases(tagged, rest_root), rest_root


def describe_actions(pack, clips, markers):
    by_key = {(a.get("rotf_file"), a.get("rotf_clip")): a for a in bpy.data.actions if a.get("rotf_pack") == pack}
    for clip in clips:
        action = by_key.get((clip["file"], clip["name"]))
        if action is None:
            continue
        for key in ("set", "id", "looping", "speed", "crossfade", "additive", "length_ticks", "aim"):
            if clip.get(key) is not None:
                action[f"rotf_{key}"] = clip[key]
        if clip.get("attack"):
            action["rotf_attack"] = json.dumps(clip["attack"])
        action["rotf_events"] = json.dumps(clip["events"])
        action.use_frame_range = True
        action.frame_start = 0.0
        action.frame_end = max(clip["length_ticks"], 1.0)
        action.use_cyclic = bool(clip.get("looping"))
        if markers:
            for e in clip["events"]:
                m = action.pose_markers.new(event_label(e))
                m.frame = int(round(e["tick"]))


def node_world(piece, node, rest_root):
    """A node's world matrix now, in Blender. A node that became a bone is not the bone: the
    importer turns bones to point at their children, so the node is the bone times the turn the
    rest pose shows."""
    target = piece.target(node)
    if target is None:
        return None
    obj, bone = target
    if bone is None:
        return obj.matrix_world.copy()
    arm_rest = rest_root.get(obj["rotf_node"], Matrix.Identity(4))
    node_rest = arm_rest.inverted() @ rest_root[node]
    turn = obj.data.bones[bone].matrix_local.inverted() @ node_rest
    return obj.matrix_world @ obj.pose.bones[bone].matrix @ turn


def hang_weapons(context, data, pieces, rests_root, report):
    """Hang each weapon file on its start node's `.frame`: its root goes where that frame is."""
    hung = set()
    for w in data.get("weapons", []) + data.get("melee_weapons", []):
        f = w.get("file")
        if not f or f in hung or f not in pieces or not w.get("hangs_on"):
            continue
        host_file = next((x["file"] for x in data["files"] if x["kind"] == w["hangs_on"]), None)
        host_piece = pieces.get(host_file)
        frame = f"{w['start_node']}.frame"
        target = host_piece.target(frame) if host_piece else None
        root = next((o for o in pieces[f].objects.values() if o.parent is None), None)
        if target is None or root is None:
            report({"WARNING"}, f"{f}: no node {frame} to hang it on")
            continue
        context.view_layer.update()
        world = node_world(host_piece, frame, rests_root[host_file])
        host, bone = target
        root.parent = host
        root.parent_type = "BONE" if bone else "OBJECT"
        if bone:
            root.parent_bone = bone
        context.view_layer.update()
        root.matrix_world = world
        hung.add(f)
    return hung


def add_collision_shapes(pack_col, pieces, data):
    """A wire shape for each collision body, in a `collision` collection inside its file's, so it
    shows and hides with its model."""
    colours = {"Character": (0.2, 0.5, 1.0, 1.0), "Attack": (1.0, 0.25, 0.2, 1.0)}
    count = 0
    for entry in data.get("collision", {}).get("files", []):
        piece = pieces.get(entry["file"])
        if piece is None:
            continue
        file_col = next((c for c in pack_col.children if c.get("rotf_file") == entry["file"]), pack_col)
        col = bpy.data.collections.new(f"{file_col.name} collision")
        file_col.children.link(col)
        for body in entry["bodies"]:
            empty = piece.objects.get(body["gltf_node"])
            if empty is None:
                continue
            bm = bmesh.new()
            if body["shape"] == "sphere":
                bmesh.ops.create_uvsphere(bm, u_segments=16, v_segments=8, radius=body["radius"])
            elif body["shape"] == "cylinder":
                r, h = body["radius"], body["half_height"]
                bmesh.ops.create_cone(bm, cap_ends=True, segments=16, radius1=r, radius2=r, depth=2.0 * h)
            else:
                bmesh.ops.create_cube(bm, size=1.0)
                hx, hy, hz = body["half_extents"]
                bmesh.ops.scale(bm, vec=(2.0 * hx, 2.0 * hy, 2.0 * hz), verts=bm.verts)
            mesh = bpy.data.meshes.new(f"{empty.name}.shape")
            bm.to_mesh(mesh)
            bm.free()
            obj = bpy.data.objects.new(mesh.name, mesh)
            col.objects.link(obj)
            # The shape is in its body's own axes, which the importer turned from y up to z up
            # with every node: the shape turns the same way.
            obj.parent = empty
            obj.matrix_parent_inverse = Matrix.Identity(4)
            obj.matrix_basis = Y_UP
            obj.display_type = "WIRE"
            obj.hide_render = True
            obj.color = colours.get(body.get("mask_preset"), (1.0, 0.8, 0.1, 1.0))
            obj["rotf_collision"] = json.dumps({k: v for k, v in body.items()})
            count += 1
    return count


def import_robot(context, path, pieces_choice, hang, collision, hold, markers, report):
    data = load_robot_json(path)
    version = data.get("format_version")
    if not isinstance(version, int):
        report({"ERROR"}, "not a rotf-export robot JSON (no format_version)")
        return None
    if version > FORMAT_VERSION:
        report({"WARNING"}, f"format_version {version} is newer than this add-on's {FORMAT_VERSION}: update it")
    folder = os.path.dirname(path)
    pack = data["character"]["pack"]
    scene = context.scene
    scene.render.fps = TICKS_PER_SECOND
    scene.render.fps_base = 1.0

    pack_col = bpy.data.collections.new(pack)
    scene.collection.children.link(pack_col)
    pack_col["rotf_json"] = path
    pack_col["rotf_pack"] = pack
    files = [f for f in data["files"] if pieces_choice == "ALL" or f["kind"] in ("robot", "vehicle")]
    nodes_by_file = {}
    rests = {}
    rests_root = {}
    for f in files:
        file_path = os.path.join(folder, f["file"])
        if not os.path.exists(file_path):
            report({"WARNING"}, f"{f['file']} is missing: skipped")
            continue
        col = bpy.data.collections.new(f"{pack} {stem(f['file'], pack)}")
        pack_col.children.link(col)
        col["rotf_kind"] = f["kind"]
        col["rotf_file"] = f["file"]
        nodes, rest, rest_root = import_file(context, file_path, pack, f["file"], col)
        nodes_by_file[f["file"]] = nodes
        rests[f["file"]] = rest
        rests_root[f["file"]] = rest_root
    pack_col["rotf_nodes"] = nodes_by_file
    context.view_layer.active_layer_collection = layer_collection(context.view_layer.layer_collection, pack_col)

    pieces = pieces_of(pack_col)
    for f, piece in pieces.items():
        piece.rest = rests.get(f, {})
    describe_actions(pack, data["clips"], markers)

    lines = []
    vehicle = (data.get("vehicle") or {}).get("file")
    if hold and vehicle in pieces:
        summary = hold_unkeyed(pieces[vehicle], data["clips"])
        held, rest, empty = (sum(s[i] for s in summary.values()) for i in range(3))
        lines.append(
            f"{len(summary)} vehicle clips: {held} unkeyed nodes held from the clip before, "
            f"{rest} at rest with no clip before, {empty} at rest by an empty channel"
        )
    if hang:
        hung = hang_weapons(context, data, pieces, rests_root, report)
        lines.append(f"{len(hung)} weapons hung")
    if collision:
        lines.append(f"{add_collision_shapes(pack_col, pieces, data)} collision shapes")

    # As the game starts: the robot alone, its weapons put away unless they always show.
    shown = {w["file"] for w in data.get("weapons", []) if w.get("always_appear") and w.get("file")}
    for child in pack_col.children:
        kind = child.get("rotf_kind")
        if kind in PROJECTILE_KINDS or kind == "vehicle" or (kind in ("weapon", "melee") and child.get("rotf_file") not in shown):
            child.hide_viewport = True
            child.hide_render = True
    scene.rotf_pack = pack_col.name
    idle = next((c for c in data["clips"] if c["name"] == "IdleStandSet.Idle"), None)
    if idle:
        scene.rotf_clip = clip_label(idle, pack)
        play_clip(context, pack_col, scene.rotf_clip)
    return lines


# --- playing a clip ----------------------------------------------------------------------------

PLAY_TRACK = "RotF play"


def find_clip(data, label):
    pack = data["character"]["pack"]
    return next((c for c in data["clips"] if clip_label(c, pack) == label), None)


def clear_play(pack_col):
    for o in pack_objects(pack_col):
        ad = o.animation_data
        if not ad:
            continue
        for track in list(ad.nla_tracks):
            if track.name == PLAY_TRACK:
                ad.nla_tracks.remove(track)
        ad.action = None


def assign(piece, action):
    for obj in piece.animated_ids():
        slot = slot_of(obj, action)
        ad = obj.animation_data_create()
        if slot is None:
            ad.action = None
            continue
        ad.action = action
        ad.action_slot = slot


def vehicle_schedule(clip, vehicle_file, by_name):
    """The vehicle clips a robot clip's `model` events play: [(start tick, name, end tick)]."""
    out = []
    then = None
    for e in clip["events"]:
        if e["type"] != "model" or not e.get("vehicle_animation"):
            continue
        if e.get("vehicle_file") not in (None, vehicle_file):
            continue
        name = e["vehicle_animation"]
        if out and out[-1][1] == name and out[-1][0] == e["tick"]:
            continue
        if out:
            start, prev, _ = out[-1]
            out[-1] = (start, prev, e["tick"])
        length = by_name[name]["length_ticks"] if name in by_name else 0.0
        out.append((e["tick"], name, e["tick"] + length))
        then = e.get("vehicle_then")
    if out and then and then in by_name:
        end = out[-1][2]
        out.append((end, then, end + by_name[then]["length_ticks"]))
    return out


def schedule_on_nla(piece, schedule):
    for obj in piece.animated_ids():
        ad = obj.animation_data_create()
        ad.action = None
        track = ad.nla_tracks.new()
        track.name = PLAY_TRACK
        for i, (start, name, end) in enumerate(schedule):
            action = piece.actions.get(name)
            slot = slot_of(obj, action) if action else None
            if slot is None:
                continue  # the node keeps what the strip before left it in
            strip = track.strips.new(f"{name}", int(round(start)), action)
            strip.action_slot = slot
            strip.action_frame_start = 0.0
            strip.action_frame_end = max(end - start, 1.0)
            strip.extrapolation = "HOLD" if i == 0 else "HOLD_FORWARD"


def play_clip(context, pack_col, label):
    data = load_robot_json(pack_col["rotf_json"])
    clip = find_clip(data, label)
    if clip is None:
        return f"no clip {label}"
    pieces = pieces_of(pack_col)
    piece = pieces.get(clip["file"])
    if piece is None:
        return f"{clip['file']} was not imported"
    clear_play(pack_col)
    action = piece.actions.get(clip["name"])
    if action:
        assign(piece, action)

    kinds = {c.get("rotf_kind"): c.name for c in pack_col.children if c.get("rotf_kind")}
    vehicle_file = (data.get("vehicle") or {}).get("file")
    visibility = []
    if clip["file"] == vehicle_file:
        visibility.append((0.0, False, True))
    else:
        visibility.append((0.0, True, False))
    by_name = {c["name"]: c for c in data["clips"] if c["file"] == vehicle_file}
    schedule = vehicle_schedule(clip, vehicle_file, by_name)
    if schedule and vehicle_file in pieces:
        schedule_on_nla(pieces[vehicle_file], schedule)
    models = [e for e in clip["events"] if e["type"] == "model"]
    if models:
        visibility = [(e["tick"], bool(e.get("show_robot")), bool(e.get("show_vehicle"))) for e in models]

    scene = context.scene
    for name in scene.get("rotf_markers", []):
        m = scene.timeline_markers.get(name)
        if m:
            scene.timeline_markers.remove(m)
    made = []
    for e in clip["events"]:
        m = scene.timeline_markers.new(event_label(e), frame=int(round(e["tick"])))
        made.append(m.name)
    scene["rotf_markers"] = made
    scene["rotf_visibility"] = json.dumps(
        {"robot": kinds.get("robot"), "vehicle": kinds.get("vehicle"), "events": visibility}
    )
    scene.frame_start = 0
    scene.frame_end = max(int(round(clip["length_ticks"])), 1)
    scene.frame_set(0)
    apply_visibility(scene)
    return None


def apply_visibility(scene):
    raw = scene.get("rotf_visibility")
    if not raw:
        return
    state = json.loads(raw)
    events = state["events"]
    robot, vehicle = events[0][1], events[0][2]
    for tick, r, v in events:
        if tick <= scene.frame_current + 1e-4:
            robot, vehicle = r, v
    for key, show in (("robot", robot), ("vehicle", vehicle)):
        col = bpy.data.collections.get(state.get(key) or "")
        if col and col.hide_viewport == show:
            col.hide_viewport = not show
            col.hide_render = not show


@persistent
def on_frame_change(scene, _depsgraph=None):
    apply_visibility(scene)


# --- operators and UI --------------------------------------------------------------------------


class ROTF_OT_import(bpy.types.Operator, ImportHelper):
    """Import a robot written by rotf-export (its <pack>.json)"""

    bl_idname = "import_scene.rotf_robot"
    bl_label = "Import RotF robot"
    bl_options = {"REGISTER", "UNDO", "PRESET"}

    filename_ext = ".json"
    filter_glob: StringProperty(default="*.json", options={"HIDDEN"})

    pieces: EnumProperty(
        name="Pieces",
        items=(
            ("ROBOT_VEHICLE", "Robot and vehicle", "The robot's and the vehicle's models"),
            ("ALL", "All", "Also the weapons, melee weapons and projectiles"),
        ),
        default="ALL",
    )
    hang_weapons: BoolProperty(name="Hang weapons", default=True, description="Put each weapon on its start node")
    collision_shapes: BoolProperty(
        name="Collision shapes", default=True, description="Wire shapes for the hit boxes and attack bodies"
    )
    hold_unkeyed: BoolProperty(
        name="Hold unkeyed vehicle nodes",
        default=True,
        description="Key a vehicle clip's unkeyed nodes with the pose the clip before it leaves them in",
    )
    event_markers: BoolProperty(name="Event markers", default=True, description="The clips' events as pose markers")

    def execute(self, context):
        if os.path.basename(self.filepath).endswith("_attributes.json"):
            self.report({"ERROR"}, "pick <pack>.json, not the attributes file")
            return {"CANCELLED"}
        lines = import_robot(
            context,
            self.filepath,
            self.pieces,
            self.hang_weapons,
            self.collision_shapes,
            self.hold_unkeyed,
            self.event_markers,
            self.report,
        )
        if lines is None:
            return {"CANCELLED"}
        self.report({"INFO"}, "; ".join(lines))
        return {"FINISHED"}


def pack_collections():
    return [c for c in bpy.data.collections if "rotf_json" in c]


def search_packs(_self, _context, text):
    return [c.name for c in pack_collections() if text.lower() in c.name.lower()]


def search_clips(_self, context, text):
    col = bpy.data.collections.get(context.scene.rotf_pack)
    if not col or "rotf_json" not in col:
        return []
    data = load_robot_json(col["rotf_json"])
    pack = data["character"]["pack"]
    words = text.lower().split()
    labels = [clip_label(c, pack) for c in data["clips"]]
    return [label for label in labels if all(w in label.lower() for w in words)]


class ROTF_OT_play(bpy.types.Operator):
    """Play the chosen clip: its action, the vehicle's clips on a change of form, and the clip's
    events as timeline markers"""

    bl_idname = "rotf.play_clip"
    bl_label = "Play clip"
    bl_options = {"REGISTER", "UNDO"}

    def execute(self, context):
        scene = context.scene
        col = bpy.data.collections.get(scene.rotf_pack)
        if not col or "rotf_json" not in col:
            self.report({"ERROR"}, "import a robot first (File > Import > RotF robot)")
            return {"CANCELLED"}
        if not scene.rotf_clip:
            self.report({"ERROR"}, "pick a clip first: click the Clip box and type part of its name")
            return {"CANCELLED"}
        problem = play_clip(context, col, scene.rotf_clip)
        if problem:
            self.report({"ERROR"}, problem)
            return {"CANCELLED"}
        return {"FINISHED"}


class ROTF_PT_panel(bpy.types.Panel):
    bl_label = "RotF robot"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "RotF"

    def draw(self, context):
        layout = self.layout
        scene = context.scene
        layout.prop(scene, "rotf_pack", text="Robot")
        layout.prop(scene, "rotf_clip", text="Clip")
        layout.operator(ROTF_OT_play.bl_idname, icon="PLAY")
        col = bpy.data.collections.get(scene.rotf_pack)
        if not col or "rotf_json" not in col:
            return
        clip = find_clip(load_robot_json(col["rotf_json"]), scene.rotf_clip)
        if clip:
            box = layout.box()
            box.label(text=f"{clip['length_ticks']:g} ticks, speed {clip.get('speed', 1):g}")
            flags = [k for k in ("looping", "additive") if clip.get(k)]
            if flags:
                box.label(text=", ".join(flags))
            box.label(text=f"{len(clip['events'])} events")


def menu_import(self, _context):
    self.layout.operator(ROTF_OT_import.bl_idname, text="RotF robot (.json)")


CLASSES = (ROTF_OT_import, ROTF_OT_play, ROTF_PT_panel)


def register():
    for cls in CLASSES:
        bpy.utils.register_class(cls)
    bpy.types.TOPBAR_MT_file_import.append(menu_import)
    bpy.types.Scene.rotf_pack = StringProperty(name="Robot", search=search_packs)
    bpy.types.Scene.rotf_clip = StringProperty(name="Clip", search=search_clips)
    bpy.app.handlers.frame_change_pre.append(on_frame_change)


def unregister():
    bpy.app.handlers.frame_change_pre.remove(on_frame_change)
    del bpy.types.Scene.rotf_clip
    del bpy.types.Scene.rotf_pack
    bpy.types.TOPBAR_MT_file_import.remove(menu_import)
    for cls in reversed(CLASSES):
        bpy.utils.unregister_class(cls)
