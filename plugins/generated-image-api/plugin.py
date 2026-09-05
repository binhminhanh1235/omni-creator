#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import os
import re
import sys
from pathlib import Path, PurePosixPath
from typing import Any, Callable

from image_api_support import (
    ApiImageClient,
    PluginFailure,
    extension_for_mime,
    require_text,
    validate_endpoint,
)

API_VERSION = 1
PROVIDER_ID = "generated-image-api"
REQUEST_SCHEMA = "omnicreator.generated-image-request"
REQUEST_SCHEMA_VERSION = 1
DEFAULT_SETTINGS: dict[str, Any] = {
    "api_endpoint": "https://api.openai.com/v1/images/generations",
    "api_key_env": "OPENAI_API_KEY",
    "timeout_seconds": 60,
    "model": "gpt-image-2",
    "model_version": "provider-managed",
}
PROHIBITED_SECRET_SETTING_KEYS = {
    "api_key", "token", "secret", "password", "authorization", "credential"
}
ENV_NAME_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


class PluginState:
    def __init__(self) -> None:
        self.settings = dict(DEFAULT_SETTINGS)
        self.output_dir: Path | None = None

    def initialize(self, params: Any) -> dict[str, Any]:
        if params is None:
            params = {}
        if not isinstance(params, dict):
            raise PluginFailure(
                "INVALID_CONFIGURATION",
                "plugin.initialize params must be an object.",
            )
        workspace = params.get("job_workspace")
        if workspace is not None:
            self.output_dir = validate_job_workspace(workspace)
        if any(key in params for key in ("settings", "job_workspace", "permissions")):
            raw_settings = params.get("settings", {})
        else:
            raw_settings = params
        if raw_settings is None:
            raw_settings = {}
        self.settings = merge_settings(DEFAULT_SETTINGS, raw_settings)
        return {
            "plugin_id": PROVIDER_ID,
            "api_version": API_VERSION,
            "settings": public_settings(self.settings),
            "workspace_ready": self.output_dir is not None,
        }


def merge_settings(base: dict[str, Any], raw: Any) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise PluginFailure(
            "INVALID_CONFIGURATION",
            "Generated-image API settings must be an object.",
        )
    prohibited = sorted(
        key for key in raw
        if isinstance(key, str) and key.strip().lower() in PROHIBITED_SECRET_SETTING_KEYS
    )
    if prohibited:
        raise PluginFailure(
            "SECRET_SETTING_REJECTED",
            "Secret values must stay machine-local; configure only the symbolic api_key_env reference.",
        )
    settings = dict(base)
    for key in DEFAULT_SETTINGS:
        if key in raw:
            settings[key] = raw[key]
    validate_settings(settings)
    return settings


def validate_settings(settings: dict[str, Any]) -> None:
    validate_endpoint(settings.get("api_endpoint"))
    env_name = settings.get("api_key_env")
    if not isinstance(env_name, str) or not ENV_NAME_RE.fullmatch(env_name.strip()):
        raise PluginFailure(
            "INVALID_CONFIGURATION",
            "api_key_env must be a valid environment variable name.",
        )
    timeout = settings.get("timeout_seconds")
    if type(timeout) is not int or not 1 <= timeout <= 300:
        raise PluginFailure(
            "INVALID_CONFIGURATION",
            "timeout_seconds must be an integer between 1 and 300.",
        )
    require_text(settings.get("model"), "model", code="INVALID_CONFIGURATION")
    require_text(
        settings.get("model_version"),
        "model_version",
        code="INVALID_CONFIGURATION",
    )


def public_settings(settings: dict[str, Any]) -> dict[str, Any]:
    return {
        "api_endpoint": settings["api_endpoint"],
        "api_key_env": settings["api_key_env"],
        "timeout_seconds": settings["timeout_seconds"],
        "model": settings["model"],
        "model_version": settings["model_version"],
    }


def health_result(state: PluginState) -> dict[str, Any]:
    configured = True
    try:
        validate_settings(state.settings)
    except PluginFailure:
        configured = False
    env_name = state.settings.get("api_key_env")
    credential_available = (
        isinstance(env_name, str) and bool(os.environ.get(env_name, "").strip())
    )
    return {
        "status": "ready" if configured and credential_available else "needs_attention",
        "provider": PROVIDER_ID,
        "api_execution": {
            "configured": configured,
            "credential": "available" if credential_available else "missing",
        },
        "credential_env": env_name if isinstance(env_name, str) else None,
        "model_id": state.settings.get("model"),
        "model_version": state.settings.get("model_version"),
    }


