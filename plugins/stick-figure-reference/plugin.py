#!/usr/bin/env python3
import hashlib
import html
import json
import os
import pathlib
import re
import sys
from dataclasses import dataclass
from typing import Any, Dict, List, Optional, Tuple

API_VERSION = 1
REQUEST_SCHEMA = "omnicreator.generated-image-request"
REQUEST_SCHEMA_VERSION = 1
SCENE_SCHEMA = "omnicreator.scene-intent"
SCENE_SCHEMA_VERSION = 1
MODEL_ID = "stick-figure-reference-svg"
MODEL_VERSION = "1"
OPERATIONS = ["visual.generate"]

PRESETS = {
    "christian-stick-explainer": ("minimal-motion", False),
    "stick-figure-minimal-motion": ("minimal-motion", False),
    "stick-figure-thumbnail": ("none", True),
    "christian-stick-explainer-thumbnail": ("none", True),
}

CHARACTER_RULES = [
    (("child", "children", "kid"), "child"),
    (("mother", "father", "parent"), "parent"),
    (("friend", "neighbor", "neighbour"), "friend"),
    (("pastor", "teacher", "mentor", "leader", "jesus"), "guide"),
    (("worker", "builder", "craftsperson"), "builder"),
]

ACTION_RULES = [
    (("repair", "rebuild", "restore", "mend"), "repair"),
    (("boundary", "limit", "say no", "stop enabling"), "set_boundary"),
    (("forgive", "forgiveness", "grace"), "offer_grace"),
    (("trust", "reconcile", "relationship"), "rebuild_trust"),
    (("carry", "burden", "support", "help"), "support"),
    (("teach", "explain", "learn", "understand"), "explain"),
    (("choose", "decision", "decide"), "choose"),
    (("walk", "journey", "path", "move forward"), "walk"),
    (("wait", "patience", "patient"), "wait"),
    (("pray", "prayer"), "pray"),
    (("speak", "tell", "say", "conversation"), "speak"),
]

OBJECT_RULES = [
    (("bridge",), "bridge"),
    (("fence",), "fence"),
    (("door", "gate"), "door"),
    (("path", "road", "journey"), "path"),
    (("book", "bible", "scripture"), "book"),
    (("heart", "love"), "heart"),
    (("boundary", "limit"), "boundary"),
    (("box", "burden", "load"), "box"),
    (("light", "lamp", "hope"), "light"),
    (("table",), "table"),
    (("cross",), "cross"),
]

ACCENTS = [
    "#3568d4",
    "#6b55c9",
    "#d45d3f",
    "#228b6b",
    "#b1781f",
    "#a44773",
]


@dataclass
class PluginFailure(Exception):
    code: str
    message: str
    retryable: bool = False
    retry_after_seconds: Optional[int] = None
    suggested_fallback: Optional[str] = None


class PluginState:
    def __init__(self) -> None:
        self.output_dir: Optional[pathlib.Path] = None

    def initialize(self, params: Dict[str, Any]) -> Dict[str, Any]:
        workspace = params.get("job_workspace")
        if not isinstance(workspace, dict):
            raise PluginFailure(
                "WORKSPACE_REQUIRED",
                "plugin.initialize requires the granted job_workspace.",
            )

        output = workspace.get("output")
        if not isinstance(output, str) or not output.strip():
            raise PluginFailure(
                "WORKSPACE_REQUIRED",
                "job_workspace.output is required.",
            )

        output_dir = pathlib.Path(output).resolve()
        if not output_dir.is_dir():
            raise PluginFailure(
                "WORKSPACE_REQUIRED",
                "The granted job workspace output directory is unavailable.",
            )

        self.output_dir = output_dir
        return {
            "initialized": True,
            "model_id": MODEL_ID,
            "model_version": MODEL_VERSION,
        }


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical_json(value: Any) -> bytes:
    return json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
    ).encode("utf-8")


