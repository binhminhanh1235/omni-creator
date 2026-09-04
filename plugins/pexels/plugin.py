#!/usr/bin/env python3
"""OmniCreator Pexels VisualProvider plugin.

Plugin API v1 JSONL adapter. Search is intentionally preview-first: provider
responses are normalized without exposing original photo URLs or video_files.
Full-media resolution belongs to the selected-asset step.
"""

from __future__ import annotations

import json
import os
import socket
import sys
from dataclasses import dataclass
from typing import Any, Callable
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode
from urllib.request import Request, urlopen

API_VERSION = 1
PROVIDER_ID = "pexels"
PEXELS_API_BASE = "https://api.pexels.com"
PEXELS_HOST = "api.pexels.com"
REQUEST_TIMEOUT_SECONDS = 20

SUPPORTED_MEDIA_TYPES = {"video", "image", "both"}
SUPPORTED_ORIENTATIONS = {"auto", "landscape", "portrait", "square"}
SUPPORTED_SIZES = {"small", "medium", "large"}
SUPPORTED_LOCALES = {
    "en-US",
    "pt-BR",
    "es-ES",
    "ca-ES",
    "de-DE",
    "it-IT",
    "fr-FR",
    "sv-SE",
    "id-ID",
    "pl-PL",
    "ja-JP",
    "zh-TW",
    "zh-CN",
    "ko-KR",
    "th-TH",
    "nl-NL",
    "hu-HU",
    "vi-VN",
    "cs-CZ",
    "da-DK",
    "fi-FI",
    "uk-UA",
    "el-GR",
    "ro-RO",
    "nb-NO",
    "sk-SK",
    "tr-TR",
    "ru-RU",
}

DEFAULT_SETTINGS: dict[str, Any] = {
    "media_type": "video",
    "per_query": 8,
    "orientation": "auto",
    "minimum_size": "medium",
    "locale": "en-US",
    "api_key_env": "PEXELS_API_KEY",
}

PROHIBITED_SECRET_SETTING_KEYS = {
    "api_key",
    "token",
    "secret",
    "password",
    "authorization",
}


@dataclass
class PluginFailure(Exception):
    code: str
    message: str
    retryable: bool = False
    retry_after_seconds: int | None = None
    suggested_fallback: str | None = None

    def __str__(self) -> str:
        return self.message


@dataclass
class PexelsResponse:
    data: dict[str, Any]
    quota: dict[str, int | None]