def capabilities_result(state: PluginState) -> dict[str, Any]:
    return {
        "types": ["visual"],
        "capabilities": ["generated_still", "visual_generate", "api_execution"],
        "operations": ["visual.generate"],
        "generated_still": True,
        "api_execution": True,
        "model_id": state.settings["model"],
        "model_version": state.settings["model_version"],
    }


def optional_text(value: Any) -> str | None:
    if value is None:
        return None
    if not isinstance(value, str):
        raise PluginFailure(
            "INVALID_INPUT",
            "negative_prompt must be a string when present.",
        )
    value = value.strip()
    return value or None


def require_sha256(value: Any, label: str) -> str:
    text = require_text(value, label)
    if not re.fullmatch(r"[0-9a-f]{64}", text):
        raise PluginFailure(
            "INVALID_INPUT",
            f"{label} must be a lowercase SHA-256 hex digest.",
        )
    return text


def validate_request(payload: Any) -> dict[str, Any]:
    if not isinstance(payload, dict):
        raise PluginFailure("INVALID_INPUT", "visual.generate payload must be an object.")
    if payload.get("schema") != REQUEST_SCHEMA or payload.get("version") != REQUEST_SCHEMA_VERSION:
        raise PluginFailure(
            "INVALID_INPUT",
            "Unsupported generated-image request schema/version.",
        )
    scene = payload.get("scene")
    if not isinstance(scene, dict):
        raise PluginFailure("INVALID_INPUT", "scene must be an object.")
    scene_id = require_text(scene.get("id"), "scene.id")
    if not re.fullmatch(r"[A-Za-z0-9_-]+", scene_id):
        raise PluginFailure("INVALID_INPUT", "scene.id contains unsupported characters.")

    resolution = payload.get("resolution")
    if not isinstance(resolution, dict):
        raise PluginFailure("INVALID_INPUT", "resolution must be an object.")
    width = resolution.get("width")
    height = resolution.get("height")
    if (
        type(width) is not int
        or type(height) is not int
        or width <= 0
        or height <= 0
        or width > 16384
        or height > 16384
    ):
        raise PluginFailure("INVALID_INPUT", "resolution dimensions are invalid.")
    seed = payload.get("seed")
    if seed is not None and (type(seed) is not int or seed < 0):
        raise PluginFailure(
            "INVALID_INPUT",
            "seed must be a non-negative integer when present.",
        )
    return {
        "scene_id": scene_id,
        "prompt": require_text(payload.get("prompt"), "prompt"),
        "negative_prompt": optional_text(payload.get("negative_prompt")),
        "width": width,
        "height": height,
        "seed": seed,
        "prompt_sha256": require_sha256(payload.get("prompt_sha256"), "prompt_sha256"),
        "settings_fingerprint": require_sha256(
            payload.get("settings_fingerprint"),
            "settings_fingerprint",
        ),
    }


def validate_job_workspace(raw: Any) -> Path:
    if not isinstance(raw, dict):
        raise PluginFailure(
            "WORKSPACE_REQUIRED",
            "job_workspace must be an object.",
        )
    output = raw.get("output")
    if not isinstance(output, str) or not output.strip():
        raise PluginFailure(
            "WORKSPACE_REQUIRED",
            "job_workspace.output is required.",
        )
    output_dir = Path(output).resolve()
    if not output_dir.is_dir():
        raise PluginFailure(
            "WORKSPACE_REQUIRED",
            "The granted job workspace output directory is unavailable.",
        )
    return output_dir


def resolve_output(output_dir: Path, relative_output: str) -> Path:
    relative = PurePosixPath(relative_output)
    if relative.is_absolute() or not relative.parts or any(
        part in {"", ".", ".."} for part in relative.parts
    ):
        raise PluginFailure(
            "INVALID_OUTPUT_PATH",
            "Output path escapes the job workspace.",
        )
    destination = output_dir.joinpath(*relative.parts).resolve()
    try:
        common = Path(os.path.commonpath([output_dir, destination]))
    except ValueError as error:
        raise PluginFailure(
            "INVALID_OUTPUT_PATH",
            "Output path is outside the job workspace.",
        ) from error
    if common != output_dir:
        raise PluginFailure(
            "INVALID_OUTPUT_PATH",
            "Output path escapes the job workspace.",
        )
    return destination


