"""Builds every Iron Tide model procedurally, exports .glb files and renders previews (and optionally UI icons).

blender -b --factory-startup --python tools/blender/build_models.py -- \
    --out assets/models --previews assets/previews --icons assets/icons
"""
import argparse
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from irontide_models import buildings, decorations, nature, ore, soldier, units, vegetation  # noqa: E402
from irontide_models.materials import TEAM_COLOR_MATERIAL, set_team_color  # noqa: E402
from irontide_models.parts import TURRET, begin_model  # noqa: E402
from irontide_models.pipeline import export_glb, inspect_glb, render_icon, render_preview, reset_scene  # noqa: E402

TEAMS = ("blue", "red", "orange", "purple")
PREVIEW_TEAM = "blue"
UNIT_TRIANGLE_BUDGET = 16_000
BUILDING_TRIANGLE_BUDGET = 30_000
NEUTRAL_TRIANGLE_BUDGET = 5_000
DECORATION_TRIANGLE_BUDGET = 3_000

TEAM_MODELS = {
    "rifleman": (soldier.build_rifleman, UNIT_TRIANGLE_BUDGET),
    "rocketeer": (soldier.build_rocketeer, UNIT_TRIANGLE_BUDGET),
    "medic": (soldier.build_medic, UNIT_TRIANGLE_BUDGET),
    "engineer": (soldier.build_engineer, UNIT_TRIANGLE_BUDGET),
    "sniper": (soldier.build_sniper, UNIT_TRIANGLE_BUDGET),
    "scout": (units.build_scout, UNIT_TRIANGLE_BUDGET),
    "tank": (units.build_tank, UNIT_TRIANGLE_BUDGET),
    "bulwark": (units.build_bulwark, UNIT_TRIANGLE_BUDGET),
    "artillery": (units.build_artillery, UNIT_TRIANGLE_BUDGET),
    "harvester": (units.build_harvester, UNIT_TRIANGLE_BUDGET),
    "gunship": (units.build_gunship, UNIT_TRIANGLE_BUDGET),
    "flak_track": (units.build_flak_track, UNIT_TRIANGLE_BUDGET),
    "command_center": (buildings.build_command_center, BUILDING_TRIANGLE_BUDGET),
    "power_plant": (buildings.build_power_plant, BUILDING_TRIANGLE_BUDGET),
    "refinery": (buildings.build_refinery, BUILDING_TRIANGLE_BUDGET),
    "barracks": (buildings.build_barracks, BUILDING_TRIANGLE_BUDGET),
    "factory": (buildings.build_factory, BUILDING_TRIANGLE_BUDGET),
    "gun_turret": (buildings.build_gun_turret, BUILDING_TRIANGLE_BUDGET),
    "helipad": (buildings.build_helipad, BUILDING_TRIANGLE_BUDGET),
    "flak_cannon": (buildings.build_flak_cannon, BUILDING_TRIANGLE_BUDGET),
    "tech_center": (buildings.build_tech_center, BUILDING_TRIANGLE_BUDGET),
    "wall": (buildings.build_wall, 4_000),
}
NEUTRAL_MODELS = {
    "crystal": (ore.build_crystal_a, 3_000),
    "crystal_a": (ore.build_crystal_a, 3_000),
    "crystal_b": (ore.build_crystal_b, 3_000),
    "crystal_c": (ore.build_crystal_c, 3_000),
    "tree": (vegetation.build_tree, 6_000),
    "rock": (nature.build_rock, NEUTRAL_TRIANGLE_BUDGET),
    "rock_a": (ore.build_rock_a, 1_500),
    "rock_b": (ore.build_rock_b, 1_500),
    "bush": (decorations.build_bush, DECORATION_TRIANGLE_BUDGET),
    "grass_tuft": (decorations.build_grass_tuft, 600),
    "flowers": (decorations.build_flowers, 800),
    "pebbles": (decorations.build_pebbles, 600),
    "fallen_log": (decorations.build_fallen_log, DECORATION_TRIANGLE_BUDGET),
    "stump": (decorations.build_stump, DECORATION_TRIANGLE_BUDGET),
    "reeds": (decorations.build_reeds, 1_000),
    "boulder_large": (decorations.build_boulder_large, 4_000),
    "tree_pine": (vegetation.build_tree_pine, 7_000),
    "tree_pine_young": (vegetation.build_tree_pine_young, 4_000),
    "tree_broadleaf": (vegetation.build_tree_broadleaf, 8_000),
    "tree_birch": (vegetation.build_tree_birch, 6_000),
    "tree_dead": (vegetation.build_tree_dead, 3_000),
    "bush_round": (vegetation.build_bush_round, 2_000),
    "bush_flowering": (vegetation.build_bush_flowering, 2_000),
}
MODELS_WITH_TURRET = {"tank", "artillery", "scout", "gun_turret", "bulwark", "flak_track", "flak_cannon"}
INFANTRY_NODES = ("Rig", "Body", "hips", "chest", "head", "rifle", "hand.L", "hand.R")
INFANTRY_CLIPS = {"Idle": None, "Walk": None, "KneelFire": None, "Death": None}
REQUIRED_NODES = {
    "harvester": ("OreLoad", "CollectorDrum"),
    "rifleman": INFANTRY_NODES,
    "rocketeer": INFANTRY_NODES,
    "medic": INFANTRY_NODES,
    "engineer": INFANTRY_NODES,
    "sniper": INFANTRY_NODES,
    "gunship": ("Rotor", "TailRotor"),
    "bulwark": ("Turret", "Barrel", "Sprocket_L", "Sprocket_R", "Idler_L", "Idler_R",
                *(f"RoadWheel_{side}{index}" for side in "LR" for index in range(1, 8))),
}
CRYSTAL_IDLE = {"Idle": None}
REQUIRED_ANIMATIONS = {
    "rifleman": INFANTRY_CLIPS,
    "rocketeer": INFANTRY_CLIPS,
    "medic": INFANTRY_CLIPS,
    "engineer": INFANTRY_CLIPS,
    "sniper": INFANTRY_CLIPS,
    "bulwark": {"Drive": None, "Fire": "Barrel"},
    "scout": {"Drive": None},
    "tank": {"Drive": None, "Fire": "Barrel"},
    "artillery": {"Drive": None, "Fire": "Barrel"},
    "gun_turret": {"Fire": "Barrel"},
    "flak_cannon": {"Fire": "Barrel"},
    "flak_track": {"Drive": None, "Fire": "Barrel"},
    "harvester": {"Drive": None, "Work": "CollectorDrum"},
    "crystal": CRYSTAL_IDLE,
    "crystal_a": CRYSTAL_IDLE,
    "crystal_b": CRYSTAL_IDLE,
    "crystal_c": CRYSTAL_IDLE,
}


