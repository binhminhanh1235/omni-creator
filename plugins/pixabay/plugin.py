#!/usr/bin/env python3
"""OmniCreator Pixabay VisualProvider plugin.

Plugin API v1 JSONL adapter. Search is preview-first and never exposes the
selected production download URL. Pixabay search responses are cached for 24
hours in the scoped provider cache granted by Plugin Runtime v1.
"""

from __future__ import annotations

import hashlib
import json
import os
import socket
import sys
import time
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any, Callable
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlencode, urlparse
from urllib.request import Request, urlopen

API_VERSION = 1
PROVIDER_ID = "pixabay"
PIXABAY_API_BASE = "https://pixabay.com"
PIXABAY_HOST = "pixabay.com"
REQUEST_TIMEOUT_SECONDS = 20
CACHE_TTL_SECONDS = 24 * 60 * 60
MAX_QUERY_CHARS = 100

SUPPORTED_MEDIA_TYPES = {"video", "image", "both"}
SUPPORTED_QUALITY_MODES = {"standard", "high"}
SUPPORTED_ORIENTATIONS = {"auto", "all", "horizontal", "vertical"}
SUPPORTED_LANGUAGES = {
    "cs", "da", "de", "en", "es", "fr", "id", "it", "hu", "nl", "no",
    "pl", "pt", "ro", "sk", "fi", "sv", "tr", "vi", "th", "bg", "ru",
    "el", "ja", "ko", "zh",
}
SUPPORTED_ORDERS = {"popular", "latest"}
ALLOWED_MEDIA_HOSTS = {"pixabay.com", "cdn.pixabay.com"}

DEFAULT_SETTINGS: dict[str, Any] = {
    "media_type": "video",
    "per_query": 8,
    "orientation": "auto",
    "language": "en",
    "safe_search": True,
    "order": "popular",
    "minimum_width": 1280,
    "minimum_height": 720,
    "api_key_env": "PIXABAY_API_KEY",
}

