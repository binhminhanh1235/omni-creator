#!/usr/bin/env python3
import hashlib
import html
import json
import os
import pathlib
import re
import sys
from dataclasses import dataclass
from typing import Any, Dict, Optional, Tuple

API_VERSION = 1
REQUEST_SCHEMA = "omnicreator.generated-image-request"
REQUEST_SCHEMA_VERSION = 1
MODEL_ID = "reference-svg"
MODEL_VERSION = "1"
OPERATIONS = ["visual.generate"]


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


def require_sha256(value: Any, label: str) -> str:
    text = require_text(value, label)
    if not re.fullmatch(r"[0-9a-fA-F]{64}", text):
        raise PluginFailure("INVALID_INPUT", f"{label} must be a SHA-256 hex digest.")
    return text.lower()


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

    seed = payload.get("seed")
    if seed is not None and (
        not isinstance(seed, int) or isinstance(seed, bool) or seed < 0
    ):
        raise PluginFailure("INVALID_INPUT", "seed must be a non-negative integer.")

    expected_prompt_hash = sha256_hex(
        b"".join(
            [
                len(b"generated-image-prompt-v1").to_bytes(8, "big"),
                b"generated-image-prompt-v1",
                len(prompt.encode("utf-8")).to_bytes(8, "big"),
                prompt.encode("utf-8"),
            ]
        )
    )
    # Core owns the request fingerprint. We require well-formed input and echo it
    # rather than reimplementing the Rust framing contract in providers.
    del expected_prompt_hash

    return {
        "scene_id": scene_id,
        "prompt": prompt,
        "prompt_sha256": prompt_sha256,
        "settings_fingerprint": settings_fingerprint,
        "width": width,
        "height": height,
        "preset": preset,
        "seed": seed,
        "negative_prompt": payload.get("negative_prompt"),
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


def palette_from_digest(digest: str) -> Tuple[str, str, str]:
    return (
        f"#{digest[0:6]}",
        f"#{digest[6:12]}",
        f"#{digest[12:18]}",
    )


def render_svg(request: Dict[str, Any]) -> bytes:
    digest = sha256_hex(
        canonical_json(
            {
                "model": [MODEL_ID, MODEL_VERSION],
                "prompt": request["prompt"],
                "prompt_sha256": request["prompt_sha256"],
                "settings_fingerprint": request["settings_fingerprint"],
                "style": request["preset"],
                "seed": request["seed"],
                "resolution": [request["width"], request["height"]],
                "aspect_ratio": request["aspect_ratio"],
            }
        )
    )
    color_a, color_b, accent = palette_from_digest(digest)
    width = request["width"]
    height = request["height"]
    title = html.escape(request["prompt"][:160], quote=True)
    preset = html.escape(request["preset"][:80], quote=True)
    scene_id = html.escape(request["scene_id"], quote=True)

    circle_x = 15 + int(digest[18:20], 16) % 70
    circle_y = 15 + int(digest[20:22], 16) % 70
    circle_r = 12 + int(digest[22:24], 16) % 24

    svg = f"""<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 100 100" role="img" aria-labelledby="title desc">
  <title id="title">{title}</title>
  <desc id="desc">Deterministic generated still for {scene_id}, preset {preset}</desc>
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="{color_a}"/>
      <stop offset="100%" stop-color="{color_b}"/>
    </linearGradient>
  </defs>
  <rect width="100" height="100" fill="url(#bg)"/>
  <circle cx="{circle_x}" cy="{circle_y}" r="{circle_r}" fill="{accent}" opacity="0.72"/>
  <path d="M0 82 C24 62 52 96 100 68 L100 100 L0 100 Z" fill="#000000" opacity="0.24"/>
  <rect x="6" y="6" width="88" height="88" rx="4" fill="none" stroke="#ffffff" stroke-opacity="0.36" stroke-width="0.8"/>
</svg>
"""
    return svg.encode("utf-8")


def execute_visual_generate(payload: Dict[str, Any], state: PluginState) -> Dict[str, Any]:
    if not isinstance(payload, dict):
        raise PluginFailure("INVALID_INPUT", "visual.generate payload must be an object.")

    request = validate_request(payload)
    svg = render_svg(request)
    digest = sha256_hex(svg)
    relative_output = f"generated/{request['scene_id']}-{digest[:16]}.svg"
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
            "renderer": "procedural-svg-v1",
            "deterministic": True,
        },
        "provenance": {
            "source": "generated",
            "provider": "generated-image-reference",
            "model_id": MODEL_ID,
            "model_version": MODEL_VERSION,
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
        "generated_still": True,
        "deterministic_seed": True,
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