def execute_visual_generate(
    payload: Any,
    state: PluginState,
    client_factory: Callable[..., ApiImageClient] = ApiImageClient,
) -> dict[str, Any]:
    request = validate_request(payload)
    if state.output_dir is None:
        raise PluginFailure(
            "WORKSPACE_REQUIRED",
            "plugin.initialize must grant a job workspace before generation.",
        )

    api_key_env = state.settings["api_key_env"]
    api_key = os.environ.get(api_key_env, "").strip()
    if not api_key:
        raise PluginFailure(
            "CREDENTIAL_MISSING",
            f"Generated-image API credential is missing from machine-local environment variable {api_key_env}.",
            retryable=False,
        )

    client = client_factory(
        api_key,
        state.settings["api_endpoint"],
        state.settings["timeout_seconds"],
    )
    provider = client.generate(
        prompt=request["prompt"],
        negative_prompt=request["negative_prompt"],
        width=request["width"],
        height=request["height"],
        seed=request["seed"],
        model_id=state.settings["model"],
        model_version=state.settings["model_version"],
    )

    digest = hashlib.sha256(provider.image_bytes).hexdigest()
    relative_output = (
        f"generated/{request['scene_id']}-{digest[:16]}."
        f"{extension_for_mime(provider.mime_type)}"
    )
    destination = resolve_output(state.output_dir, relative_output)
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(provider.image_bytes)

    metadata: dict[str, Any] = {
        "transport": "http_json",
        "response_mime_type": provider.mime_type,
    }
    provenance: dict[str, Any] = {
        "source": "generated",
        "provider": PROVIDER_ID,
        "model_id": provider.model_id,
        "model_version": provider.model_version,
        "execution_target": "api",
    }
    if provider.request_id:
        metadata["provider_request_id"] = provider.request_id
        provenance["provider_request_id"] = provider.request_id

    return {
        "relative_output": relative_output,
        "mime_type": provider.mime_type,
        "width": provider.width,
        "height": provider.height,
        "sha256": digest,
        "model_id": provider.model_id,
        "model_version": provider.model_version,
        "seed": request["seed"],
        "prompt_sha256": request["prompt_sha256"],
        "settings_fingerprint": request["settings_fingerprint"],
        "metadata": metadata,
        "provenance": provenance,
    }


def success_response(request_id: str, result: Any) -> dict[str, Any]:
    return {"api_version": API_VERSION, "request_id": request_id, "result": result}


def failure_response(request_id: str, error: PluginFailure) -> dict[str, Any]:
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
    request: Any,
    state: PluginState,
    client_factory: Callable[..., ApiImageClient] = ApiImageClient,
) -> tuple[dict[str, Any], bool]:
    request_id = "unknown"
    try:
        if not isinstance(request, dict):
            raise PluginFailure(
                "INVALID_REQUEST",
                "Plugin request must be a JSON object.",
            )
        raw_id = request.get("request_id")
        if not isinstance(raw_id, str) or not raw_id.strip():
            raise PluginFailure(
                "INVALID_REQUEST",
                "request_id must be a non-empty string.",
            )
        request_id = raw_id.strip()
        if request.get("api_version") != API_VERSION:
            raise PluginFailure(
                "PLUGIN_API_INCOMPATIBLE",
                f"api_version must be {API_VERSION}.",
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
            return success_response(request_id, capabilities_result(state)), False
        if method == "plugin.cancel":
            return success_response(request_id, {"cancelled": False}), False
        if method == "plugin.shutdown":
            return success_response(request_id, {"shutdown": True}), True
        if method != "plugin.execute":
            raise PluginFailure(
                "UNSUPPORTED_METHOD",
                f"Unsupported plugin method: {method}",
            )
        if params.get("operation") != "visual.generate":
            raise PluginFailure(
                "UNSUPPORTED_OPERATION",
                f"Unsupported operation: {params.get('operation')}",
            )
        result = execute_visual_generate(params.get("payload"), state, client_factory)
        return success_response(request_id, result), False
    except PluginFailure as error:
        return failure_response(request_id, error), False


def run_jsonl() -> int:
    state = PluginState()
    for line in sys.stdin:
        if not line.strip():
            continue
        try:
            request = json.loads(line)
        except json.JSONDecodeError:
            print(
                json.dumps(
                    failure_response(
                        "unknown",
                        PluginFailure("INVALID_JSON", "Request is not valid JSON."),
                    ),
                    separators=(",", ":"),
                ),
                flush=True,
            )
            continue
        response, shutdown = handle_request(request, state)
        print(json.dumps(response, separators=(",", ":")), flush=True)
        if shutdown:
            return 0
    return 0


if __name__ == "__main__":
    raise SystemExit(run_jsonl())