class PexelsClient:
    def __init__(
        self,
        api_key: str,
        opener: Callable[..., Any] = urlopen,
        api_base: str = PEXELS_API_BASE,
        timeout_seconds: int = REQUEST_TIMEOUT_SECONDS,
    ) -> None:
        if not api_key.strip():
            raise PluginFailure(
                "CREDENTIAL_MISSING",
                "Pexels API key is missing.",
                retryable=False,
            )
        self._api_key = api_key.strip()
        self._opener = opener
        self._api_base = api_base.rstrip("/")
        self._timeout_seconds = timeout_seconds

    def search_videos(
        self,
        query: str,
        *,
        orientation: str | None,
        size: str,
        locale: str,
        per_page: int,
    ) -> PexelsResponse:
        params = self._search_params(
            query=query,
            orientation=orientation,
            size=size,
            locale=locale,
            per_page=per_page,
        )
        return self._get_json("/v1/videos/search", params)

    def search_photos(
        self,
        query: str,
        *,
        orientation: str | None,
        size: str,
        locale: str,
        per_page: int,
    ) -> PexelsResponse:
        params = self._search_params(
            query=query,
            orientation=orientation,
            size=size,
            locale=locale,
            per_page=per_page,
        )
        return self._get_json("/v1/search", params)

    @staticmethod
    def _search_params(
        *,
        query: str,
        orientation: str | None,
        size: str,
        locale: str,
        per_page: int,
    ) -> dict[str, Any]:
        params: dict[str, Any] = {
            "query": query,
            "size": size,
            "locale": locale,
            "per_page": per_page,
            "page": 1,
        }
        if orientation is not None:
            params["orientation"] = orientation
        return params

    def _get_json(self, path: str, params: dict[str, Any]) -> PexelsResponse:
        url = f"{self._api_base}{path}?{urlencode(params)}"
        request = Request(
            url,
            method="GET",
            headers={
                "Authorization": self._api_key,
                "Accept": "application/json",
                "User-Agent": "OmniCreator-Pexels/1.0",
            },
        )

        try:
            with self._opener(request, timeout=self._timeout_seconds) as response:
                payload = response.read()
                headers = response.headers
        except HTTPError as error:
            raise map_http_error(error) from error
        except (URLError, TimeoutError, socket.timeout) as error:
            raise PluginFailure(
                "PEXELS_NETWORK_ERROR",
                f"Pexels network request failed: {error}",
                retryable=True,
                suggested_fallback="local-library",
            ) from error
        except OSError as error:
            raise PluginFailure(
                "PEXELS_NETWORK_ERROR",
                f"Pexels network request failed: {error}",
                retryable=True,
                suggested_fallback="local-library",
            ) from error

        try:
            data = json.loads(payload.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise PluginFailure(
                "PEXELS_INVALID_RESPONSE",
                f"Pexels returned invalid JSON: {error}",
                retryable=True,
                suggested_fallback="local-library",
            ) from error

        if not isinstance(data, dict):
            raise PluginFailure(
                "PEXELS_INVALID_RESPONSE",
                "Pexels response root must be a JSON object.",
                retryable=True,
                suggested_fallback="local-library",
            )

        return PexelsResponse(data=data, quota=quota_from_headers(headers))


def map_http_error(error: HTTPError) -> PluginFailure:
    retry_after = parse_optional_positive_int(
        error.headers.get("Retry-After") if error.headers is not None else None
    )

    if error.code == 401:
        return PluginFailure(
            "PEXELS_UNAUTHORIZED",
            "Pexels rejected the configured API key.",
            retryable=False,
        )
    if error.code == 403:
        return PluginFailure(
            "PEXELS_FORBIDDEN",
            "Pexels denied access to the requested resource.",
            retryable=False,
        )
    if error.code == 429:
        return PluginFailure(
            "PEXELS_RATE_LIMITED",
            "Pexels API rate limit was exceeded.",
            retryable=True,
            retry_after_seconds=retry_after,
            suggested_fallback="local-library",
        )
    if 500 <= error.code <= 599:
        return PluginFailure(
            "PEXELS_UPSTREAM_ERROR",
            f"Pexels returned HTTP {error.code}.",
            retryable=True,
            retry_after_seconds=retry_after,
            suggested_fallback="local-library",
        )
    return PluginFailure(
        "PEXELS_HTTP_ERROR",
        f"Pexels returned HTTP {error.code}.",
        retryable=False,
    )


def quota_from_headers(headers: Any) -> dict[str, int | None]:
    return {
        "limit": parse_optional_int(headers.get("X-Ratelimit-Limit")),
        "remaining": parse_optional_int(headers.get("X-Ratelimit-Remaining")),
        "reset": parse_optional_int(headers.get("X-Ratelimit-Reset")),
    }


def parse_optional_int(value: Any) -> int | None:
    if value is None:
        return None
    try:
        return int(str(value))
    except (TypeError, ValueError):
        return None


def parse_optional_positive_int(value: Any) -> int | None:
    parsed = parse_optional_int(value)
    return parsed if parsed is not None and parsed >= 0 else None


class PluginState:
    def __init__(self) -> None:
        self.settings = dict(DEFAULT_SETTINGS)
        self.shutdown_requested = False

    def initialize(self, params: Any) -> dict[str, Any]:
        if params is None:
            params = {}
        if not isinstance(params, dict):
            raise PluginFailure(
                "INVALID_SETTINGS",
                "plugin.initialize params must be an object when present.",
            )

        raw_settings = params.get("settings", params)
        if raw_settings is None:
            raw_settings = {}
        self.settings = merge_settings(DEFAULT_SETTINGS, raw_settings)
        return {
            "plugin_id": PROVIDER_ID,
            "api_version": API_VERSION,
            "settings": public_settings(self.settings),
        }


def merge_settings(base: dict[str, Any], raw: Any) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise PluginFailure("INVALID_SETTINGS", "Pexels settings must be an object.")

    prohibited = sorted(
        key for key in raw if key.strip().lower() in PROHIBITED_SECRET_SETTING_KEYS
    )
    if prohibited:
        raise PluginFailure(
            "SECRET_SETTING_REJECTED",
            "Pexels secret values must stay machine-local. Configure the API key through "
            f"the environment variable named by api_key_env; rejected keys: {', '.join(prohibited)}.",
        )

    settings = dict(base)
    for key in DEFAULT_SETTINGS:
        if key in raw:
            settings[key] = raw[key]

    validate_settings(settings)
    return settings


def validate_settings(settings: dict[str, Any]) -> None:
    media_type = settings.get("media_type")
    if media_type not in SUPPORTED_MEDIA_TYPES:
        raise PluginFailure(
            "INVALID_SETTINGS",
            f"media_type must be one of {sorted(SUPPORTED_MEDIA_TYPES)}.",
        )

    per_query = settings.get("per_query")
    if type(per_query) is not int or not 1 <= per_query <= 80:
        raise PluginFailure(
            "INVALID_SETTINGS",
            "per_query must be an integer between 1 and 80.",
        )

    orientation = settings.get("orientation")
    if orientation not in SUPPORTED_ORIENTATIONS:
        raise PluginFailure(
            "INVALID_SETTINGS",
            f"orientation must be one of {sorted(SUPPORTED_ORIENTATIONS)}.",
        )

    minimum_size = settings.get("minimum_size")
    if minimum_size not in SUPPORTED_SIZES:
        raise PluginFailure(
            "INVALID_SETTINGS",
            f"minimum_size must be one of {sorted(SUPPORTED_SIZES)}.",
        )

    locale = settings.get("locale")
    if locale not in SUPPORTED_LOCALES:
        raise PluginFailure(
            "INVALID_SETTINGS",
            "locale is not supported by the Pexels search API.",
        )

    api_key_env = settings.get("api_key_env")
    if not isinstance(api_key_env, str) or not api_key_env.strip():
        raise PluginFailure(
            "INVALID_SETTINGS",
            "api_key_env must be a non-empty environment variable name.",
        )


def public_settings(settings: dict[str, Any]) -> dict[str, Any]:
    return {
        "media_type": settings["media_type"],
        "per_query": settings["per_query"],
        "orientation": settings["orientation"],
        "minimum_size": settings["minimum_size"],
        "locale": settings["locale"],
        "api_key_env": settings["api_key_env"],
    }


def health_result(state: PluginState) -> dict[str, Any]:
    env_name = state.settings["api_key_env"]
    credential_present = bool(os.environ.get(env_name, "").strip())
    return {
        "status": "ready" if credential_present else "needs_attention",
        "provider": PROVIDER_ID,
        "credential_env": env_name,
        "credential_present": credential_present,
        "network_host": PEXELS_HOST,
    }


def capabilities_result() -> dict[str, Any]:
    return {
        "types": ["visual"],
        "capabilities": ["stock_video", "stock_image", "preview_first_search"],
        "operations": ["visual.resolve"],
        "scene_types": ["literal", "emotional", "conceptual"],
    }


def execute_visual_resolve(
    payload: Any,
    state: PluginState,
    client_factory: Callable[[str], Any] = PexelsClient,
) -> dict[str, Any]:
    if not isinstance(payload, dict):
        raise PluginFailure("INVALID_REQUEST", "visual.resolve payload must be an object.")

    scene = payload.get("scene")
    if not isinstance(scene, dict):
        raise PluginFailure("INVALID_REQUEST", "visual.resolve requires payload.scene.")

    scene_id = require_non_empty_string(scene.get("id"), "scene.id")
    raw_queries = scene.get("search_queries")
    if not isinstance(raw_queries, list) or not raw_queries:
        raise PluginFailure(
            "INVALID_REQUEST",
            "scene.search_queries must be a non-empty array.",
        )
    if len(raw_queries) > 6:
        raise PluginFailure(
            "INVALID_REQUEST",
            "scene.search_queries must contain at most 6 entries.",
        )

    queries: list[str] = []
    seen_queries: set[str] = set()
    for value in raw_queries:
        query = require_non_empty_string(value, "scene.search_queries entry")
        normalized = " ".join(query.lower().split())
        if normalized in seen_queries:
            continue
        seen_queries.add(normalized)
        queries.append(query)

    raw_settings = payload.get("settings", {})
    settings = merge_settings(state.settings, raw_settings)
    media_type = payload.get("media_type", settings["media_type"])
    if media_type not in SUPPORTED_MEDIA_TYPES:
        raise PluginFailure(
            "INVALID_REQUEST",
            f"media_type must be one of {sorted(SUPPORTED_MEDIA_TYPES)}.",
        )

    orientation = resolved_orientation(settings["orientation"], scene.get("aspect_ratio"))
    api_key_env = settings["api_key_env"]
    api_key = os.environ.get(api_key_env, "").strip()
    if not api_key:
        raise PluginFailure(
            "CREDENTIAL_MISSING",
            f"Pexels API key is missing. Set machine-local environment variable {api_key_env}.",
            retryable=False,
            suggested_fallback="local-library",
        )

    client = client_factory(api_key)
    candidates: list[dict[str, Any]] = []
    seen_candidates: set[str] = set()
    last_quota: dict[str, int | None] = {
        "limit": None,
        "remaining": None,
        "reset": None,
    }

    for query in queries:
        if media_type in {"video", "both"}:
            response = client.search_videos(
                query,
                orientation=orientation,
                size=settings["minimum_size"],
                locale=settings["locale"],
                per_page=settings["per_query"],
            )
            last_quota = response.quota
            for raw_video in require_list(response.data.get("videos"), "videos"):
                candidate = normalize_video(scene_id, raw_video)
                if candidate["candidate_id"] not in seen_candidates:
                    seen_candidates.add(candidate["candidate_id"])
                    candidates.append(candidate)

        if media_type in {"image", "both"}:
            response = client.search_photos(
                query,
                orientation=orientation,
                size=settings["minimum_size"],
                locale=settings["locale"],
                per_page=settings["per_query"],
            )
            last_quota = response.quota
            for raw_photo in require_list(response.data.get("photos"), "photos"):
                candidate = normalize_photo(scene_id, raw_photo)
                if candidate["candidate_id"] not in seen_candidates:
                    seen_candidates.add(candidate["candidate_id"])
                    candidates.append(candidate)

    return {
        "provider": PROVIDER_ID,
        "scene_id": scene_id,
        "preview_only": True,
        "queries": queries,
        "media_type": media_type,
        "candidates": candidates,
        "quota": last_quota,
    }


def normalize_video(scene_id: str, raw: Any) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise PluginFailure("PEXELS_INVALID_RESPONSE", "Pexels video must be an object.")

    asset_id = require_provider_id(raw.get("id"), "video.id")
    width = optional_positive_int(raw.get("width"), "video.width")
    height = optional_positive_int(raw.get("height"), "video.height")
    duration = optional_positive_number(raw.get("duration"), "video.duration")
    source_page_url = optional_non_empty_string(raw.get("url"), "video.url")

    user = raw.get("user") if isinstance(raw.get("user"), dict) else {}
    creator_name = optional_non_empty_string(user.get("name"), "video.user.name")
    creator_url = optional_non_empty_string(user.get("url"), "video.user.url")

    previews: list[dict[str, Any]] = []
    primary_image = raw.get("image")
    if isinstance(primary_image, str) and primary_image.strip():
        previews.append(
            {
                "kind": "thumbnail",
                "url": primary_image.strip(),
                "width": None,
                "height": None,
                "duration": None,
            }
        )

    pictures = raw.get("video_pictures")
    if isinstance(pictures, list):
        for picture in pictures[:4]:
            if not isinstance(picture, dict):
                continue
            url = picture.get("picture")
            if isinstance(url, str) and url.strip():
                previews.append(
                    {
                        "kind": "image",
                        "url": url.strip(),
                        "width": None,
                        "height": None,
                        "duration": None,
                    }
                )

    if not previews:
        raise PluginFailure(
            "PEXELS_INVALID_RESPONSE",
            f"Pexels video {asset_id} has no preview frames.",
            retryable=True,
        )

    return {
        "candidate_id": f"pexels:video:{asset_id}",
        "scene_id": scene_id,
        "source_provider": PROVIDER_ID,
        "source_asset_id": asset_id,
        "selection_ref": f"pexels:video:{asset_id}",
        "media_type": "video",
        "title": None,
        "description": None,
        "tags": [],
        "source_page_url": source_page_url,
        "creator_name": creator_name,
        "creator_url": creator_url,
        "width": width,
        "height": height,
        "duration": duration,
        "previews": previews,
    }


def normalize_photo(scene_id: str, raw: Any) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise PluginFailure("PEXELS_INVALID_RESPONSE", "Pexels photo must be an object.")

    asset_id = require_provider_id(raw.get("id"), "photo.id")
    width = optional_positive_int(raw.get("width"), "photo.width")
    height = optional_positive_int(raw.get("height"), "photo.height")
    source_page_url = optional_non_empty_string(raw.get("url"), "photo.url")
    creator_name = optional_non_empty_string(raw.get("photographer"), "photo.photographer")
    creator_url = optional_non_empty_string(raw.get("photographer_url"), "photo.photographer_url")
    description = optional_non_empty_string(raw.get("alt"), "photo.alt")

    src = raw.get("src")
    if not isinstance(src, dict):
        raise PluginFailure(
            "PEXELS_INVALID_RESPONSE",
            f"Pexels photo {asset_id} has no src preview object.",
            retryable=True,
        )

    preview_url = None
    for key in ("medium", "small", "large"):
        value = src.get(key)
        if isinstance(value, str) and value.strip():
            preview_url = value.strip()
            break
    if preview_url is None:
        raise PluginFailure(
            "PEXELS_INVALID_RESPONSE",
            f"Pexels photo {asset_id} has no safe preview URL.",
            retryable=True,
        )

    return {
        "candidate_id": f"pexels:image:{asset_id}",
        "scene_id": scene_id,
        "source_provider": PROVIDER_ID,
        "source_asset_id": asset_id,
        "selection_ref": f"pexels:image:{asset_id}",
        "media_type": "image",
        "title": description,
        "description": description,
        "tags": [],
        "source_page_url": source_page_url,
        "creator_name": creator_name,
        "creator_url": creator_url,
        "width": width,
        "height": height,
        "duration": None,
        "previews": [
            {
                "kind": "image",
                "url": preview_url,
                "width": None,
                "height": None,
                "duration": None,
            }
        ],
    }


def resolved_orientation(configured: str, aspect_ratio: Any) -> str | None:
    if configured != "auto":
        return configured

    if not isinstance(aspect_ratio, str) or ":" not in aspect_ratio:
        return None
    left, right = aspect_ratio.split(":", 1)
    try:
        width = float(left)
        height = float(right)
    except ValueError:
        return None
    if width <= 0 or height <= 0:
        return None
    ratio = width / height
    if 0.9 <= ratio <= 1.1:
        return "square"
    return "landscape" if ratio > 1.0 else "portrait"


def handle_request(
    request: Any,
    state: PluginState,
    client_factory: Callable[[str], Any] = PexelsClient,
) -> tuple[dict[str, Any], bool]:
    request_id = "unknown"
    try:
        if not isinstance(request, dict):
            raise PluginFailure("INVALID_REQUEST", "Plugin request must be a JSON object.")

        raw_request_id = request.get("request_id")
        if isinstance(raw_request_id, str) and raw_request_id.strip():
            request_id = raw_request_id.strip()
        else:
            raise PluginFailure("INVALID_REQUEST", "request_id must be a non-empty string.")

        if request.get("api_version") != API_VERSION:
            raise PluginFailure(
                "PLUGIN_API_INCOMPATIBLE",
                f"api_version must be {API_VERSION}.",
            )

        method = require_non_empty_string(request.get("method"), "method")
        params = request.get("params")

        if method == "plugin.initialize":
            result = state.initialize(params)
        elif method == "plugin.health":
            result = health_result(state)
        elif method == "plugin.capabilities":
            result = capabilities_result()
        elif method == "plugin.execute":
            if not isinstance(params, dict):
                raise PluginFailure(
                    "INVALID_REQUEST",
                    "plugin.execute params must be an object.",
                )
            operation = require_non_empty_string(params.get("operation"), "operation")
            if operation != "visual.resolve":
                raise PluginFailure(
                    "UNSUPPORTED_OPERATION",
                    f"Unsupported Pexels operation: {operation}.",
                )
            result = execute_visual_resolve(
                params.get("payload"),
                state,
                client_factory=client_factory,
            )
        elif method == "plugin.cancel":
            result = {
                "cancelled": False,
                "reason": "Pexels requests are synchronous; runtime termination is the cancellation fallback.",
            }
        elif method == "plugin.shutdown":
            state.shutdown_requested = True
            result = {"shutdown": True}
        else:
            raise PluginFailure(
                "UNSUPPORTED_METHOD",
                f"Unsupported Plugin API method: {method}.",
            )

        return success_response(request_id, result), state.shutdown_requested
    except PluginFailure as error:
        return failure_response(request_id, error), False
    except Exception as error:  # defensive process boundary
        return (
            failure_response(
                request_id,
                PluginFailure(
                    "PEXELS_INTERNAL_ERROR",
                    f"Unexpected Pexels plugin error: {error}",
                    retryable=False,
                ),
            ),
            False,
        )


def success_response(request_id: str, result: Any) -> dict[str, Any]:
    return {
        "api_version": API_VERSION,
        "request_id": request_id,
        "result": result,
    }


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


def require_non_empty_string(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise PluginFailure("INVALID_REQUEST", f"{label} must be a non-empty string.")
    return value.strip()


def require_provider_id(value: Any, label: str) -> str:
    if isinstance(value, bool) or not isinstance(value, (int, str)):
        raise PluginFailure(
            "PEXELS_INVALID_RESPONSE",
            f"{label} must be a provider identifier.",
            retryable=True,
        )
    result = str(value).strip()
    if not result:
        raise PluginFailure(
            "PEXELS_INVALID_RESPONSE",
            f"{label} must not be empty.",
            retryable=True,
        )
    return result


def optional_positive_int(value: Any, label: str) -> int | None:
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise PluginFailure(
            "PEXELS_INVALID_RESPONSE",
            f"{label} must be a positive integer when present.",
            retryable=True,
        )
    return value


def optional_positive_number(value: Any, label: str) -> float | None:
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, (int, float)) or value <= 0:
        raise PluginFailure(
            "PEXELS_INVALID_RESPONSE",
            f"{label} must be positive when present.",
            retryable=True,
        )
    return float(value)


def optional_non_empty_string(value: Any, label: str) -> str | None:
    if value is None:
        return None
    if not isinstance(value, str) or not value.strip():
        raise PluginFailure(
            "PEXELS_INVALID_RESPONSE",
            f"{label} must be a non-empty string when present.",
            retryable=True,
        )
    return value.strip()


def require_list(value: Any, label: str) -> list[Any]:
    if not isinstance(value, list):
        raise PluginFailure(
            "PEXELS_INVALID_RESPONSE",
            f"Pexels response {label} must be an array.",
            retryable=True,
        )
    return value


def run_jsonl() -> int:
    state = PluginState()

    for raw_line in sys.stdin:
        line = raw_line.strip()
        if not line:
            continue

        try:
            request = json.loads(line)
        except json.JSONDecodeError as error:
            response = failure_response(
                "unknown",
                PluginFailure(
                    "INVALID_JSON",
                    f"Request is not valid JSON: {error}",
                    retryable=False,
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
