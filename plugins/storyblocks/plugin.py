#!/usr/bin/env python3
"""OmniCreator Storyblocks VisualProvider plugin.

Plugin API v1 JSONL adapter. Search is preview-first. Storyblocks test keys may
be used for search/preview, but selected production output is deliberately
blocked unless STORYBLOCKS_API_MODE (or the configured env name) is
"production". This prevents test-only downloads from becoming durable
ArtifactStore assets.
"""

from __future__ import annotations

import hashlib
import hmac
import json
import os
import socket
import sys
import time
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any, Callable
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode, urlparse
from urllib.request import Request, urlopen

API_VERSION = 1
PROVIDER_ID = "storyblocks"
STORYBLOCKS_API_BASE = "https://api.storyblocks.com"
STORYBLOCKS_API_HOST = "api.storyblocks.com"
VIDEO_CDN_HOST = "d2v9y0dukr6mq2.cloudfront.net"
IMAGE_CDN_HOST = "d1yn1kh78jj1rr.cloudfront.net"
ALLOWED_MEDIA_HOSTS = {VIDEO_CDN_HOST, IMAGE_CDN_HOST}
REQUEST_TIMEOUT_SECONDS = 20
AUTH_TTL_SECONDS = 100

SUPPORTED_MEDIA_TYPES = {"video", "image", "both"}
SUPPORTED_ORIENTATIONS = {"auto", "all", "horizontal", "vertical"}
SUPPORTED_SORTS = {
    "most_relevant",
    "most_downloaded",
    "most_recent",
    "trending_now",
    "undiscovered",
}
SUPPORTED_QUALITY_MODES = {"standard", "high"}
SUPPORTED_API_MODES = {"test", "production"}

DEFAULT_SETTINGS: dict[str, Any] = {
    "media_type": "video",
    "per_query": 8,
    "orientation": "auto",
    "safe_search": True,
    "sort_by": "most_relevant",
    "public_key_env": "STORYBLOCKS_PUBLIC_KEY",
    "private_key_env": "STORYBLOCKS_PRIVATE_KEY",
    "user_id_env": "STORYBLOCKS_USER_ID",
    "api_mode_env": "STORYBLOCKS_API_MODE",
}

