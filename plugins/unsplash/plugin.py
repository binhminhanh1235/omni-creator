#!/usr/bin/env python3
"""OmniCreator Unsplash VisualProvider plugin.

Plugin API v1 JSONL adapter. Search results use Unsplash hotlinked photo.urls
values for previews. A selected photo triggers photo.links.download_location
before its photo.urls.full bytes are copied into the granted job workspace.
"""

from __future__ import annotations

import json
import os
import socket
import sys
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any, Callable
from urllib.error import HTTPError, URLError
from urllib.parse import parse_qsl, quote, urlencode, urlparse, urlunparse
from urllib.request import Request, urlopen

API_VERSION = 1
PROVIDER_ID = "unsplash"
UNSPLASH_API_BASE = "https://api.unsplash.com"
UNSPLASH_API_HOST = "api.unsplash.com"
UNSPLASH_MEDIA_HOST = "images.unsplash.com"
UNSPLASH_WEB_HOST = "unsplash.com"
REQUEST_TIMEOUT_SECONDS = 20
APP_UTM_SOURCE = "omnicreator"
APP_UTM_MEDIUM = "referral"

SUPPORTED_ORIENTATIONS = {"auto", "all", "landscape", "portrait", "squarish"}
SUPPORTED_ORDER = {"relevant", "latest"}
SUPPORTED_CONTENT_FILTERS = {"low", "high"}
SUPPORTED_QUALITY_MODES = {"standard", "high"}

DEFAULT_SETTINGS: dict[str, Any] = {
    "per_query": 8,
    "orientation": "auto",
    "order_by": "relevant",
    "content_filter": "high",
    "api_key_env": "UNSPLASH_ACCESS_KEY",
}