PROHIBITED_SECRET_SETTING_KEYS = {
    "api_key",
    "key",
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
class PixabayResponse:
    data: dict[str, Any]
    quota: dict[str, int | None]
    cache_hit: bool = False


class PixabayClient:
    def __init__(
        self,
        api_key: str,
        cache_dir: Path | None = None,
        opener: Callable[..., Any] = urlopen,
        api_base: str = PIXABAY_API_BASE,
        timeout_seconds: int = REQUEST_TIMEOUT_SECONDS,
        clock: Callable[[], float] = time.time,
    ) -> None:
        if not api_key.strip():
            raise PluginFailure("CREDENTIAL_MISSING", "Pixabay API key is missing.")
        self._api_key = api_key.strip()
        self._cache_dir = cache_dir
        self._opener = opener
        self._api_base = api_base.rstrip("/")
        self._timeout_seconds = timeout_seconds
        self._clock = clock

    def search_images(
        self,
        query: str,
        *,
        orientation: str | None,
        language: str,
        safe_search: bool,
        order: str,
        minimum_width: int,
        minimum_height: int,
        per_page: int,
    ) -> PixabayResponse:
        return self._get_json(
            "/api/",
            self._search_params(
                query=query,
                orientation=orientation,
                language=language,
                safe_search=safe_search,
                order=order,
                minimum_width=minimum_width,
                minimum_height=minimum_height,
                per_page=per_page,
            ),
        )

    def search_videos(
        self,
        query: str,
        *,
        orientation: str | None,
        language: str,
        safe_search: bool,
        order: str,
        minimum_width: int,
        minimum_height: int,
        per_page: int,
    ) -> PixabayResponse:
        return self._get_json(
            "/api/videos/",
            self._search_params(
                query=query,
                orientation=orientation,
                language=language,
                safe_search=safe_search,
                order=order,
                minimum_width=minimum_width,
                minimum_height=minimum_height,
                per_page=per_page,
            ),
        )

    def get_image(self, asset_id: str) -> PixabayResponse:
        return self._get_json("/api/", {"id": asset_id})

    def get_video(self, asset_id: str) -> PixabayResponse:
        return self._get_json("/api/videos/", {"id": asset_id})

    def download_to_path(self, url: str, destination: Path) -> None:
        validate_media_url(url)
        request = Request(
            url,
            method="GET",
            headers={
                "Accept": "*/*",
                "User-Agent": "OmniCreator-Pixabay/1.0",
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
            raise map_http_error(error) from error
        except (URLError, TimeoutError, socket.timeout, OSError) as error:
            raise PluginFailure(
                "PIXABAY_DOWNLOAD_ERROR",
                f"Pixabay media download failed: {error}",
                retryable=True,
                suggested_fallback="retry-selected-asset",
            ) from error

        if not destination.is_file() or destination.stat().st_size == 0:
            raise PluginFailure(
                "PIXABAY_EMPTY_DOWNLOAD",
                "Pixabay media download produced an empty file.",
                retryable=True,
                suggested_fallback="retry-selected-asset",
            )

    @staticmethod
    def _search_params(
        *,
        query: str,
        orientation: str | None,
        language: str,
        safe_search: bool,
        order: str,
        minimum_width: int,
        minimum_height: int,
        per_page: int,
    ) -> dict[str, Any]:
        params: dict[str, Any] = {
            "q": normalize_provider_query(query),
            "lang": language,
            "safesearch": "true" if safe_search else "false",
            "order": order,
            "min_width": minimum_width,
            "min_height": minimum_height,
            "per_page": per_page,
            "page": 1,
        }
        if orientation is not None and orientation != "all":
            params["orientation"] = orientation
        return params

    def _get_json(self, path: str, params: dict[str, Any]) -> PixabayResponse:
        cached = self._read_cache(path, params)
        if cached is not None:
            return PixabayResponse(
                data=cached,
                quota={"limit": None, "remaining": None, "reset": None},
                cache_hit=True,
            )

        request_params = {"key": self._api_key, **params}
        query = urlencode(request_params)
        url = f"{self._api_base}{path}?{query}"
        request = Request(
            url,
            method="GET",
            headers={
                "Accept": "application/json",
                "User-Agent": "OmniCreator-Pixabay/1.0",
            },
        )

        try:
            with self._opener(request, timeout=self._timeout_seconds) as response:
                payload = response.read()
                headers = response.headers
        except HTTPError as error:
            raise map_http_error(error) from error
        except (URLError, TimeoutError, socket.timeout, OSError) as error:
            raise PluginFailure(
                "PIXABAY_NETWORK_ERROR",
                f"Pixabay network request failed: {error}",
                retryable=True,
                suggested_fallback="local-library",
            ) from error

        try:
            data = json.loads(payload.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise PluginFailure(
                "PIXABAY_INVALID_RESPONSE",
                f"Pixabay returned invalid JSON: {error}",
                retryable=True,
                suggested_fallback="local-library",
            ) from error

        if not isinstance(data, dict):
            raise PluginFailure(
                "PIXABAY_INVALID_RESPONSE",
                "Pixabay response root must be a JSON object.",
                retryable=True,
                suggested_fallback="local-library",
            )

        self._write_cache(path, params, data)
        return PixabayResponse(data=data, quota=quota_from_headers(headers), cache_hit=False)

    def _cache_path(self, path: str, params: dict[str, Any]) -> Path | None:
        if self._cache_dir is None:
            return None
        canonical = json.dumps(
            {"path": path, "params": params},
            sort_keys=True,
            separators=(",", ":"),
            ensure_ascii=True,
        ).encode("utf-8")
        digest = hashlib.sha256(canonical).hexdigest()
        return self._cache_dir / f"{digest}.json"

    def _read_cache(self, path: str, params: dict[str, Any]) -> dict[str, Any] | None:
        cache_path = self._cache_path(path, params)
        if cache_path is None or not cache_path.is_file():
            return None
        try:
            envelope = json.loads(cache_path.read_text(encoding="utf-8"))
        except (OSError, UnicodeDecodeError, json.JSONDecodeError):
            return None
        if not isinstance(envelope, dict):
            return None
        cached_at = envelope.get("cached_at")
        data = envelope.get("data")
        if not isinstance(cached_at, (int, float)) or not isinstance(data, dict):
            return None
        age = self._clock() - float(cached_at)
        if age < 0 or age > CACHE_TTL_SECONDS:
            return None
        return data

    def _write_cache(self, path: str, params: dict[str, Any], data: dict[str, Any]) -> None:
        cache_path = self._cache_path(path, params)
        if cache_path is None:
            return
        try:
            cache_path.parent.mkdir(parents=True, exist_ok=True)
            temp_path = cache_path.with_suffix(".tmp")
            temp_path.write_text(
                json.dumps(
                    {"cached_at": self._clock(), "data": data},
                    sort_keys=True,
                    separators=(",", ":"),
                ),
                encoding="utf-8",
            )
            os.replace(temp_path, cache_path)
        except OSError:
            # Cache failure must not convert a successful provider request into a failed job.
            return


def map_http_error(error: HTTPError) -> PluginFailure:
    retry_after = parse_optional_positive_int(
        error.headers.get("Retry-After") if error.headers is not None else None
    )
    if retry_after is None and error.headers is not None:
        retry_after = parse_optional_positive_int(error.headers.get("X-RateLimit-Reset"))

    if error.code in {401, 403}:
        return PluginFailure(
            "PIXABAY_UNAUTHORIZED",
            "Pixabay rejected the configured API key or denied API access.",
            retryable=False,
        )
    if error.code == 429:
        return PluginFailure(
            "PIXABAY_RATE_LIMITED",
            "Pixabay API rate limit was exceeded.",
            retryable=True,
            retry_after_seconds=retry_after,
            suggested_fallback="local-library",
        )
    if 500 <= error.code <= 599:
        return PluginFailure(
            "PIXABAY_UPSTREAM_ERROR",
            f"Pixabay returned HTTP {error.code}.",
            retryable=True,
            retry_after_seconds=retry_after,
            suggested_fallback="local-library",
        )
    return PluginFailure(
        "PIXABAY_HTTP_ERROR",
        f"Pixabay returned HTTP {error.code}.",
        retryable=False,
    )


def quota_from_headers(headers: Any) -> dict[str, int | None]:
    return {
        "limit": parse_optional_int(headers.get("X-RateLimit-Limit")),
        "remaining": parse_optional_int(headers.get("X-RateLimit-Remaining")),
        "reset": parse_optional_int(headers.get("X-RateLimit-Reset")),
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
        self.provider_cache: Path | None = None
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
        if "provider_cache" in params and params.get("provider_cache") is not None:
            self.provider_cache = validate_provider_cache(params.get("provider_cache"))

        if any(
            key in params
            for key in ("settings", "job_workspace", "provider_cache", "permissions")
        ):
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
            "provider_cache_ready": self.provider_cache is not None,
            "cache_ttl_seconds": CACHE_TTL_SECONDS,
        }


def merge_settings(base: dict[str, Any], raw: Any) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise PluginFailure("INVALID_SETTINGS", "Pixabay settings must be an object.")

    prohibited = sorted(
        key for key in raw if key.strip().lower() in PROHIBITED_SECRET_SETTING_KEYS
    )
    if prohibited:
        raise PluginFailure(
            "SECRET_SETTING_REJECTED",
            "Pixabay secret values must stay machine-local. Configure the API key through "
            f"the environment variable named by api_key_env; rejected keys: {', '.join(prohibited)}.",
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
    if type(per_query) is not int or not 3 <= per_query <= 80:
        raise PluginFailure(
            "INVALID_SETTINGS",
            "per_query must be an integer between 3 and 80.",
        )
    if settings.get("orientation") not in SUPPORTED_ORIENTATIONS:
        raise PluginFailure(
            "INVALID_SETTINGS",
            f"orientation must be one of {sorted(SUPPORTED_ORIENTATIONS)}.",
        )
    if settings.get("language") not in SUPPORTED_LANGUAGES:
        raise PluginFailure(
            "INVALID_SETTINGS",
            "language is not supported by the Pixabay search API.",
        )
    if type(settings.get("safe_search")) is not bool:
        raise PluginFailure("INVALID_SETTINGS", "safe_search must be a boolean.")
    if settings.get("order") not in SUPPORTED_ORDERS:
        raise PluginFailure(
            "INVALID_SETTINGS",
            f"order must be one of {sorted(SUPPORTED_ORDERS)}.",
        )
    for key, maximum in (("minimum_width", 7680), ("minimum_height", 4320)):
        value = settings.get(key)
        if type(value) is not int or not 0 <= value <= maximum:
            raise PluginFailure(
                "INVALID_SETTINGS",
                f"{key} must be an integer between 0 and {maximum}.",
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
        "network_host": PIXABAY_HOST,
        "provider_cache_ready": state.provider_cache is not None,
        "cache_ttl_seconds": CACHE_TTL_SECONDS,
    }


def capabilities_result() -> dict[str, Any]:
    return {
        "types": ["visual"],
        "capabilities": [
            "stock_video",
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
    client_factory: Callable[[str, Path | None], Any] = PixabayClient,
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
        query = normalize_provider_query(
            require_non_empty_string(value, "scene.search_queries entry")
        )
        normalized = " ".join(query.lower().split())
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

    orientation = resolved_orientation(settings["orientation"], scene.get("aspect_ratio"))
    api_key_env = settings["api_key_env"]
    api_key = os.environ.get(api_key_env, "").strip()
    if not api_key:
        raise PluginFailure(
            "CREDENTIAL_MISSING",
            f"Pixabay API key is missing. Set machine-local environment variable {api_key_env}.",
            retryable=False,
            suggested_fallback="local-library",
        )

    client = client_factory(api_key, state.provider_cache)
    candidates: list[dict[str, Any]] = []
    seen_candidates: set[str] = set()
    last_quota = {"limit": None, "remaining": None, "reset": None}
    cache_hits = 0

    search_kwargs = {
        "orientation": orientation,
        "language": settings["language"],
        "safe_search": settings["safe_search"],
        "order": settings["order"],
        "minimum_width": settings["minimum_width"],
        "minimum_height": settings["minimum_height"],
        "per_page": settings["per_query"],
    }

    for query in queries:
        if media_type in {"video", "both"}:
            response = client.search_videos(query, **search_kwargs)
            last_quota = response.quota
            cache_hits += 1 if response.cache_hit else 0
            for raw_video in require_list(response.data.get("hits"), "hits"):
                candidate = normalize_video(scene_id, raw_video)
                if candidate["candidate_id"] not in seen_candidates:
                    seen_candidates.add(candidate["candidate_id"])
                    candidates.append(candidate)

        if media_type in {"image", "both"}:
            response = client.search_images(query, **search_kwargs)
            last_quota = response.quota
            cache_hits += 1 if response.cache_hit else 0
            for raw_image in require_list(response.data.get("hits"), "hits"):
                candidate = normalize_image(scene_id, raw_image)
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
        "cache_hits": cache_hits,
        "cache_ttl_seconds": CACHE_TTL_SECONDS,
    }


def normalize_image(scene_id: str, raw: Any) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise PluginFailure("PIXABAY_INVALID_RESPONSE", "Pixabay image must be an object.")

    asset_id = require_provider_id(raw.get("id"), "image.id")
    preview_url = validate_media_url(raw.get("previewURL"))
    source_page_url = optional_non_empty_string(raw.get("pageURL"), "image.pageURL")
    creator_name = optional_non_empty_string(raw.get("user"), "image.user")
    creator_url = creator_profile_url(raw)
    tags = normalize_tags(raw.get("tags"))

    return {
        "candidate_id": f"pixabay:image:{asset_id}",
        "scene_id": scene_id,
        "source_provider": PROVIDER_ID,
        "source_asset_id": asset_id,
        "selection_ref": f"pixabay:image:{asset_id}",
        "media_type": "image",
        "title": ", ".join(tags[:3]) or None,
        "description": ", ".join(tags) or None,
        "tags": tags,
        "source_page_url": source_page_url,
        "creator_name": creator_name,
        "creator_url": creator_url,
        "width": optional_positive_int(raw.get("imageWidth"), "image.imageWidth"),
        "height": optional_positive_int(raw.get("imageHeight"), "image.imageHeight"),
        "duration": None,
        "previews": [
            {
                "kind": "image",
                "url": preview_url,
                "width": optional_positive_int(raw.get("previewWidth"), "image.previewWidth"),
                "height": optional_positive_int(raw.get("previewHeight"), "image.previewHeight"),
                "duration": None,
            }
        ],
    }


def normalize_video(scene_id: str, raw: Any) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise PluginFailure("PIXABAY_INVALID_RESPONSE", "Pixabay video must be an object.")

    asset_id = require_provider_id(raw.get("id"), "video.id")
    source_page_url = optional_non_empty_string(raw.get("pageURL"), "video.pageURL")
    creator_name = optional_non_empty_string(raw.get("user"), "video.user")
    creator_url = creator_profile_url(raw)
    tags = normalize_tags(raw.get("tags"))
    duration = optional_positive_number(raw.get("duration"), "video.duration")
    rendition = preferred_video_rendition(raw, "standard", require_url=False)

    thumbnail = validate_media_url(rendition.get("thumbnail"))
    return {
        "candidate_id": f"pixabay:video:{asset_id}",
        "scene_id": scene_id,
        "source_provider": PROVIDER_ID,
        "source_asset_id": asset_id,
        "selection_ref": f"pixabay:video:{asset_id}",
        "media_type": "video",
        "title": ", ".join(tags[:3]) or None,
        "description": ", ".join(tags) or None,
        "tags": tags,
        "source_page_url": source_page_url,
        "creator_name": creator_name,
        "creator_url": creator_url,
        "width": optional_positive_int(rendition.get("width"), "video.rendition.width"),
        "height": optional_positive_int(rendition.get("height"), "video.rendition.height"),
        "duration": duration,
        "previews": [
            {
                "kind": "thumbnail",
                "url": thumbnail,
                "width": optional_positive_int(rendition.get("width"), "video.rendition.width"),
                "height": optional_positive_int(rendition.get("height"), "video.rendition.height"),
                "duration": duration,
            }
        ],
    }


def normalize_tags(value: Any) -> list[str]:
    if value is None:
        return []
    if not isinstance(value, str):
        raise PluginFailure(
            "PIXABAY_INVALID_RESPONSE",
            "Pixabay tags must be a string when present.",
            retryable=True,
        )
    tags: list[str] = []
    seen: set[str] = set()
    for part in value.split(","):
        tag = " ".join(part.strip().split())
        key = tag.lower()
        if tag and key not in seen:
            seen.add(key)
            tags.append(tag)
    return tags[:20]


def creator_profile_url(raw: dict[str, Any]) -> str | None:
    user = raw.get("user")
    user_id = raw.get("user_id")
    if (
        not isinstance(user, str)
        or not user.strip()
        or isinstance(user_id, bool)
        or not isinstance(user_id, int)
        or user_id <= 0
    ):
        return None
    encoded = quote(user.strip(), safe="-_")
    return f"https://pixabay.com/users/{encoded}-{user_id}/"


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


def validate_provider_cache(raw: Any) -> Path:
    if not isinstance(raw, str) or not raw.strip():
        raise PluginFailure(
            "INVALID_PROVIDER_CACHE",
            "provider_cache must be a non-empty absolute directory path.",
        )
    path = Path(raw).expanduser()
    if not path.is_absolute() or not path.is_dir():
        raise PluginFailure(
            "INVALID_PROVIDER_CACHE",
            "provider_cache must be an existing absolute directory.",
        )
    return path.resolve()


def parse_selection_ref(selection_ref: Any) -> tuple[str, str]:
    value = require_non_empty_string(selection_ref, "selection_ref")
    parts = value.split(":")
    if len(parts) != 3 or parts[0] != PROVIDER_ID or parts[1] not in {"video", "image"}:
        raise PluginFailure(
            "INVALID_SELECTION_REF",
            "selection_ref must be pixabay:video:<id> or pixabay:image:<id>.",
        )
    asset_id = parts[2]
    if not asset_id.isdigit() or int(asset_id) <= 0:
        raise PluginFailure(
            "INVALID_SELECTION_REF",
            "selection_ref asset id must be a positive integer.",
        )
    return parts[1], asset_id


def validate_media_url(url: Any) -> str:
    value = require_non_empty_string(url, "media URL")
    parsed = urlparse(value)
    host = (parsed.hostname or "").lower()
    if parsed.scheme != "https" or host not in ALLOWED_MEDIA_HOSTS:
        raise PluginFailure(
            "PIXABAY_MEDIA_HOST_NOT_ALLOWED",
            f"Pixabay media URL must use HTTPS on an allowed host, found {host or 'unknown'}.",
            retryable=False,
        )
    return value


def preferred_video_rendition(
    raw_video: Any,
    quality_mode: str,
    *,
    require_url: bool = True,
) -> dict[str, Any]:
    if quality_mode not in SUPPORTED_QUALITY_MODES:
        raise PluginFailure(
            "INVALID_REQUEST",
            f"quality_mode must be one of {sorted(SUPPORTED_QUALITY_MODES)}.",
        )
    if not isinstance(raw_video, dict):
        raise PluginFailure(
            "PIXABAY_INVALID_RESPONSE",
            "Pixabay video detail must be an object.",
            retryable=True,
        )

    videos = raw_video.get("videos")
    if not isinstance(videos, dict):
        raise PluginFailure(
            "PIXABAY_INVALID_RESPONSE",
            "Pixabay video detail must contain a videos object.",
            retryable=True,
        )

    order = ("large", "medium", "small", "tiny") if quality_mode == "high" else (
        "medium",
        "small",
        "large",
        "tiny",
    )
    for name in order:
        rendition = videos.get(name)
        if not isinstance(rendition, dict):
            continue
        url = rendition.get("url")
        thumbnail = rendition.get("thumbnail")
        if require_url and (not isinstance(url, str) or not url.strip()):
            continue
        if not require_url and (not isinstance(thumbnail, str) or not thumbnail.strip()):
            continue
        result = dict(rendition)
        result["name"] = name
        if require_url:
            result["url"] = validate_media_url(url)
        return result

    raise PluginFailure(
        "PIXABAY_NO_DOWNLOADABLE_VIDEO",
        "Pixabay returned no usable video rendition.",
        retryable=True,
        suggested_fallback="retry-selected-asset",
    )


def preferred_image_source(raw_image: Any, quality_mode: str) -> dict[str, Any]:
    if quality_mode not in SUPPORTED_QUALITY_MODES:
        raise PluginFailure(
            "INVALID_REQUEST",
            f"quality_mode must be one of {sorted(SUPPORTED_QUALITY_MODES)}.",
        )
    if not isinstance(raw_image, dict):
        raise PluginFailure(
            "PIXABAY_INVALID_RESPONSE",
            "Pixabay image detail must be an object.",
            retryable=True,
        )

    if quality_mode == "high":
        choices = (
            ("imageURL", "original"),
            ("fullHDURL", "full_hd"),
            ("largeImageURL", "large"),
            ("webformatURL", "web"),
        )
    else:
        choices = (
            ("largeImageURL", "large"),
            ("webformatURL", "web"),
            ("imageURL", "original"),
        )

    for key, label in choices:
        value = raw_image.get(key)
        if isinstance(value, str) and value.strip():
            width = None
            height = None
            if key == "imageURL":
                width = optional_positive_int(raw_image.get("imageWidth"), "image.imageWidth")
                height = optional_positive_int(raw_image.get("imageHeight"), "image.imageHeight")
            elif key == "webformatURL":
                width = optional_positive_int(
                    raw_image.get("webformatWidth"), "image.webformatWidth"
                )
                height = optional_positive_int(
                    raw_image.get("webformatHeight"), "image.webformatHeight"
                )
            return {
                "url": validate_media_url(value),
                "quality": label,
                "width": width,
                "height": height,
            }

    raise PluginFailure(
        "PIXABAY_NO_DOWNLOADABLE_IMAGE",
        "Pixabay returned no downloadable image source.",
        retryable=True,
        suggested_fallback="retry-selected-asset",
    )


def extract_single_hit(response: PixabayResponse, asset_id: str, label: str) -> dict[str, Any]:
    hits = require_list(response.data.get("hits"), "hits")
    for raw in hits:
        if not isinstance(raw, dict):
            continue
        if str(raw.get("id", "")).strip() == asset_id:
            return raw
    raise PluginFailure(
        "PIXABAY_INVALID_RESPONSE",
        f"Pixabay did not return {label} id {asset_id}.",
        retryable=True,
    )


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

    relative_output = f"selected/pixabay-{media_type}-{asset_id}.{extension}"
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

    temp_path = temp_root / f"pixabay-{media_type}-{asset_id}.{extension}.part"
    return relative_output, destination, temp_path


def image_extension_from_url(url: str) -> str:
    suffix = Path(urlparse(url).path).suffix.lower().lstrip(".")
    if suffix in {"jpg", "jpeg", "png", "webp"}:
        return suffix
    return "jpg"


def execute_fetch_selected(
    payload: Any,
    state: PluginState,
    client_factory: Callable[[str, Path | None], Any] = PixabayClient,
) -> dict[str, Any]:
    if not isinstance(payload, dict):
        raise PluginFailure(
            "INVALID_REQUEST",
            "visual.fetch_selected payload must be an object.",
        )

    selection_ref = require_non_empty_string(payload.get("selection_ref"), "selection_ref")
    media_type, asset_id = parse_selection_ref(selection_ref)
    quality_mode = payload.get("quality_mode", "standard")
    if quality_mode not in SUPPORTED_QUALITY_MODES:
        raise PluginFailure(
            "INVALID_REQUEST",
            f"quality_mode must be one of {sorted(SUPPORTED_QUALITY_MODES)}.",
        )

    api_key_env = state.settings["api_key_env"]
    api_key = os.environ.get(api_key_env, "").strip()
    if not api_key:
        raise PluginFailure(
            "CREDENTIAL_MISSING",
            f"Pixabay API key is missing. Set machine-local environment variable {api_key_env}.",
            retryable=False,
            suggested_fallback="local-library",
        )

    client = client_factory(api_key, state.provider_cache)

    if media_type == "video":
        raw = extract_single_hit(client.get_video(asset_id), asset_id, "video")
        selected = preferred_video_rendition(raw, quality_mode)
        relative_output, destination, temp_path = output_paths_for_selection(
            state, "video", asset_id, "mp4"
        )
        client.download_to_path(selected["url"], temp_path)
        os.replace(temp_path, destination)

        creator_name = optional_non_empty_string(raw.get("user"), "video.user")
        creator_url = creator_profile_url(raw)
        source_page_url = optional_non_empty_string(raw.get("pageURL"), "video.pageURL")
        attribution = (
            f"Video by {creator_name} on Pixabay"
            if creator_name
            else "Video provided by Pixabay"
        )
        return {
            "source_provider": PROVIDER_ID,
            "source_asset_id": asset_id,
            "selection_ref": selection_ref,
            "media_type": "video",
            "relative_output": relative_output,
            "width": optional_positive_int(selected.get("width"), "video.rendition.width"),
            "height": optional_positive_int(selected.get("height"), "video.rendition.height"),
            "duration": optional_positive_number(raw.get("duration"), "video.duration"),
            "provenance": {
                "provider": PROVIDER_ID,
                "provider_asset_id": asset_id,
                "source_page_url": source_page_url,
                "creator_name": creator_name,
                "creator_url": creator_url,
                "attribution": attribution,
                "license": "Pixabay Content License",
                "quality_mode": quality_mode,
                "provider_rendition": selected["name"],
            },
        }

    raw = extract_single_hit(client.get_image(asset_id), asset_id, "image")
    selected = preferred_image_source(raw, quality_mode)
    extension = image_extension_from_url(selected["url"])
    relative_output, destination, temp_path = output_paths_for_selection(
        state, "image", asset_id, extension
    )
    client.download_to_path(selected["url"], temp_path)
    os.replace(temp_path, destination)

    creator_name = optional_non_empty_string(raw.get("user"), "image.user")
    creator_url = creator_profile_url(raw)
    source_page_url = optional_non_empty_string(raw.get("pageURL"), "image.pageURL")
    attribution = (
        f"Image by {creator_name} on Pixabay"
        if creator_name
        else "Image provided by Pixabay"
    )
    return {
        "source_provider": PROVIDER_ID,
        "source_asset_id": asset_id,
        "selection_ref": selection_ref,
        "media_type": "image",
        "relative_output": relative_output,
        "width": selected["width"],
        "height": selected["height"],
        "duration": None,
        "provenance": {
            "provider": PROVIDER_ID,
            "provider_asset_id": asset_id,
            "source_page_url": source_page_url,
            "creator_name": creator_name,
            "creator_url": creator_url,
            "attribution": attribution,
            "license": "Pixabay Content License",
            "quality_mode": selected["quality"],
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
        return None
    return "horizontal" if ratio > 1.0 else "vertical"


def normalize_provider_query(query: str) -> str:
    normalized = " ".join(query.split())
    if len(normalized) <= MAX_QUERY_CHARS:
        return normalized
    shortened = normalized[:MAX_QUERY_CHARS].rstrip()
    if " " in shortened:
        candidate = shortened.rsplit(" ", 1)[0].strip()
        if candidate:
            shortened = candidate
    if not shortened:
        raise PluginFailure("INVALID_REQUEST", "search query became empty after normalization.")
    return shortened


def handle_request(
    request: Any,
    state: PluginState,
    client_factory: Callable[[str, Path | None], Any] = PixabayClient,
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
                    f"Unsupported Pixabay operation: {operation}.",
                )
        elif method == "plugin.cancel":
            result = {
                "cancelled": False,
                "reason": "Pixabay requests are synchronous; runtime termination is the cancellation fallback.",
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
                    "PIXABAY_INTERNAL_ERROR",
                    f"Unexpected Pixabay plugin error: {error}",
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
            "PIXABAY_INVALID_RESPONSE",
            f"{label} must be a provider identifier.",
            retryable=True,
        )
    result = str(value).strip()
    if not result:
        raise PluginFailure(
            "PIXABAY_INVALID_RESPONSE",
            f"{label} must not be empty.",
            retryable=True,
        )
    return result


def optional_positive_int(value: Any, label: str) -> int | None:
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise PluginFailure(
            "PIXABAY_INVALID_RESPONSE",
            f"{label} must be a positive integer when present.",
            retryable=True,
        )
    return value


def optional_positive_number(value: Any, label: str) -> float | None:
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, (int, float)) or value <= 0:
        raise PluginFailure(
            "PIXABAY_INVALID_RESPONSE",
            f"{label} must be positive when present.",
            retryable=True,
        )
    return float(value)


def optional_non_empty_string(value: Any, label: str) -> str | None:
    if value is None:
        return None
    if not isinstance(value, str) or not value.strip():
        raise PluginFailure(
            "PIXABAY_INVALID_RESPONSE",
            f"{label} must be a non-empty string when present.",
            retryable=True,
        )
    return value.strip()


def require_list(value: Any, label: str) -> list[Any]:
    if not isinstance(value, list):
        raise PluginFailure(
            "PIXABAY_INVALID_RESPONSE",
            f"Pixabay response {label} must be an array.",
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