def require_text(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise PluginFailure("INVALID_INPUT", f"{label} must not be empty.")
    return " ".join(value.split())


def optional_text(value: Any) -> str:
    if not isinstance(value, str):
        return ""
    return " ".join(value.split())


def require_sha256(value: Any, label: str) -> str:
    text = require_text(value, label)
    if not re.fullmatch(r"[0-9a-fA-F]{64}", text):
        raise PluginFailure("INVALID_INPUT", f"{label} must be a SHA-256 hex digest.")
    return text.lower()


def string_list(value: Any) -> List[str]:
    if not isinstance(value, list):
        return []
    return [optional_text(item) for item in value if optional_text(item)]


def validate_request(payload: Dict[str, Any]) -> Dict[str, Any]:
    if payload.get("schema") != REQUEST_SCHEMA or payload.get("version") != REQUEST_SCHEMA_VERSION:
        raise PluginFailure(
            "INVALID_INPUT",
            "Unsupported generated-image request schema/version.",
        )

    prompt = require_text(payload.get("prompt"), "prompt")
    prompt_sha256 = require_sha256(payload.get("prompt_sha256"), "prompt_sha256")
    settings_fingerprint = require_sha256(
        payload.get("settings_fingerprint"),
        "settings_fingerprint",
    )

    scene = payload.get("scene")
    if not isinstance(scene, dict):
        raise PluginFailure("INVALID_INPUT", "scene must be an object.")
    if (
        scene.get("schema") != SCENE_SCHEMA
        or scene.get("schema_version") != SCENE_SCHEMA_VERSION
    ):
        raise PluginFailure(
            "INVALID_INPUT",
            "Stick figure generation requires SceneIntent v1.",
        )

    scene_id = require_text(scene.get("id"), "scene.id")
    if not re.fullmatch(r"[A-Za-z0-9_-]+", scene_id):
        raise PluginFailure(
            "INVALID_INPUT",
            "scene.id must use only letters, numbers, underscore or hyphen.",
        )

    resolution = payload.get("resolution")
    if not isinstance(resolution, dict):
        raise PluginFailure("INVALID_INPUT", "resolution must be an object.")
    width = resolution.get("width")
    height = resolution.get("height")
    if (
        not isinstance(width, int)
        or isinstance(width, bool)
        or not isinstance(height, int)
        or isinstance(height, bool)
        or width <= 0
        or height <= 0
        or width > 16384
        or height > 16384
    ):
        raise PluginFailure(
            "INVALID_INPUT",
            "resolution dimensions must be integers in 1..=16384.",
        )

    style = payload.get("style")
    if not isinstance(style, dict):
        raise PluginFailure("INVALID_INPUT", "style must be an object.")
    preset = require_text(style.get("preset"), "style.preset")
    if preset not in PRESETS:
        raise PluginFailure(
            "UNSUPPORTED_PRESET",
            f"Unsupported stick figure preset: {preset}",
        )

    seed = payload.get("seed")
    if seed is not None and (
        not isinstance(seed, int) or isinstance(seed, bool) or seed < 0
    ):
        raise PluginFailure("INVALID_INPUT", "seed must be a non-negative integer.")

    return {
        "scene_id": scene_id,
        "scene": scene,
        "prompt": prompt,
        "prompt_sha256": prompt_sha256,
        "settings_fingerprint": settings_fingerprint,
        "width": width,
        "height": height,
        "preset": preset,
        "seed": seed,
        "settings": payload.get("settings", {}),
        "aspect_ratio": require_text(payload.get("aspect_ratio"), "aspect_ratio"),
    }


def resolve_output(state: PluginState, relative_output: str) -> pathlib.Path:
    if state.output_dir is None:
        raise PluginFailure(
            "WORKSPACE_REQUIRED",
            "plugin.initialize must grant a job workspace before generation.",
        )

    relative = pathlib.PurePosixPath(relative_output)
    if relative.is_absolute() or not relative.parts or any(
        part in ("", ".", "..") for part in relative.parts
    ):
        raise PluginFailure("INVALID_OUTPUT_PATH", "Output path escapes the job workspace.")

    destination = state.output_dir.joinpath(*relative.parts).resolve()
    try:
        common = pathlib.Path(os.path.commonpath([state.output_dir, destination]))
    except ValueError as error:
        raise PluginFailure(
            "INVALID_OUTPUT_PATH",
            "Output path is outside the job workspace.",
        ) from error

    if common != state.output_dir:
        raise PluginFailure("INVALID_OUTPUT_PATH", "Output path escapes the job workspace.")
    return destination


def semantic_text(scene: Dict[str, Any]) -> str:
    parts = [
        optional_text(scene.get("narration")),
        optional_text(scene.get("purpose")),
        optional_text(scene.get("emotion_before")),
        optional_text(scene.get("emotion_after")),
        *string_list(scene.get("visual_ideas")),
        *string_list(scene.get("search_queries")),
    ]
    return " ".join(part.lower() for part in parts if part)


def first_matching_rules(
    text: str,
    rules: List[Tuple[Tuple[str, ...], str]],
    limit: int,
) -> List[str]:
    selected: List[str] = []
    for keywords, value in rules:
        if any(keyword in text for keyword in keywords) and value not in selected:
            selected.append(value)
        if len(selected) >= limit:
            break
    return selected


def map_scene_intent(scene: Dict[str, Any], preset: str) -> Dict[str, Any]:
    text = semantic_text(scene)
    roles = first_matching_rules(text, CHARACTER_RULES, 3)

    relational = any(
        term in text
        for term in (
            "together",
            "relationship",
            "another person",
            "one another",
            "each other",
            "friend",
            "neighbor",
            "parent",
            "child",
            "people",
        )
    )
    if not roles:
        roles = ["person", "other"] if relational else ["person"]
    elif relational and len(roles) == 1:
        roles.append("other")

    actions = first_matching_rules(text, ACTION_RULES, 3)
    if not actions:
        scene_type = optional_text(scene.get("scene_type")).lower()
        actions = ["explain" if scene_type in ("conceptual", "educational") else "observe"]

    avoid_text = " ".join(string_list(scene.get("avoid"))).lower()
    objects = [
        value
        for value in first_matching_rules(text, OBJECT_RULES, 4)
        if value not in avoid_text
    ]
    if not objects:
        scene_type = optional_text(scene.get("scene_type")).lower()
        fallback = {
            "conceptual": "concept-card",
            "educational": "sign",
            "emotional": "heart",
            "literal": "ground",
        }.get(scene_type, "concept-card")
        objects = [fallback]

    animation_preset, thumbnail_style = PRESETS[preset]
    layout = "thumbnail-focus" if thumbnail_style else (
        "two-person-dialogue" if len(roles) >= 2 else "single-explainer"
    )

    return {
        "characters": [
            {
                "id": f"character-{index + 1}",
                "role": role,
                "action": actions[min(index, len(actions) - 1)],
            }
            for index, role in enumerate(roles[:3])
        ],
        "actions": actions,
        "objects": objects,
        "layout": layout,
        "animation_preset": animation_preset,
        "thumbnail_style": thumbnail_style,
    }


def accent_from_digest(digest: str) -> str:
    return ACCENTS[int(digest[:2], 16) % len(ACCENTS)]


def stick_pose(action: str) -> Tuple[Tuple[float, float], Tuple[float, float], Tuple[float, float], Tuple[float, float]]:
    if action in ("explain", "speak", "set_boundary", "choose"):
        return ((-6, 6), (8, 2), (-4, 15), (5, 15))
    if action in ("repair", "rebuild_trust"):
        return ((-7, 9), (7, 9), (-5, 15), (5, 15))
    if action == "pray":
        return ((-2, 8), (2, 8), (-4, 15), (4, 15))
    if action in ("walk", "support"):
        return ((-7, 5), (7, 8), (-7, 15), (7, 13))
    if action == "offer_grace":
        return ((-8, 5), (8, 5), (-4, 15), (4, 15))
    return ((-6, 7), (6, 7), (-4, 15), (4, 15))


def render_character(
    character: Dict[str, Any],
    x: float,
    y: float,
    accent: str,
    animated: bool,
    index: int,
) -> str:
    role = html.escape(str(character["role"]), quote=True)
    action = str(character["action"])
    left_arm, right_arm, left_leg, right_leg = stick_pose(action)
    animation = ""
    if animated:
        animation = (
            f'<animateTransform attributeName="transform" type="translate" '
            f'values="0 0;0 -1.1;0 0" dur="{2.2 + index * 0.25:.2f}s" '
            f'begin="{index * 0.12:.2f}s" repeatCount="indefinite"/>'
        )

    return f"""<g transform="translate({x:.2f} {y:.2f})" class="stick-character">
  <title>{role}: {html.escape(action, quote=True)}</title>
  {animation}
  <circle cx="0" cy="0" r="3.8" fill="#ffffff" stroke="#20242b" stroke-width="1.25"/>
  <line x1="0" y1="3.8" x2="0" y2="13" stroke="#20242b" stroke-width="1.45" stroke-linecap="round"/>
  <line x1="0" y1="6" x2="{left_arm[0]}" y2="{left_arm[1]}" stroke="#20242b" stroke-width="1.35" stroke-linecap="round"/>
  <line x1="0" y1="6" x2="{right_arm[0]}" y2="{right_arm[1]}" stroke="{accent}" stroke-width="1.55" stroke-linecap="round"/>
  <line x1="0" y1="13" x2="{left_leg[0]}" y2="{left_leg[1]}" stroke="#20242b" stroke-width="1.35" stroke-linecap="round"/>
  <line x1="0" y1="13" x2="{right_leg[0]}" y2="{right_leg[1]}" stroke="#20242b" stroke-width="1.35" stroke-linecap="round"/>
</g>"""


def render_object(kind: str, x: float, y: float, accent: str) -> str:
    title = html.escape(kind, quote=True)
    if kind == "bridge":
        shape = f'<path d="M{x-11} {y+6} Q{x} {y-3} {x+11} {y+6}" fill="none" stroke="{accent}" stroke-width="2"/><line x1="{x-8}" y1="{y+5}" x2="{x-8}" y2="{y+12}" stroke="#20242b"/><line x1="{x+8}" y1="{y+5}" x2="{x+8}" y2="{y+12}" stroke="#20242b"/>'
    elif kind == "fence":
        shape = "".join(
            f'<line x1="{x+offset}" y1="{y-5}" x2="{x+offset}" y2="{y+10}" stroke="#20242b" stroke-width="1.3"/>'
            for offset in (-8, -3, 2, 7)
        ) + f'<line x1="{x-10}" y1="{y}" x2="{x+10}" y2="{y}" stroke="{accent}" stroke-width="1.5"/><line x1="{x-10}" y1="{y+6}" x2="{x+10}" y2="{y+6}" stroke="{accent}" stroke-width="1.5"/>'
    elif kind == "door":
        shape = f'<rect x="{x-7}" y="{y-8}" width="14" height="20" rx="1" fill="#ffffff" stroke="#20242b" stroke-width="1.4"/><circle cx="{x+4}" cy="{y+2}" r="1" fill="{accent}"/>'
    elif kind == "path":
        shape = f'<path d="M{x-11} {y+10} C{x-3} {y+2} {x+4} {y+5} {x+11} {y-8}" fill="none" stroke="{accent}" stroke-width="2" stroke-dasharray="3 2"/>'
    elif kind == "book":
        shape = f'<path d="M{x-10} {y-5} Q{x-5} {y-8} {x} {y-4} Q{x+5} {y-8} {x+10} {y-5} L{x+9} {y+7} Q{x+4} {y+4} {x} {y+8} Q{x-4} {y+4} {x-9} {y+7} Z" fill="#ffffff" stroke="#20242b" stroke-width="1.2"/><line x1="{x}" y1="{y-4}" x2="{x}" y2="{y+8}" stroke="{accent}"/>'
    elif kind == "heart":
        shape = f'<path d="M{x} {y+8} C{x-12} {y} {x-8} {y-9} {x} {y-4} C{x+8} {y-9} {x+12} {y} {x} {y+8} Z" fill="none" stroke="{accent}" stroke-width="1.8"/>'
    elif kind == "boundary":
        shape = f'<line x1="{x}" y1="{y-10}" x2="{x}" y2="{y+12}" stroke="{accent}" stroke-width="2.4"/><line x1="{x-5}" y1="{y-7}" x2="{x+5}" y2="{y-7}" stroke="#20242b"/><line x1="{x-5}" y1="{y+8}" x2="{x+5}" y2="{y+8}" stroke="#20242b"/>'
    elif kind == "box":
        shape = f'<rect x="{x-8}" y="{y-5}" width="16" height="14" rx="1.5" fill="#ffffff" stroke="#20242b" stroke-width="1.3"/><path d="M{x-8} {y-5} L{x} {y} L{x+8} {y-5}" fill="none" stroke="{accent}"/>'
    elif kind == "light":
        shape = f'<circle cx="{x}" cy="{y}" r="5" fill="none" stroke="{accent}" stroke-width="1.7"/><line x1="{x}" y1="{y+5}" x2="{x}" y2="{y+10}" stroke="#20242b"/><line x1="{x-3}" y1="{y+10}" x2="{x+3}" y2="{y+10}" stroke="#20242b"/>'
    elif kind == "table":
        shape = f'<line x1="{x-10}" y1="{y}" x2="{x+10}" y2="{y}" stroke="{accent}" stroke-width="2"/><line x1="{x-7}" y1="{y}" x2="{x-8}" y2="{y+10}" stroke="#20242b"/><line x1="{x+7}" y1="{y}" x2="{x+8}" y2="{y+10}" stroke="#20242b"/>'
    elif kind == "cross":
        shape = f'<line x1="{x}" y1="{y-10}" x2="{x}" y2="{y+11}" stroke="#20242b" stroke-width="2"/><line x1="{x-6}" y1="{y-3}" x2="{x+6}" y2="{y-3}" stroke="{accent}" stroke-width="2"/>'
    elif kind == "sign":
        shape = f'<rect x="{x-9}" y="{y-7}" width="18" height="12" rx="2" fill="#ffffff" stroke="{accent}" stroke-width="1.6"/><line x1="{x}" y1="{y+5}" x2="{x}" y2="{y+12}" stroke="#20242b"/>'
    elif kind == "ground":
        shape = f'<line x1="{x-12}" y1="{y+8}" x2="{x+12}" y2="{y+8}" stroke="{accent}" stroke-width="1.8"/>'
    else:
        shape = f'<rect x="{x-9}" y="{y-7}" width="18" height="14" rx="3" fill="#ffffff" stroke="{accent}" stroke-width="1.6"/><circle cx="{x}" cy="{y}" r="2" fill="{accent}"/>'
    return f'<g class="stick-object"><title>{title}</title>{shape}</g>'


def render_svg(request: Dict[str, Any], plan: Dict[str, Any]) -> bytes:
    digest = sha256_hex(
        canonical_json(
            {
                "model": [MODEL_ID, MODEL_VERSION],
                "scene": request["scene"],
                "prompt_sha256": request["prompt_sha256"],
                "settings_fingerprint": request["settings_fingerprint"],
                "preset": request["preset"],
                "seed": request["seed"],
                "resolution": [request["width"], request["height"]],
                "aspect_ratio": request["aspect_ratio"],
                "semantic_plan": plan,
            }
        )
    )
    accent = accent_from_digest(digest)
    width = request["width"]
    height = request["height"]
    scene_id = html.escape(request["scene_id"], quote=True)
    purpose = html.escape(optional_text(request["scene"].get("purpose"))[:160], quote=True)
    animated = plan["animation_preset"] == "minimal-motion"
    thumbnail = bool(plan["thumbnail_style"])

    if thumbnail:
        character_positions = [(72.0, 25.0), (84.0, 27.0), (62.0, 28.0)]
        object_positions = [(76.0, 17.0), (88.0, 16.0), (65.0, 17.0), (82.0, 37.0)]
    else:
        count = len(plan["characters"])
        character_positions = {
            1: [(34.0, 25.0)],
            2: [(28.0, 25.0), (47.0, 25.0)],
            3: [(22.0, 25.0), (39.0, 25.0), (56.0, 25.0)],
        }[min(3, max(1, count))]
        object_positions = [(72.0, 27.0), (84.0, 19.0), (82.0, 38.0), (64.0, 14.0)]

    characters = "\n".join(
        render_character(character, *character_positions[index], accent, animated, index)
        for index, character in enumerate(plan["characters"][:3])
    )
    objects = "\n".join(
        render_object(kind, *object_positions[index], accent)
        for index, kind in enumerate(plan["objects"][:4])
    )

    motion_hint = ""
    if animated:
        motion_hint = f"""<path d="M12 48 C28 44 45 49 58 45" fill="none" stroke="{accent}" stroke-opacity="0.35" stroke-width="1" stroke-dasharray="2 2">
  <animate attributeName="stroke-dashoffset" values="0;8" dur="2.8s" repeatCount="indefinite"/>
</path>"""

    thumbnail_safe = ""
    if thumbnail:
        thumbnail_safe = f"""<rect x="5" y="7" width="47" height="39" rx="4" fill="{accent}" fill-opacity="0.08" stroke="{accent}" stroke-opacity="0.28" stroke-width="0.8"/>
<path d="M10 17 H43 M10 24 H37 M10 31 H41" stroke="#20242b" stroke-opacity="0.28" stroke-width="1.4" stroke-linecap="round"/>"""

    svg = f"""<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 100 56.25" role="img" aria-labelledby="title desc">
  <title id="title">Stick figure scene {scene_id}</title>
  <desc id="desc">{purpose or "Semantic stick figure explanation"}</desc>
  <rect width="100" height="56.25" fill="#fbfaf7"/>
  <rect x="2" y="2" width="96" height="52.25" rx="3" fill="none" stroke="#d9d6cf" stroke-width="0.55"/>
  <line x1="7" y1="45.5" x2="93" y2="45.5" stroke="#d5d2cb" stroke-width="0.7"/>
  {thumbnail_safe}
  {motion_hint}
  {characters}
  {objects}
  <circle cx="94" cy="6" r="1.6" fill="{accent}"/>
</svg>
"""
    return svg.encode("utf-8")


def execute_visual_generate(payload: Dict[str, Any], state: PluginState) -> Dict[str, Any]:
    if not isinstance(payload, dict):
        raise PluginFailure("INVALID_INPUT", "visual.generate payload must be an object.")

    request = validate_request(payload)
    plan = map_scene_intent(request["scene"], request["preset"])
    svg = render_svg(request, plan)
    digest = sha256_hex(svg)
    relative_output = f"stick-figure/{request['scene_id']}-{digest[:16]}.svg"
    destination = resolve_output(state, relative_output)
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(svg)

    return {
        "relative_output": relative_output,
        "mime_type": "image/svg+xml",
        "width": request["width"],
        "height": request["height"],
        "sha256": digest,
        "model_id": MODEL_ID,
        "model_version": MODEL_VERSION,
        "seed": request["seed"],
        "prompt_sha256": request["prompt_sha256"],
        "settings_fingerprint": request["settings_fingerprint"],
        "metadata": {
            "renderer": "stick-figure-procedural-svg-v1",
            "deterministic": True,
            "semantic_plan": plan,
            "animation_preset": plan["animation_preset"],
            "thumbnail_style": plan["thumbnail_style"],
        },
        "provenance": {
            "source": "generated",
            "provider": "stick-figure-reference",
            "model_id": MODEL_ID,
            "model_version": MODEL_VERSION,
            "offline": True,
        },
    }


def health_result(state: PluginState) -> Dict[str, Any]:
    return {
        "status": "ready" if state.output_dir is not None else "ready_for_initialize",
        "offline": True,
        "credential_required": False,
    }


def capabilities_result() -> Dict[str, Any]:
    return {
        "operations": OPERATIONS,
        "stick_figure_visual": True,
        "visual_generate": True,
        "deterministic_seed": True,
        "minimal_motion": True,
        "thumbnail_style": "stick-figure-thumbnail",
        "model_id": MODEL_ID,
        "model_version": MODEL_VERSION,
    }


def success_response(request_id: str, result: Any) -> Dict[str, Any]:
    return {
        "api_version": API_VERSION,
        "request_id": request_id,
        "result": result,
    }


def failure_response(request_id: str, error: PluginFailure) -> Dict[str, Any]:
    return {
        "api_version": API_VERSION,
        "request_id": request_id,
        "error": {
            "code": error.code,
            "message": error.message,
            "retryable": error.retryable,
            "retry_after_seconds": error.retry_after_seconds,
            "suggested_fallback": error.suggested_fallback,
        },
    }


def handle_request(
    request: Dict[str, Any],
    state: PluginState,
) -> Tuple[Dict[str, Any], bool]:
    request_id = request.get("request_id")
    if not isinstance(request_id, str) or not request_id.strip():
        request_id = "unknown"

    try:
        if request.get("api_version") != API_VERSION:
            raise PluginFailure(
                "API_VERSION_UNSUPPORTED",
                f"Expected plugin API version {API_VERSION}.",
            )

        method = request.get("method")
        params = request.get("params")
        if params is None:
            params = {}
        if not isinstance(params, dict):
            raise PluginFailure("INVALID_REQUEST", "params must be an object.")

        if method == "plugin.initialize":
            return success_response(request_id, state.initialize(params)), False
        if method == "plugin.health":
            return success_response(request_id, health_result(state)), False
        if method == "plugin.capabilities":
            return success_response(request_id, capabilities_result()), False
        if method == "plugin.cancel":
            return success_response(request_id, {"cancelled": False}), False
        if method == "plugin.shutdown":
            return success_response(request_id, {"shutdown": True}), True
        if method != "plugin.execute":
            raise PluginFailure(
                "UNSUPPORTED_METHOD",
                f"Unsupported plugin method: {method}",
            )

        operation = params.get("operation")
        payload = params.get("payload")
        if operation != "visual.generate":
            raise PluginFailure(
                "UNSUPPORTED_OPERATION",
                f"Unsupported operation: {operation}",
            )
        return success_response(
            request_id,
            execute_visual_generate(payload, state),
        ), False
    except PluginFailure as error:
        return failure_response(request_id, error), False


def run_jsonl() -> int:
    state = PluginState()
    for line in sys.stdin:
        if not line.strip():
            continue
        try:
            request = json.loads(line)
            if not isinstance(request, dict):
                raise ValueError("request must be a JSON object")
        except (json.JSONDecodeError, ValueError) as error:
            response = failure_response(
                "unknown",
                PluginFailure(
                    "INVALID_JSON",
                    f"Request is not valid JSON: {error}",
                ),
            )
            print(json.dumps(response, separators=(",", ":")), flush=True)
            continue

        response, should_shutdown = handle_request(request, state)
        print(json.dumps(response, separators=(",", ":")), flush=True)
        if should_shutdown:
            return 0
    return 0


if __name__ == "__main__":
    raise SystemExit(run_jsonl())