PROHIBITED_SECRET_SETTING_KEYS = {
    "access_key",
    "api_key",
    "client_id",
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
class UnsplashResponse:
    data: dict[str, Any]
    quota: dict[str, int | None]


class UnsplashClient:
    def __init__(
        self,
        access_key: str,
        opener: Callable[..., Any] = urlopen,
        api_base: str = UNSPLASH_API_BASE,
        timeout_seconds: int = REQUEST_TIMEOUT_SECONDS,
    ) -> None:
        if not access_key.strip():
            raise PluginFailure(
                "CREDENTIAL_MISSING",
                "Unsplash access key is missing.",
                retryable=False,
            )
        self._access_key = access_key.strip()
        self._opener = opener
        self._api_base = api_base.rstrip("/")
        self._timeout_seconds = timeout_seconds

    def search_photos(
        self,
        query: str,
        *,
        orientation: str | None,
        order_by: str,
        content_filter: str,
        per_page: int,
    ) -> UnsplashResponse:
        params: dict[str, Any] = {
            "query": query,
            "page": 1,
            "per_page": per_page,
            "order_by": order_by,
            "content_filter": content_filter,
        }
        if orientation is not None and orientation != "all":
            params["orientation"] = orientation
        return self._get_json("/search/photos", params)

    def get_photo(self, asset_id: str) -> UnsplashResponse:
        encoded = quote(asset_id, safe="")
        return self._get_json(f"/photos/{encoded}", {})

    def track_download(self, download_location: str) -> UnsplashResponse:
        url = validate_download_location(download_location)
        request = self._authorized_request(url)
        try:
            with self._opener(request, timeout=self._timeout_seconds) as response:
                payload = response.read()
                headers = response.headers
        except HTTPError as error:
            raise map_http_error(error, "download tracking") from error
        except (URLError, TimeoutError, socket.timeout, OSError) as error:
            raise PluginFailure(
                "UNSPLASH_DOWNLOAD_TRACKING_ERROR",
                f"Unsplash download tracking failed: {error}",
                retryable=True,
                suggested_fallback="retry-selected-asset",
            ) from error

        data = decode_json_object(payload, "download tracking")
        return UnsplashResponse(data=data, quota=quota_from_headers(headers))

    def download_to_path(self, url: str, destination: Path) -> None:
        media_url = validate_media_url(url)
        request = Request(
            media_url,
            method="GET",
            headers={
                "Accept": "image/*",
                "User-Agent": "OmniCreator-Unsplash/1.0",
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
            raise map_http_error(error, "photo transfer") from error
        except (URLError, TimeoutError, socket.timeout, OSError) as error:
            raise PluginFailure(
                "UNSPLASH_DOWNLOAD_ERROR",
                f"Unsplash photo transfer failed: {error}",
                retryable=True,
                suggested_fallback="retry-selected-asset",
            ) from error

        if not destination.is_file() or destination.stat().st_size == 0:
            raise PluginFailure(
                "UNSPLASH_EMPTY_DOWNLOAD",
                "Unsplash photo transfer produced an empty file.",
                retryable=True,
                suggested_fallback="retry-selected-asset",
            )

    def _get_json(self, path: str, params: dict[str, Any]) -> UnsplashResponse:
        query = urlencode(params)
        url = f"{self._api_base}{path}"
        if query:
            url = f"{url}?{query}"
        request = self._authorized_request(url)

        try:
            with self._opener(request, timeout=self._timeout_seconds) as response:
                payload = response.read()
                headers = response.headers
        except HTTPError as error:
            raise map_http_error(error, "API request") from error
        except (URLError, TimeoutError, socket.timeout, OSError) as error:
            raise PluginFailure(
                "UNSPLASH_NETWORK_ERROR",
                f"Unsplash API request failed: {error}",
                retryable=True,
                suggested_fallback="local-library",
            ) from error

        data = decode_json_object(payload, "API")
        return UnsplashResponse(data=data, quota=quota_from_headers(headers))

    def _authorized_request(self, url: str) -> Request:
        parsed = urlparse(url)
        if parsed.scheme != "https" or (parsed.hostname or "").lower() != UNSPLASH_API_HOST:
            raise PluginFailure(
                "UNSPLASH_API_HOST_NOT_ALLOWED",
                "Unsplash API requests must use https://api.unsplash.com.",
                retryable=False,
            )
        return Request(
            url,
            method="GET",
            headers={
                "Accept": "application/json",
                "Accept-Version": "v1",
                "Authorization": f"Client-ID {self._access_key}",
                "User-Agent": "OmniCreator-Unsplash/1.0",
            },
        )


def decode_json_object(payload: bytes, label: str) -> dict[str, Any]:
    try:
        data = json.loads(payload.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise PluginFailure(
            "UNSPLASH_INVALID_RESPONSE",
            f"Unsplash {label} response was invalid JSON: {error}",
            retryable=True,
            suggested_fallback="local-library",
        ) from error
    if not isinstance(data, dict):
        raise PluginFailure(
            "UNSPLASH_INVALID_RESPONSE",
            f"Unsplash {label} response root must be an object.",
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
            "UNSPLASH_UNAUTHORIZED",
            f"Unsplash rejected the configured access key during {operation}.",
            retryable=False,
        )
    if error.code == 429:
        return PluginFailure(
            "UNSPLASH_RATE_LIMITED",
            f"Unsplash rate limit was exceeded during {operation}.",
            retryable=True,
            retry_after_seconds=retry_after,
            suggested_fallback="local-library",
        )
    if 500 <= error.code <= 599:
        return PluginFailure(
            "UNSPLASH_UPSTREAM_ERROR",
            f"Unsplash returned HTTP {error.code} during {operation}.",
            retryable=True,
            retry_after_seconds=retry_after,
            suggested_fallback="local-library",
        )
    return PluginFailure(
        "UNSPLASH_HTTP_ERROR",
        f"Unsplash returned HTTP {error.code} during {operation}.",
        retryable=False,
    )


def quota_from_headers(headers: Any) -> dict[str, int | None]:
    return {
        "limit": parse_optional_int(headers.get("X-Ratelimit-Limit")),
        "remaining": parse_optional_int(headers.get("X-Ratelimit-Remaining")),
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
        return {
            "plugin_id": PROVIDER_ID,
            "api_version": API_VERSION,
            "settings": public_settings(self.settings),
            "workspace_ready": self.job_workspace is not None,
            "hotlink_previews": True,
            "download_tracking_required": True,
        }


def merge_settings(base: dict[str, Any], raw: Any) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise PluginFailure("INVALID_SETTINGS", "Unsplash settings must be an object.")

    prohibited = sorted(
        key for key in raw if key.strip().lower() in PROHIBITED_SECRET_SETTING_KEYS
    )
    if prohibited:
        raise PluginFailure(
            "SECRET_SETTING_REJECTED",
            "Unsplash secret values must stay machine-local. Configure the access key through "
            f"the environment variable named by api_key_env; rejected keys: {', '.join(prohibited)}.",
        )

    settings = dict(base)
    for key in DEFAULT_SETTINGS:
        if key in raw:
            settings[key] = raw[key]
    validate_settings(settings)
    return settings


def validate_settings(settings: dict[str, Any]) -> None:
    per_query = settings.get("per_query")
    if type(per_query) is not int or not 1 <= per_query <= 30:
        raise PluginFailure(
            "INVALID_SETTINGS",
            "per_query must be an integer between 1 and 30.",
        )
    if settings.get("orientation") not in SUPPORTED_ORIENTATIONS:
        raise PluginFailure(
            "INVALID_SETTINGS",
            f"orientation must be one of {sorted(SUPPORTED_ORIENTATIONS)}.",
        )
    if settings.get("order_by") not in SUPPORTED_ORDER:
        raise PluginFailure(
            "INVALID_SETTINGS",
            f"order_by must be one of {sorted(SUPPORTED_ORDER)}.",
        )
    if settings.get("content_filter") not in SUPPORTED_CONTENT_FILTERS:
        raise PluginFailure(
            "INVALID_SETTINGS",
            f"content_filter must be one of {sorted(SUPPORTED_CONTENT_FILTERS)}.",
        )
    api_key_env = settings.get("api_key_env")
    if not isinstance(api_key_env, str) or not api_key_env.strip():
        raise PluginFailure(
            "INVALID_SETTINGS",
            "api_key_env must be a non-empty environment variable name.",
        )


def public_settings(settings: dict[str, Any]) -> dict[str, Any]:
    return {key: settings[key] for key in DEFAULT_SETTINGS}


def health_result(state: PluginState) -> dict[str, Any]:
    env_name = state.settings["api_key_env"]
    credential_present = bool(os.environ.get(env_name, "").strip())
    return {
        "status": "ready" if credential_present else "needs_attention",
        "provider": PROVIDER_ID,
        "credential_env": env_name,
        "credential_present": credential_present,
        "network_host": UNSPLASH_API_HOST,
        "hotlink_previews": True,
        "download_tracking_required": True,
    }


def capabilities_result() -> dict[str, Any]:
    return {
        "types": ["visual"],
        "capabilities": [
            "stock_image",
            "preview_first_search",
            "selected_asset_download",
        ],
        "operations": ["visual.resolve", "visual.fetch_selected"],
        "scene_types": ["literal", "emotional", "conceptual"],
    }


def execute_visual_resolve(
    payload: Any,
    state: PluginState,
    client_factory: Callable[[str], Any] = UnsplashClient,
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
        query = " ".join(require_non_empty_string(
            value, "scene.search_queries entry"
        ).split())
        normalized = query.lower()
        if normalized in seen_queries:
            continue
        seen_queries.add(normalized)
        queries.append(query)

    settings = merge_settings(state.settings, payload.get("settings", {}))
    media_type = payload.get("media_type", "image")
    if media_type not in {"image", "both"}:
        raise PluginFailure(
            "UNSUPPORTED_MEDIA_TYPE",
            "Unsplash is an image-only provider; media_type must be image or both.",
            retryable=False,
            suggested_fallback="next-stock-provider",
        )

    orientation = resolved_orientation(settings["orientation"], scene.get("aspect_ratio"))
    api_key_env = settings["api_key_env"]
    access_key = os.environ.get(api_key_env, "").strip()
    if not access_key:
        raise PluginFailure(
            "CREDENTIAL_MISSING",
            f"Unsplash access key is missing. Set machine-local environment variable {api_key_env}.",
            retryable=False,
            suggested_fallback="local-library",
        )

    client = client_factory(access_key)
    candidates: list[dict[str, Any]] = []
    seen_candidates: set[str] = set()
    last_quota = {"limit": None, "remaining": None}

    for query in queries:
        response = client.search_photos(
            query,
            orientation=orientation,
            order_by=settings["order_by"],
            content_filter=settings["content_filter"],
            per_page=settings["per_query"],
        )
        last_quota = response.quota
        raw_results = response.data.get("results")
        if not isinstance(raw_results, list):
            raise PluginFailure(
                "UNSPLASH_INVALID_RESPONSE",
                "Unsplash search response results must be an array.",
                retryable=True,
                suggested_fallback="local-library",
            )
        for raw_photo in raw_results:
            candidate = normalize_photo(scene_id, raw_photo)
            if candidate["candidate_id"] not in seen_candidates:
                seen_candidates.add(candidate["candidate_id"])
                candidates.append(candidate)

    return {
        "provider": PROVIDER_ID,
        "scene_id": scene_id,
        "preview_only": True,
        "hotlink_previews": True,
        "queries": queries,
        "media_type": "image",
        "candidates": candidates,
        "quota": last_quota,
    }


def normalize_photo(scene_id: str, raw: Any) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise PluginFailure(
            "UNSPLASH_INVALID_RESPONSE",
            "Unsplash photo must be an object.",
            retryable=True,
        )

    asset_id = require_provider_id(raw.get("id"), "photo.id")
    urls = require_object(raw.get("urls"), "photo.urls")
    preview_url = first_hotlink_url(urls, ("regular", "small", "thumb"))
    links = require_object(raw.get("links"), "photo.links")
    source_page = validate_unsplash_web_url(
        require_non_empty_string(links.get("html"), "photo.links.html")
    )
    user = require_object(raw.get("user"), "photo.user")
    creator_name = require_non_empty_string(user.get("name"), "photo.user.name")
    user_links = require_object(user.get("links"), "photo.user.links")
    creator_url = validate_unsplash_web_url(
        require_non_empty_string(user_links.get("html"), "photo.user.links.html")
    )

    description = optional_non_empty_string(raw.get("description"))
    alt_description = optional_non_empty_string(raw.get("alt_description"))
    tags = normalize_tags(raw.get("tags"))

    return {
        "candidate_id": f"unsplash:image:{asset_id}",
        "scene_id": scene_id,
        "source_provider": PROVIDER_ID,
        "source_asset_id": asset_id,
        "selection_ref": f"unsplash:image:{asset_id}",
        "media_type": "image",
        "title": alt_description or description,
        "description": description or alt_description,
        "tags": tags,
        "source_page_url": with_utm(source_page),
        "creator_name": creator_name,
        "creator_url": with_utm(creator_url),
        "width": optional_positive_int(raw.get("width"), "photo.width"),
        "height": optional_positive_int(raw.get("height"), "photo.height"),
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


def normalize_tags(value: Any) -> list[str]:
    if value is None:
        return []
    if not isinstance(value, list):
        raise PluginFailure(
            "UNSPLASH_INVALID_RESPONSE",
            "Unsplash photo tags must be an array when present.",
            retryable=True,
        )

    tags: list[str] = []
    seen: set[str] = set()
    for raw_tag in value:
        if isinstance(raw_tag, str):
            tag = " ".join(raw_tag.split())
        elif isinstance(raw_tag, dict):
            source = raw_tag.get("title")
            tag = " ".join(source.split()) if isinstance(source, str) else ""
        else:
            tag = ""
        key = tag.lower()
        if tag and key not in seen:
            seen.add(key)
            tags.append(tag)
    return tags[:20]


def first_hotlink_url(urls: dict[str, Any], keys: tuple[str, ...]) -> str:
    for key in keys:
        value = urls.get(key)
        if isinstance(value, str) and value.strip():
            return validate_media_url(value)
    raise PluginFailure(
        "UNSPLASH_INVALID_RESPONSE",
        "Unsplash photo did not contain a usable hotlinked image URL.",
        retryable=True,
    )


def validate_download_location(url: Any) -> str:
    value = require_non_empty_string(url, "photo.links.download_location")
    parsed = urlparse(value)
    host = (parsed.hostname or "").lower()
    if parsed.scheme != "https" or host != UNSPLASH_API_HOST:
        raise PluginFailure(
            "UNSPLASH_DOWNLOAD_LOCATION_NOT_ALLOWED",
            "Unsplash download_location must use https://api.unsplash.com.",
            retryable=False,
        )
    if not parsed.path.startswith("/photos/") or not parsed.path.endswith("/download"):
        raise PluginFailure(
            "UNSPLASH_DOWNLOAD_LOCATION_NOT_ALLOWED",
            "Unsplash download_location must target a photo download tracking endpoint.",
            retryable=False,
        )
    return value


def validate_media_url(url: Any) -> str:
    value = require_non_empty_string(url, "photo URL")
    parsed = urlparse(value)
    host = (parsed.hostname or "").lower()
    if parsed.scheme != "https" or host != UNSPLASH_MEDIA_HOST:
        raise PluginFailure(
            "UNSPLASH_MEDIA_HOST_NOT_ALLOWED",
            "Unsplash photo URLs must use https://images.unsplash.com.",
            retryable=False,
        )
    return value


def validate_unsplash_web_url(url: str) -> str:
    parsed = urlparse(url)
    host = (parsed.hostname or "").lower()
    if parsed.scheme != "https" or host not in {UNSPLASH_WEB_HOST, f"www.{UNSPLASH_WEB_HOST}"}:
        raise PluginFailure(
            "UNSPLASH_ATTRIBUTION_URL_NOT_ALLOWED",
            "Unsplash attribution links must use https://unsplash.com.",
            retryable=False,
        )
    return url


def with_utm(url: str) -> str:
    parsed = urlparse(validate_unsplash_web_url(url))
    query = dict(parse_qsl(parsed.query, keep_blank_values=True))
    query["utm_source"] = APP_UTM_SOURCE
    query["utm_medium"] = APP_UTM_MEDIUM
    return urlunparse(parsed._replace(query=urlencode(query)))


def parse_selection_ref(selection_ref: Any) -> str:
    value = require_non_empty_string(selection_ref, "selection_ref")
    prefix = "unsplash:image:"
    if not value.startswith(prefix):
        raise PluginFailure(
            "INVALID_SELECTION_REF",
            "selection_ref must be unsplash:image:<photo-id>.",
        )
    asset_id = value[len(prefix):].strip()
    if not asset_id or ":" in asset_id or "/" in asset_id or "\\" in asset_id:
        raise PluginFailure(
            "INVALID_SELECTION_REF",
            "Unsplash photo id in selection_ref is invalid.",
        )
    return asset_id


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


def output_paths_for_selection(
    state: PluginState,
    asset_id: str,
) -> tuple[str, Path, Path]:
    if state.job_workspace is None:
        raise PluginFailure(
            "WORKSPACE_REQUIRED",
            "visual.fetch_selected requires plugin.initialize with a job_workspace.",
            retryable=False,
        )

    safe_id = "".join(
        character if character.isalnum() or character in {"-", "_"} else "_"
        for character in asset_id
    )
    if not safe_id:
        raise PluginFailure("INVALID_SELECTION_REF", "Unsplash photo id is invalid.")

    relative_output = f"selected/unsplash-image-{safe_id}.jpg"
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

    temp_path = temp_root / f"unsplash-image-{safe_id}.jpg.part"
    return relative_output, destination, temp_path


def execute_fetch_selected(
    payload: Any,
    state: PluginState,
    client_factory: Callable[[str], Any] = UnsplashClient,
) -> dict[str, Any]:
    if not isinstance(payload, dict):
        raise PluginFailure(
            "INVALID_REQUEST",
            "visual.fetch_selected payload must be an object.",
        )

    selection_ref = require_non_empty_string(payload.get("selection_ref"), "selection_ref")
    asset_id = parse_selection_ref(selection_ref)
    quality_mode = payload.get("quality_mode", "standard")
    if quality_mode not in SUPPORTED_QUALITY_MODES:
        raise PluginFailure(
            "INVALID_REQUEST",
            f"quality_mode must be one of {sorted(SUPPORTED_QUALITY_MODES)}.",
        )

    api_key_env = state.settings["api_key_env"]
    access_key = os.environ.get(api_key_env, "").strip()
    if not access_key:
        raise PluginFailure(
            "CREDENTIAL_MISSING",
            f"Unsplash access key is missing. Set machine-local environment variable {api_key_env}.",
            retryable=False,
            suggested_fallback="local-library",
        )

    client = client_factory(access_key)
    detail = client.get_photo(asset_id)
    raw = detail.data

    returned_id = require_provider_id(raw.get("id"), "photo.id")
    if returned_id != asset_id:
        raise PluginFailure(
            "UNSPLASH_INVALID_RESPONSE",
            f"Unsplash returned photo id {returned_id} for selected id {asset_id}.",
            retryable=True,
        )

    links = require_object(raw.get("links"), "photo.links")
    download_location = validate_download_location(
        links.get("download_location")
    )

    # Required by Unsplash whenever a user selects a photo for use.
    # Tracking must succeed before production bytes are accepted.
    tracking = client.track_download(download_location)

    urls = require_object(raw.get("urls"), "photo.urls")
    selected_url = first_hotlink_url(urls, ("full", "raw"))
    relative_output, destination, temp_path = output_paths_for_selection(state, asset_id)
    client.download_to_path(selected_url, temp_path)
    os.replace(temp_path, destination)

    user = require_object(raw.get("user"), "photo.user")
    creator_name = require_non_empty_string(user.get("name"), "photo.user.name")
    user_links = require_object(user.get("links"), "photo.user.links")
    creator_url = with_utm(
        validate_unsplash_web_url(
            require_non_empty_string(user_links.get("html"), "photo.user.links.html")
        )
    )
    source_page_url = with_utm(
        validate_unsplash_web_url(
            require_non_empty_string(links.get("html"), "photo.links.html")
        )
    )
    unsplash_url = with_utm("https://unsplash.com/")
    attribution = f"Photo by {creator_name} on Unsplash"

    return {
        "source_provider": PROVIDER_ID,
        "source_asset_id": asset_id,
        "selection_ref": selection_ref,
        "media_type": "image",
        "relative_output": relative_output,
        "width": optional_positive_int(raw.get("width"), "photo.width"),
        "height": optional_positive_int(raw.get("height"), "photo.height"),
        "duration": None,
        "provenance": {
            "provider": PROVIDER_ID,
            "provider_asset_id": asset_id,
            "source_page_url": source_page_url,
            "creator_name": creator_name,
            "creator_url": creator_url,
            "unsplash_url": unsplash_url,
            "attribution": attribution,
            "license": "Unsplash License",
            "api_guidelines": "Unsplash API Guidelines",
            "download_tracked": True,
            "tracking_quota": tracking.quota,
            "provider_rendition": "full",
            "requested_quality_mode": quality_mode,
        },
    }


def resolved_orientation(configured: str, aspect_ratio: Any) -> str | None:
    if configured == "all":
        return None
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
        return "squarish"
    return "landscape" if ratio > 1.0 else "portrait"


def handle_request(
    request: Any,
    state: PluginState,
    client_factory: Callable[[str], Any] = UnsplashClient,
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
                    f"Unsupported Unsplash operation: {operation}.",
                )
        elif method == "plugin.cancel":
            result = {
                "cancelled": False,
                "reason": "Unsplash requests are synchronous; runtime termination is the cancellation fallback.",
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
                    "UNSPLASH_INTERNAL_ERROR",
                    f"Unexpected Unsplash plugin error: {error}",
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
    if not isinstance(value, str) or not value.strip():
        raise PluginFailure(
            "UNSPLASH_INVALID_RESPONSE",
            f"{label} must be a non-empty string.",
            retryable=True,
        )
    return value.strip()


def require_object(value: Any, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise PluginFailure(
            "UNSPLASH_INVALID_RESPONSE",
            f"{label} must be an object.",
            retryable=True,
        )
    return value


def optional_positive_int(value: Any, label: str) -> int | None:
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise PluginFailure(
            "UNSPLASH_INVALID_RESPONSE",
            f"{label} must be a positive integer when present.",
            retryable=True,
        )
    return value


def optional_non_empty_string(value: Any) -> str | None:
    if value is None:
        return None
    if not isinstance(value, str):
        raise PluginFailure(
            "UNSPLASH_INVALID_RESPONSE",
            "Optional Unsplash text field must be a string when present.",
            retryable=True,
        )
    normalized = " ".join(value.split())
    return normalized or None


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