PROHIBITED_SECRET_SETTING_KEYS = {
    "public_key",
    "private_key",
    "api_key",
    "apikey",
    "key",
    "token",
    "secret",
    "password",
    "authorization",
    "user_id",
    "api_mode",
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
class StoryblocksResponse:
    data: dict[str, Any]
    quota: dict[str, int | None]


class StoryblocksClient:
    def __init__(
        self,
        public_key: str,
        private_key: str,
        user_id: str,
        opener: Callable[..., Any] = urlopen,
        api_base: str = STORYBLOCKS_API_BASE,
        timeout_seconds: int = REQUEST_TIMEOUT_SECONDS,
        clock: Callable[[], float] = time.time,
    ) -> None:
        self._public_key = require_machine_value(public_key, "Storyblocks public key")
        self._private_key = require_machine_value(private_key, "Storyblocks private key")
        self._user_id = require_machine_value(user_id, "Storyblocks user id")
        self._opener = opener
        self._api_base = api_base.rstrip("/")
        self._timeout_seconds = timeout_seconds
        self._clock = clock

    def search_videos(
        self,
        project_id: str,
        keywords: str,
        *,
        orientation: str | None,
        safe_search: bool,
        sort_by: str,
        results_per_page: int,
    ) -> StoryblocksResponse:
        params: dict[str, Any] = {
            "project_id": project_id,
            "user_id": self._user_id,
            "keywords": keywords,
            "page": 1,
            "results_per_page": results_per_page,
            "sort_by": sort_by,
            "sort_order": "DESC",
            "safe_search": boolean_query(safe_search),
            "extended": (
                "download_formats,keywords,description,isEditorial,"
                "hasTalentReleased,hasPropertyReleased"
            ),
        }
        if orientation is not None:
            params["orientation"] = video_orientation(orientation)
        return self._get_json("/api/v2/videos/search", params, "video search")

    def search_images(
        self,
        project_id: str,
        keywords: str,
        *,
        orientation: str | None,
        safe_search: bool,
        sort_by: str,
        results_per_page: int,
    ) -> StoryblocksResponse:
        params: dict[str, Any] = {
            "project_id": project_id,
            "user_id": self._user_id,
            "keywords": keywords,
            "content_type": "all",
            "is_editorial": "false",
            "page": 1,
            "results_per_page": results_per_page,
            "sort_by": sort_by,
            "sort_order": "DESC",
            "safe_search": boolean_query(safe_search),
            "extended": (
                "download_formats,keywords,description,isEditorial,"
                "hasTalentReleased,hasPropertyReleased,aspectRatio"
            ),
        }
        if orientation is not None:
            params["orientation"] = image_orientation(orientation)
        return self._get_json("/api/v2/images/search", params, "image search")

    def get_video_details(self, asset_id: str) -> StoryblocksResponse:
        return self._get_json(
            f"/api/v2/videos/stock-item/details/{asset_id}",
            {},
            "video details",
        )

    def get_image_details(self, asset_id: str) -> StoryblocksResponse:
        return self._get_json(
            f"/api/v2/images/stock-item/details/{asset_id}",
            {},
            "image details",
        )

    def get_video_download_links(
        self,
        project_id: str,
        asset_id: str,
    ) -> StoryblocksResponse:
        return self._get_json(
            f"/api/v2/videos/stock-item/download/{asset_id}",
            {"project_id": project_id, "user_id": self._user_id},
            "video selected download",
        )

    def get_image_download_links(
        self,
        project_id: str,
        asset_id: str,
    ) -> StoryblocksResponse:
        return self._get_json(
            f"/api/v2/images/stock-item/download/{asset_id}",
            {"project_id": project_id, "user_id": self._user_id},
            "image selected download",
        )

    def download_to_path(self, url: str, destination: Path) -> None:
        media_url = validate_media_url(url)
        request = Request(
            media_url,
            method="GET",
            headers={
                "Accept": "*/*",
                "User-Agent": "OmniCreator-Storyblocks/1.0",
            },
        )
        try:
            with self._opener(request, timeout=self._timeout_seconds) as response:
                with destination.open("wb") as output:
                    while True:
                        chunk = response.read(1024 * 1024)
                        if not chunk:
                            break
                        output.write(chunk)
        except HTTPError as error:
            raise map_http_error(error, "media transfer") from error
        except (URLError, TimeoutError, socket.timeout, OSError) as error:
            raise PluginFailure(
                "STORYBLOCKS_DOWNLOAD_ERROR",
                f"Storyblocks media transfer failed: {error}",
                retryable=True,
                suggested_fallback="retry-selected-asset",
            ) from error

        if not destination.is_file() or destination.stat().st_size == 0:
            raise PluginFailure(
                "STORYBLOCKS_EMPTY_DOWNLOAD",
                "Storyblocks media transfer produced an empty file.",
                retryable=True,
                suggested_fallback="retry-selected-asset",
            )

    def signed_request_url(self, resource: str, params: dict[str, Any]) -> str:
        validate_resource(resource)
        expires = str(int(self._clock()) + AUTH_TTL_SECONDS)
        signing_key = (self._private_key + expires).encode("utf-8")
        digest = hmac.new(
            signing_key,
            resource.encode("utf-8"),
            hashlib.sha256,
        ).hexdigest()
        query = urlencode(
            {
                "APIKEY": self._public_key,
                "EXPIRES": expires,
                "HMAC": digest,
                **params,
            }
        )
        return f"{self._api_base}{resource}?{query}"

    def _get_json(
        self,
        resource: str,
        params: dict[str, Any],
        operation: str,
    ) -> StoryblocksResponse:
        url = self.signed_request_url(resource, params)
        parsed = urlparse(url)
        if parsed.scheme != "https" or (parsed.hostname or "").lower() != STORYBLOCKS_API_HOST:
            raise PluginFailure(
                "STORYBLOCKS_API_HOST_NOT_ALLOWED",
                "Storyblocks API requests must use https://api.storyblocks.com.",
            )

        request = Request(
            url,
            method="GET",
            headers={
                "Accept": "application/json",
                "User-Agent": "OmniCreator-Storyblocks/1.0",
            },
        )
        try:
            with self._opener(request, timeout=self._timeout_seconds) as response:
                payload = response.read()
                headers = response.headers
        except HTTPError as error:
            raise map_http_error(error, operation) from error
        except (URLError, TimeoutError, socket.timeout, OSError) as error:
            raise PluginFailure(
                "STORYBLOCKS_NETWORK_ERROR",
                f"Storyblocks {operation} request failed: {error}",
                retryable=True,
                suggested_fallback="local-library",
            ) from error

        data = decode_json_object(payload, operation)
        return StoryblocksResponse(data=data, quota=quota_from_headers(headers))


def validate_resource(resource: str) -> None:
    if (
        not isinstance(resource, str)
        or not resource.startswith("/api/v2/")
        or "?" in resource
        or "#" in resource
        or ".." in resource
    ):
        raise PluginFailure(
            "STORYBLOCKS_INVALID_RESOURCE",
            "Storyblocks signed resource path is invalid.",
        )


def decode_json_object(payload: bytes, label: str) -> dict[str, Any]:
    try:
        data = json.loads(payload.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise PluginFailure(
            "STORYBLOCKS_INVALID_RESPONSE",
            f"Storyblocks {label} response was invalid JSON: {error}",
            retryable=True,
            suggested_fallback="local-library",
        ) from error
    if not isinstance(data, dict):
        raise PluginFailure(
            "STORYBLOCKS_INVALID_RESPONSE",
            f"Storyblocks {label} response root must be an object.",
            retryable=True,
            suggested_fallback="local-library",
        )
    return data


def map_http_error(error: HTTPError, operation: str) -> PluginFailure:
    retry_after = parse_optional_positive_int(
        error.headers.get("Retry-After") if error.headers is not None else None
    )
    if error.code in {401, 403}:
        return PluginFailure(
            "STORYBLOCKS_UNAUTHORIZED",
            f"Storyblocks rejected the configured API credentials during {operation}.",
            retryable=False,
        )
    if error.code == 404:
        return PluginFailure(
            "STORYBLOCKS_ASSET_NOT_FOUND",
            f"Storyblocks could not find the requested asset during {operation}.",
            retryable=False,
            suggested_fallback="next-stock-candidate",
        )
    if error.code == 429:
        return PluginFailure(
            "STORYBLOCKS_RATE_LIMITED",
            f"Storyblocks rate limit was exceeded during {operation}.",
            retryable=True,
            retry_after_seconds=retry_after,
            suggested_fallback="local-library",
        )
    if 500 <= error.code <= 599:
        return PluginFailure(
            "STORYBLOCKS_UPSTREAM_ERROR",
            f"Storyblocks returned HTTP {error.code} during {operation}.",
            retryable=True,
            retry_after_seconds=retry_after,
            suggested_fallback="local-library",
        )
    return PluginFailure(
        "STORYBLOCKS_HTTP_ERROR",
        f"Storyblocks returned HTTP {error.code} during {operation}.",
        retryable=False,
    )


def quota_from_headers(headers: Any) -> dict[str, int | None]:
    return {
        "limit": first_header_int(
            headers,
            ("X-RateLimit-Limit", "X-Ratelimit-Limit"),
        ),
        "remaining": first_header_int(
            headers,
            ("X-RateLimit-Remaining", "X-Ratelimit-Remaining"),
        ),
        "reset": first_header_int(
            headers,
            ("X-RateLimit-Reset", "X-Ratelimit-Reset"),
        ),
    }


def first_header_int(headers: Any, names: tuple[str, ...]) -> int | None:
    if headers is None:
        return None
    for name in names:
        value = headers.get(name)
        parsed = parse_optional_int(value)
        if parsed is not None:
            return parsed
    return None


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


def boolean_query(value: bool) -> str:
    return "true" if value else "false"


class PluginState:
    def __init__(self) -> None:
        self.settings = dict(DEFAULT_SETTINGS)
        self.job_workspace: dict[str, str] | None = None
        self.shutdown_requested = False

    def initialize(self, params: Any) -> dict[str, Any]:
        if params is None:
            params = {}
        if not isinstance(params, dict):
            raise PluginFailure(
                "INVALID_SETTINGS",
                "plugin.initialize params must be an object when present.",
            )

        if "job_workspace" in params:
            self.job_workspace = validate_job_workspace(params.get("job_workspace"))

        if any(key in params for key in ("settings", "job_workspace", "permissions")):
            raw_settings = params.get("settings", {})
        else:
            raw_settings = params
        if raw_settings is None:
            raw_settings = {}

        self.settings = merge_settings(DEFAULT_SETTINGS, raw_settings)
        readiness = machine_readiness(self.settings)
        return {
            "plugin_id": PROVIDER_ID,
            "api_version": API_VERSION,
            "settings": public_settings(self.settings),
            "workspace_ready": self.job_workspace is not None,
            "credential_ready": readiness["credential_ready"],
            "user_id_ready": readiness["user_id_ready"],
            "api_mode": readiness["api_mode"],
            "production_download_ready": readiness["production_download_ready"],
        }


def merge_settings(base: dict[str, Any], raw: Any) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise PluginFailure("INVALID_SETTINGS", "Storyblocks settings must be an object.")

    prohibited = sorted(
        key for key in raw if key.strip().lower() in PROHIBITED_SECRET_SETTING_KEYS
    )
    if prohibited:
        raise PluginFailure(
            "SECRET_SETTING_REJECTED",
            "Storyblocks credentials, user id and API mode are machine-local. "
            "Configure only their environment-variable names; rejected keys: "
            + ", ".join(prohibited)
            + ".",
        )

    settings = dict(base)
    for key in DEFAULT_SETTINGS:
        if key in raw:
            settings[key] = raw[key]
    validate_settings(settings)
    return settings


def validate_settings(settings: dict[str, Any]) -> None:
    if settings.get("media_type") not in SUPPORTED_MEDIA_TYPES:
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
    if settings.get("orientation") not in SUPPORTED_ORIENTATIONS:
        raise PluginFailure(
            "INVALID_SETTINGS",
            f"orientation must be one of {sorted(SUPPORTED_ORIENTATIONS)}.",
        )
    if type(settings.get("safe_search")) is not bool:
        raise PluginFailure("INVALID_SETTINGS", "safe_search must be a boolean.")
    if settings.get("sort_by") not in SUPPORTED_SORTS:
        raise PluginFailure(
            "INVALID_SETTINGS",
            f"sort_by must be one of {sorted(SUPPORTED_SORTS)}.",
        )
    for key in (
        "public_key_env",
        "private_key_env",
        "user_id_env",
        "api_mode_env",
    ):
        value = settings.get(key)
        if not isinstance(value, str) or not value.strip():
            raise PluginFailure(
                "INVALID_SETTINGS",
                f"{key} must be a non-empty environment variable name.",
            )


def public_settings(settings: dict[str, Any]) -> dict[str, Any]:
    return {key: settings[key] for key in DEFAULT_SETTINGS}


def machine_readiness(settings: dict[str, Any]) -> dict[str, Any]:
    public_key = os.environ.get(settings["public_key_env"], "").strip()
    private_key = os.environ.get(settings["private_key_env"], "").strip()
    user_id = os.environ.get(settings["user_id_env"], "").strip()
    api_mode_raw = os.environ.get(settings["api_mode_env"], "test").strip().lower()
    api_mode = api_mode_raw if api_mode_raw in SUPPORTED_API_MODES else "invalid"
    credential_ready = bool(public_key and private_key)
    user_id_ready = bool(user_id)
    return {
        "credential_ready": credential_ready,
        "user_id_ready": user_id_ready,
        "api_mode": api_mode,
        "production_download_ready": (
            credential_ready and user_id_ready and api_mode == "production"
        ),
    }


def health_result(state: PluginState) -> dict[str, Any]:
    readiness = machine_readiness(state.settings)
    search_ready = readiness["credential_ready"] and readiness["user_id_ready"]
    return {
        "status": "ready" if search_ready else "needs_attention",
        "provider": PROVIDER_ID,
        "credential_ready": readiness["credential_ready"],
        "user_id_ready": readiness["user_id_ready"],
        "api_mode": readiness["api_mode"],
        "production_download_ready": readiness["production_download_ready"],
        "public_key_env": state.settings["public_key_env"],
        "private_key_env": state.settings["private_key_env"],
        "user_id_env": state.settings["user_id_env"],
        "api_mode_env": state.settings["api_mode_env"],
        "network_host": STORYBLOCKS_API_HOST,
    }


def capabilities_result() -> dict[str, Any]:
    return {
        "types": ["visual"],
        "capabilities": [
            "stock_video",
            "stock_image",
            "preview_first_search",
            "selected_asset_download",
            "license_gated_download",
        ],
        "operations": ["visual.resolve", "visual.fetch_selected"],
        "scene_types": ["literal", "emotional", "conceptual"],
    }


def machine_credentials(
    settings: dict[str, Any],
) -> tuple[str, str, str, str]:
    public_key = os.environ.get(settings["public_key_env"], "").strip()
    private_key = os.environ.get(settings["private_key_env"], "").strip()
    user_id = os.environ.get(settings["user_id_env"], "").strip()
    api_mode = os.environ.get(settings["api_mode_env"], "test").strip().lower()

    if not public_key or not private_key:
        raise PluginFailure(
            "CREDENTIAL_MISSING",
            "Storyblocks public/private API credentials are missing from the configured "
            "machine-local environment variables.",
            retryable=False,
            suggested_fallback="local-library",
        )
    if not user_id:
        raise PluginFailure(
            "STORYBLOCKS_USER_ID_MISSING",
            "Storyblocks requires a stable machine-local end-user identifier.",
            retryable=False,
            suggested_fallback="local-library",
        )
    if api_mode not in SUPPORTED_API_MODES:
        raise PluginFailure(
            "STORYBLOCKS_API_MODE_INVALID",
            "Storyblocks API mode must be 'test' or 'production'.",
            retryable=False,
        )
    return public_key, private_key, user_id, api_mode


def execute_visual_resolve(
    payload: Any,
    state: PluginState,
    client_factory: Callable[[str, str, str], Any] = StoryblocksClient,
) -> dict[str, Any]:
    if not isinstance(payload, dict):
        raise PluginFailure("INVALID_REQUEST", "visual.resolve payload must be an object.")

    project_id = validate_project_id(payload.get("project_id"))
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
        query = " ".join(require_non_empty_string(
            value, "scene.search_queries entry"
        ).split())
        normalized = query.lower()
        if normalized in seen_queries:
            continue
        seen_queries.add(normalized)
        queries.append(query)

    settings = merge_settings(state.settings, payload.get("settings", {}))
    media_type = payload.get("media_type", settings["media_type"])
    if media_type not in SUPPORTED_MEDIA_TYPES:
        raise PluginFailure(
            "INVALID_REQUEST",
            f"media_type must be one of {sorted(SUPPORTED_MEDIA_TYPES)}.",
        )

    public_key, private_key, user_id, api_mode = machine_credentials(settings)
    client = client_factory(public_key, private_key, user_id)
    orientation = resolved_orientation(settings["orientation"], scene.get("aspect_ratio"))

    candidates: list[dict[str, Any]] = []
    seen_candidates: set[str] = set()
    last_quota = {"limit": None, "remaining": None, "reset": None}

    for query in queries:
        if media_type in {"video", "both"}:
            response = client.search_videos(
                project_id,
                query,
                orientation=orientation,
                safe_search=settings["safe_search"],
                sort_by=settings["sort_by"],
                results_per_page=settings["per_query"],
            )
            last_quota = response.quota
            for raw_video in require_results(response.data):
                candidate = normalize_video(scene_id, raw_video)
                if candidate["candidate_id"] not in seen_candidates:
                    seen_candidates.add(candidate["candidate_id"])
                    candidates.append(candidate)

        if media_type in {"image", "both"}:
            response = client.search_images(
                project_id,
                query,
                orientation=orientation,
                safe_search=settings["safe_search"],
                sort_by=settings["sort_by"],
                results_per_page=settings["per_query"],
            )
            last_quota = response.quota
            for raw_image in require_results(response.data):
                candidate = normalize_image(scene_id, raw_image)
                if candidate["candidate_id"] not in seen_candidates:
                    seen_candidates.add(candidate["candidate_id"])
                    candidates.append(candidate)

    return {
        "provider": PROVIDER_ID,
        "project_id": project_id,
        "scene_id": scene_id,
        "preview_only": True,
        "queries": queries,
        "media_type": media_type,
        "api_mode": api_mode,
        "production_download_ready": api_mode == "production",
        "candidates": candidates,
        "quota": last_quota,
    }


def normalize_video(scene_id: str, raw: Any) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise PluginFailure(
            "STORYBLOCKS_INVALID_RESPONSE",
            "Storyblocks video result must be an object.",
            retryable=True,
        )
    asset_id = require_provider_id(raw.get("id"), "video.id")
    title = optional_text(raw.get("title"))
    description = optional_text(raw.get("description"))
    duration = optional_positive_number(raw.get("duration"), "video.duration")
    tags = normalize_string_list(raw.get("keywords"))
    creator = contributor_name(raw)

    previews: list[dict[str, Any]] = []
    thumbnail = raw.get("thumbnail_url")
    if isinstance(thumbnail, str) and thumbnail.strip():
        previews.append(
            {
                "kind": "thumbnail",
                "url": validate_media_url(thumbnail.strip()),
                "width": None,
                "height": None,
                "duration": None,
            }
        )
    preview_urls = raw.get("preview_urls")
    if isinstance(preview_urls, dict):
        for key in ("_360p", "_480p", "_720p", "_180p"):
            value = preview_urls.get(key)
            if isinstance(value, str) and value.strip():
                previews.append(
                    {
                        "kind": "video",
                        "url": validate_media_url(value.strip()),
                        "width": None,
                        "height": resolution_height(key),
                        "duration": duration,
                    }
                )
                break
    if not previews:
        raise PluginFailure(
            "STORYBLOCKS_INVALID_RESPONSE",
            f"Storyblocks video {asset_id} has no preview URL.",
            retryable=True,
        )

    width, height = preferred_dimensions_from_details(raw, "video", "standard")
    return {
        "candidate_id": f"storyblocks:video:{asset_id}",
        "scene_id": scene_id,
        "source_provider": PROVIDER_ID,
        "source_asset_id": asset_id,
        "selection_ref": f"storyblocks:video:{asset_id}",
        "media_type": "video",
        "title": title,
        "description": description,
        "tags": tags,
        "source_page_url": None,
        "creator_name": creator,
        "creator_url": None,
        "width": width,
        "height": height,
        "duration": duration,
        "previews": previews,
    }


def normalize_image(scene_id: str, raw: Any) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise PluginFailure(
            "STORYBLOCKS_INVALID_RESPONSE",
            "Storyblocks image result must be an object.",
            retryable=True,
        )
    asset_id = require_provider_id(raw.get("id"), "image.id")
    title = optional_text(raw.get("title"))
    description = optional_text(raw.get("description"))
    tags = normalize_string_list(raw.get("keywords"))
    creator = contributor_name(raw)

    preview_value = raw.get("preview_url") or raw.get("thumbnail_url")
    preview = validate_media_url(
        require_non_empty_string(preview_value, "image preview URL")
    )
    width, height = preferred_dimensions_from_details(raw, "image", "standard")
    return {
        "candidate_id": f"storyblocks:image:{asset_id}",
        "scene_id": scene_id,
        "source_provider": PROVIDER_ID,
        "source_asset_id": asset_id,
        "selection_ref": f"storyblocks:image:{asset_id}",
        "media_type": "image",
        "title": title,
        "description": description,
        "tags": tags,
        "source_page_url": None,
        "creator_name": creator,
        "creator_url": None,
        "width": width,
        "height": height,
        "duration": None,
        "previews": [
            {
                "kind": "image",
                "url": preview,
                "width": None,
                "height": None,
                "duration": None,
            }
        ],
    }


def contributor_name(raw: dict[str, Any]) -> str | None:
    contributor = raw.get("contributor")
    if not isinstance(contributor, dict):
        return None
    return optional_text(contributor.get("username"))


def normalize_string_list(value: Any) -> list[str]:
    if value is None:
        return []
    if not isinstance(value, list):
        raise PluginFailure(
            "STORYBLOCKS_INVALID_RESPONSE",
            "Storyblocks keywords must be an array when present.",
            retryable=True,
        )
    result: list[str] = []
    seen: set[str] = set()
    for item in value:
        if not isinstance(item, str):
            continue
        normalized = " ".join(item.split())
        key = normalized.lower()
        if normalized and key not in seen:
            seen.add(key)
            result.append(normalized)
    return result[:30]


def preferred_dimensions_from_details(
    raw: dict[str, Any],
    media_type: str,
    quality_mode: str,
) -> tuple[int | None, int | None]:
    formats = raw.get("download_formats")
    if not isinstance(formats, dict):
        return None, None

    if media_type == "image":
        jpg = formats.get("JPG")
        if isinstance(jpg, dict):
            return (
                optional_positive_int(jpg.get("width"), "image JPG width"),
                optional_positive_int(jpg.get("height"), "image JPG height"),
            )
        return None, None

    mp4 = formats.get("MP4")
    if not isinstance(mp4, dict):
        return None, None
    selected_key = select_resolution_key(mp4.keys(), quality_mode)
    if selected_key is None:
        return None, None
    metadata = mp4.get(selected_key)
    if not isinstance(metadata, dict):
        return None, None
    return (
        optional_positive_int(metadata.get("width"), "video MP4 width"),
        optional_positive_int(metadata.get("height"), "video MP4 height"),
    )


def select_resolution_key(keys: Any, quality_mode: str) -> str | None:
    if quality_mode not in SUPPORTED_QUALITY_MODES:
        raise PluginFailure(
            "INVALID_REQUEST",
            f"quality_mode must be one of {sorted(SUPPORTED_QUALITY_MODES)}.",
        )
    candidates: list[tuple[int, str]] = []
    for raw_key in keys:
        if not isinstance(raw_key, str):
            continue
        height = resolution_height(raw_key)
        if height is not None:
            candidates.append((height, raw_key))
    if not candidates:
        return None
    candidates.sort()
    if quality_mode == "high":
        return candidates[-1][1]
    at_least_1080 = [item for item in candidates if item[0] >= 1080]
    if at_least_1080:
        return at_least_1080[0][1]
    return candidates[-1][1]


def resolution_height(key: str) -> int | None:
    normalized = key.strip().lower().lstrip("_")
    if not normalized.endswith("p"):
        return None
    number = normalized[:-1]
    try:
        value = int(number)
    except ValueError:
        return None
    return value if value > 0 else None


def validate_project_id(value: Any) -> str:
    project_id = require_non_empty_string(value, "project_id")
    if len(project_id) > 128 or any(
        character in project_id
        for character in ("/", "\\", "\n", "\r", "\t")
    ):
        raise PluginFailure(
            "INVALID_REQUEST",
            "project_id must be a portable canonical identifier, not a path.",
        )
    return project_id


def validate_job_workspace(raw: Any) -> dict[str, str]:
    if not isinstance(raw, dict):
        raise PluginFailure("INVALID_WORKSPACE", "job_workspace must be an object.")

    workspace: dict[str, str] = {}
    for key in ("output", "temp"):
        value = raw.get(key)
        if not isinstance(value, str) or not value.strip():
            raise PluginFailure(
                "INVALID_WORKSPACE",
                f"job_workspace.{key} must be a non-empty path.",
            )
        path = Path(value).expanduser()
        if not path.is_absolute():
            raise PluginFailure(
                "INVALID_WORKSPACE",
                f"job_workspace.{key} must be an absolute path.",
            )
        if not path.is_dir():
            raise PluginFailure(
                "INVALID_WORKSPACE",
                f"job_workspace.{key} does not exist or is not a directory.",
            )
        workspace[key] = str(path.resolve())
    return workspace


def parse_selection_ref(selection_ref: Any) -> tuple[str, str]:
    value = require_non_empty_string(selection_ref, "selection_ref")
    parts = value.split(":")
    if (
        len(parts) != 3
        or parts[0] != PROVIDER_ID
        or parts[1] not in {"video", "image"}
    ):
        raise PluginFailure(
            "INVALID_SELECTION_REF",
            "selection_ref must be storyblocks:video:<id> or storyblocks:image:<id>.",
        )
    asset_id = parts[2]
    if not asset_id.isdigit() or int(asset_id) <= 0:
        raise PluginFailure(
            "INVALID_SELECTION_REF",
            "Storyblocks stock item id must be a positive integer.",
        )
    return parts[1], asset_id


def validate_media_url(url: Any) -> str:
    value = require_non_empty_string(url, "media URL")
    parsed = urlparse(value)
    host = (parsed.hostname or "").lower()
    if parsed.scheme != "https" or host not in ALLOWED_MEDIA_HOSTS:
        raise PluginFailure(
            "STORYBLOCKS_MEDIA_HOST_NOT_ALLOWED",
            f"Storyblocks media URL must use HTTPS on a documented CDN host, found {host or 'unknown'}.",
            retryable=False,
        )
    return value


def select_video_download(
    details: dict[str, Any],
    links: dict[str, Any],
    quality_mode: str,
) -> dict[str, Any]:
    detail_formats = details.get("download_formats")
    if not isinstance(detail_formats, dict):
        raise PluginFailure(
            "STORYBLOCKS_NO_DOWNLOADABLE_VIDEO",
            "Storyblocks video details have no download_formats object.",
            retryable=True,
        )
    detail_mp4 = detail_formats.get("MP4")
    link_mp4 = links.get("MP4")
    if not isinstance(detail_mp4, dict) or not isinstance(link_mp4, dict):
        raise PluginFailure(
            "STORYBLOCKS_NO_DOWNLOADABLE_VIDEO",
            "Storyblocks selected video has no MP4 download variants.",
            retryable=True,
        )

    shared_keys = [key for key in detail_mp4 if key in link_mp4]
    selected_key = select_resolution_key(shared_keys, quality_mode)
    if selected_key is None:
        raise PluginFailure(
            "STORYBLOCKS_NO_DOWNLOADABLE_VIDEO",
            "Storyblocks selected video has no matching MP4 rendition.",
            retryable=True,
        )
    metadata = detail_mp4.get(selected_key)
    url = link_mp4.get(selected_key)
    if not isinstance(metadata, dict):
        raise PluginFailure(
            "STORYBLOCKS_INVALID_RESPONSE",
            "Storyblocks MP4 rendition metadata must be an object.",
            retryable=True,
        )
    return {
        "url": validate_media_url(url),
        "rendition": selected_key,
        "width": optional_positive_int(metadata.get("width"), "video rendition width"),
        "height": optional_positive_int(metadata.get("height"), "video rendition height"),
        "frame_rate": optional_positive_number(
            metadata.get("frame_rate"), "video rendition frame_rate"
        ),
        "file_size_bytes": optional_positive_int(
            metadata.get("file_size_bytes"), "video rendition file_size_bytes"
        ),
    }


def select_image_download(
    details: dict[str, Any],
    links: dict[str, Any],
) -> dict[str, Any]:
    url = links.get("JPG")
    if not isinstance(url, str) or not url.strip():
        raise PluginFailure(
            "STORYBLOCKS_NO_DOWNLOADABLE_IMAGE",
            "Storyblocks selected image has no JPG download link.",
            retryable=True,
        )

    width = None
    height = None
    formats = details.get("download_formats")
    if isinstance(formats, dict) and isinstance(formats.get("JPG"), dict):
        metadata = formats["JPG"]
        width = optional_positive_int(metadata.get("width"), "image JPG width")
        height = optional_positive_int(metadata.get("height"), "image JPG height")

    return {
        "url": validate_media_url(url),
        "format": "JPG",
        "width": width,
        "height": height,
    }


def output_paths_for_selection(
    state: PluginState,
    media_type: str,
    asset_id: str,
    extension: str,
) -> tuple[str, Path, Path]:
    if state.job_workspace is None:
        raise PluginFailure(
            "WORKSPACE_REQUIRED",
            "visual.fetch_selected requires plugin.initialize with a job_workspace.",
            retryable=False,
        )

    relative_output = f"selected/storyblocks-{media_type}-{asset_id}.{extension}"
    relative = PurePosixPath(relative_output)
    if relative.is_absolute() or any(part in {"", ".", ".."} for part in relative.parts):
        raise PluginFailure("INVALID_OUTPUT_PATH", "selected asset output path is invalid.")

    output_root = Path(state.job_workspace["output"]).resolve()
    temp_root = Path(state.job_workspace["temp"]).resolve()
    destination = output_root.joinpath(*relative.parts)
    destination.parent.mkdir(parents=True, exist_ok=True)
    resolved_parent = destination.parent.resolve()
    if output_root != resolved_parent and output_root not in resolved_parent.parents:
        raise PluginFailure(
            "INVALID_OUTPUT_PATH",
            "selected asset output escaped the job workspace.",
        )

    temp_path = temp_root / f"storyblocks-{media_type}-{asset_id}.{extension}.part"
    return relative_output, destination, temp_path


def execute_fetch_selected(
    payload: Any,
    state: PluginState,
    client_factory: Callable[[str, str, str], Any] = StoryblocksClient,
) -> dict[str, Any]:
    if not isinstance(payload, dict):
        raise PluginFailure(
            "INVALID_REQUEST",
            "visual.fetch_selected payload must be an object.",
        )
    if state.job_workspace is None:
        raise PluginFailure(
            "WORKSPACE_REQUIRED",
            "visual.fetch_selected requires plugin.initialize with a job_workspace.",
            retryable=False,
        )

    project_id = validate_project_id(payload.get("project_id"))
    selection_ref = require_non_empty_string(payload.get("selection_ref"), "selection_ref")
    media_type, asset_id = parse_selection_ref(selection_ref)
    quality_mode = payload.get("quality_mode", "standard")
    if quality_mode not in SUPPORTED_QUALITY_MODES:
        raise PluginFailure(
            "INVALID_REQUEST",
            f"quality_mode must be one of {sorted(SUPPORTED_QUALITY_MODES)}.",
        )

    public_key, private_key, user_id, api_mode = machine_credentials(state.settings)
    if api_mode != "production":
        raise PluginFailure(
            "STORYBLOCKS_PRODUCTION_ACCESS_REQUIRED",
            "Storyblocks test credentials are restricted to API testing. "
            "A promotable selected asset requires STORYBLOCKS_API_MODE=production "
            "with production API credentials.",
            retryable=False,
            suggested_fallback="next-stock-provider",
        )

    client = client_factory(public_key, private_key, user_id)

    if media_type == "video":
        details_response = client.get_video_details(asset_id)
        details = details_response.data
        verify_detail_id(details, asset_id, "video")
        links = client.get_video_download_links(project_id, asset_id).data
        selected = select_video_download(details, links, quality_mode)
        relative_output, destination, temp_path = output_paths_for_selection(
            state, "video", asset_id, "mp4"
        )
        client.download_to_path(selected["url"], temp_path)
        os.replace(temp_path, destination)

        duration = optional_positive_number(details.get("duration"), "video.duration")
        return {
            "source_provider": PROVIDER_ID,
            "source_asset_id": asset_id,
            "selection_ref": selection_ref,
            "media_type": "video",
            "relative_output": relative_output,
            "width": selected["width"],
            "height": selected["height"],
            "duration": duration,
            "provenance": selected_provenance(
                details,
                project_id,
                asset_id,
                "video",
                {
                    "download_format": "MP4",
                    "provider_rendition": selected["rendition"],
                    "frame_rate": selected["frame_rate"],
                    "file_size_bytes": selected["file_size_bytes"],
                },
            ),
        }

    details_response = client.get_image_details(asset_id)
    details = details_response.data
    verify_detail_id(details, asset_id, "image")
    links = client.get_image_download_links(project_id, asset_id).data
    selected = select_image_download(details, links)
    relative_output, destination, temp_path = output_paths_for_selection(
        state, "image", asset_id, "jpg"
    )
    client.download_to_path(selected["url"], temp_path)
    os.replace(temp_path, destination)

    return {
        "source_provider": PROVIDER_ID,
        "source_asset_id": asset_id,
        "selection_ref": selection_ref,
        "media_type": "image",
        "relative_output": relative_output,
        "width": selected["width"],
        "height": selected["height"],
        "duration": None,
        "provenance": selected_provenance(
            details,
            project_id,
            asset_id,
            "image",
            {"download_format": selected["format"]},
        ),
    }


def verify_detail_id(details: dict[str, Any], expected_id: str, media_type: str) -> None:
    actual = require_provider_id(details.get("id"), f"{media_type}.id")
    if actual != expected_id:
        raise PluginFailure(
            "STORYBLOCKS_INVALID_RESPONSE",
            f"Storyblocks returned {media_type} id {actual} for selected id {expected_id}.",
            retryable=True,
        )


def selected_provenance(
    details: dict[str, Any],
    project_id: str,
    asset_id: str,
    media_type: str,
    rendition: dict[str, Any],
) -> dict[str, Any]:
    contributor = contributor_name(details)
    return {
        "provider": PROVIDER_ID,
        "provider_asset_id": asset_id,
        "provider_asset_code": optional_text(details.get("asset_id")),
        "provider_content_id": details.get("content_id"),
        "title": optional_text(details.get("title")),
        "creator_name": contributor,
        "license": "Storyblocks production API license",
        "license_mode": "production_api",
        "licensed_project_id": project_id,
        "selected_download_recorded": True,
        "is_editorial": details.get("is_editorial"),
        "has_talent_released": details.get("has_talent_released"),
        "has_property_released": details.get("has_property_released"),
        "content_type": media_type,
        **rendition,
    }


def resolved_orientation(configured: str, aspect_ratio: Any) -> str | None:
    if configured == "all":
        return "all"
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
    return "horizontal" if ratio > 1.0 else "vertical"


def video_orientation(value: str) -> str:
    if value == "square":
        return "all"
    if value in {"horizontal", "vertical", "all"}:
        return value
    raise PluginFailure("INVALID_REQUEST", f"Unsupported Storyblocks video orientation {value}.")


def image_orientation(value: str) -> str:
    mapping = {
        "horizontal": "landscape",
        "vertical": "portrait",
        "square": "square",
        "all": "all",
    }
    if value not in mapping:
        raise PluginFailure("INVALID_REQUEST", f"Unsupported Storyblocks image orientation {value}.")
    return mapping[value]


def require_results(data: dict[str, Any]) -> list[Any]:
    results = data.get("results")
    if not isinstance(results, list):
        raise PluginFailure(
            "STORYBLOCKS_INVALID_RESPONSE",
            "Storyblocks search response results must be an array.",
            retryable=True,
        )
    return results


def require_machine_value(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise PluginFailure("CREDENTIAL_MISSING", f"{label} is missing.")
    normalized = value.strip()
    if len(normalized) > 512 or any(ord(character) < 32 for character in normalized):
        raise PluginFailure("INVALID_SETTINGS", f"{label} is invalid.")
    return normalized


def require_non_empty_string(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise PluginFailure("INVALID_REQUEST", f"{label} must be a non-empty string.")
    return value.strip()


def require_provider_id(value: Any, label: str) -> str:
    if isinstance(value, bool) or not isinstance(value, (int, str)):
        raise PluginFailure(
            "STORYBLOCKS_INVALID_RESPONSE",
            f"{label} must be a provider identifier.",
            retryable=True,
        )
    normalized = str(value).strip()
    if not normalized:
        raise PluginFailure(
            "STORYBLOCKS_INVALID_RESPONSE",
            f"{label} must not be empty.",
            retryable=True,
        )
    return normalized


def optional_text(value: Any) -> str | None:
    if value is None:
        return None
    if not isinstance(value, str):
        raise PluginFailure(
            "STORYBLOCKS_INVALID_RESPONSE",
            "Optional Storyblocks text field must be a string when present.",
            retryable=True,
        )
    normalized = " ".join(value.split())
    return normalized or None


def optional_positive_int(value: Any, label: str) -> int | None:
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise PluginFailure(
            "STORYBLOCKS_INVALID_RESPONSE",
            f"{label} must be a positive integer when present.",
            retryable=True,
        )
    return value


def optional_positive_number(value: Any, label: str) -> float | None:
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, (int, float)) or value <= 0:
        raise PluginFailure(
            "STORYBLOCKS_INVALID_RESPONSE",
            f"{label} must be positive when present.",
            retryable=True,
        )
    return float(value)


def handle_request(
    request: Any,
    state: PluginState,
    client_factory: Callable[[str, str, str], Any] = StoryblocksClient,
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
            if operation == "visual.resolve":
                result = execute_visual_resolve(
                    params.get("payload"),
                    state,
                    client_factory=client_factory,
                )
            elif operation == "visual.fetch_selected":
                result = execute_fetch_selected(
                    params.get("payload"),
                    state,
                    client_factory=client_factory,
                )
            else:
                raise PluginFailure(
                    "UNSUPPORTED_OPERATION",
                    f"Unsupported Storyblocks operation: {operation}.",
                )
        elif method == "plugin.cancel":
            result = {
                "cancelled": False,
                "reason": "Storyblocks requests are synchronous; runtime termination is the cancellation fallback.",
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
    except Exception as error:
        return (
            failure_response(
                request_id,
                PluginFailure(
                    "STORYBLOCKS_INTERNAL_ERROR",
                    f"Unexpected Storyblocks plugin error: {error}",
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