def parse_arguments():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True, help="directory for .glb files")
    parser.add_argument("--previews", type=Path, required=True, help="directory for preview .png files")
    parser.add_argument("--only", default="", help="comma separated model ids to build (default: all)")
    parser.add_argument("--no-previews", action="store_true")
    parser.add_argument("--icons", type=Path, help="directory for transparent UI icons of units and buildings")
    return parser.parse_args(argv)


def build_model(model_id, builder):
    reset_scene()
    assembly = begin_model(model_id)
    builder()
    return assembly.finalize()[0]


def verify_export(model_id, path, budget):
    summary = inspect_glb(path)
    problems = []
    if model_id in MODELS_WITH_TURRET and TURRET not in summary.node_names:
        problems.append("missing Turret node")
    if summary.triangles > budget:
        problems.append(f"{summary.triangles} triangles exceed budget {budget}")
    for node_name in REQUIRED_NODES.get(model_id, ()):
        if node_name not in summary.node_names:
            problems.append(f"missing node {node_name}")
    for name, required_target in REQUIRED_ANIMATIONS.get(model_id, {}).items():
        if name not in summary.animations:
            problems.append(f"missing animation {name}")
        elif required_target and required_target not in summary.animations[name][1]:
            problems.append(f"animation {name} does not animate {required_target}")
    return summary, problems


def build_all(arguments):
    arguments.out.mkdir(parents=True, exist_ok=True)
    arguments.previews.mkdir(parents=True, exist_ok=True)
    if arguments.icons:
        arguments.icons.mkdir(parents=True, exist_ok=True)
    selected = {name for name in arguments.only.split(",") if name}
    report = []
    failures = []

    for model_id, (builder, budget) in {**TEAM_MODELS, **NEUTRAL_MODELS}.items():
        if selected and model_id not in selected:
            continue
        started = time.time()
        root = build_model(model_id, builder)
        has_teams = model_id in TEAM_MODELS
        teams = TEAMS if has_teams else (None,)
        for team in teams:
            if team:
                set_team_color(team)
            path = arguments.out / (f"{model_id}_{team}.glb" if team else f"{model_id}.glb")
            export_glb(root, path)
            summary, problems = verify_export(model_id, path, budget)
            if has_teams and TEAM_COLOR_MATERIAL not in summary.material_names:
                problems.append("TeamColor material not exported")
            failures.extend(f"{path.name}: {p}" for p in problems)
            report.append(f"{path.name:28s} {summary.triangles:6d} tris  nodes={summary.node_names}")
            for name, (duration, targets) in summary.animations.items():
                report.append(f"{'':28s} anim {name} {duration:.2f}s -> {targets}")
            if not arguments.no_previews and team in (PREVIEW_TEAM, None):
                render_preview(root, arguments.previews / f"{model_id}.png")
            if arguments.icons and team == PREVIEW_TEAM:
                render_icon(root, arguments.icons / f"{model_id}.png")
        print(f"[iron-tide] built {model_id} in {time.time() - started:.1f}s", flush=True)

    print("\n[iron-tide] export summary")
    for line in report:
        print("  " + line)
    if failures:
        print("[iron-tide] FAILURES:\n  " + "\n  ".join(failures))
        sys.exit(1)


if __name__ == "__main__":
    build_all(parse_arguments())
