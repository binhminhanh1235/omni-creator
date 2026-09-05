from __future__ import annotations

import base64
import json
import socket
from dataclasses import dataclass
from typing import Any, Callable
from urllib.error import HTTPError, URLError
from urllib.parse import urlparse
from urllib.request import Request, urlopen

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


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
class ProviderImageResponse:
    image_bytes: bytes
    mime_type: str
    width: int
    height: int
    request_id: str | None
    model_id: str
    model_version: str


def require_text(value: Any, label: str, *, code: str = "INVALID_INPUT") -> str:
    if not isinstance(value, str) or not value.strip():
        raise PluginFailure(code, f"{label} must be a non-empty string.")
    return value.strip()


def validate_endpoint(value: Any) -> str:
    endpoint = require_text(value, "api_endpoint", code="INVALID_CONFIGURATION")
    parsed = urlparse(endpoint)
    host = (parsed.hostname or "").lower()
    if not host:
        raise PluginFailure("INVALID_CONFIGURATION", "api_endpoint must include a host.")
    loopback = host in {"localhost", "127.0.0.1", "::1"}
    if parsed.scheme != "https" and not (parsed.scheme == "http" and loopback):
        raise PluginFailure(
            "INVALID_CONFIGURATION",
            "api_endpoint must use HTTPS; HTTP is allowed only for loopback mock/development endpoints.",
        )
    if parsed.username or parsed.password:
        raise PluginFailure(
            "INVALID_CONFIGURATION",
            "api_endpoint must not contain embedded credentials.",
        )
    return endpoint


def inspect_image(data: bytes) -> tuple[str, int, int]:
    if data.startswith(PNG_SIGNATURE):
        if len(data) < 24 or data[12:16] != b"IHDR":
            raise PluginFailure(
                "INVALID_IMAGE_PAYLOAD",
                "Generated-image API returned an invalid PNG payload.",
                retryable=True,
            )
        width = int.from_bytes(data[16:20], "big")
        height = int.from_bytes(data[20:24], "big")
        if width <= 0 or height <= 0:
            raise PluginFailure(
                "INVALID_IMAGE_PAYLOAD",
                "Generated-image API returned invalid PNG dimensions.",
                retryable=True,
            )
        return "image/png", width, height

    if data.startswith(b"\xff\xd8"):
        return "image/jpeg", *jpeg_dimensions(data)

    raise PluginFailure(
        "INVALID_IMAGE_PAYLOAD",
        "Generated-image API returned an unsupported or invalid image payload.",
        retryable=True,
    )


def jpeg_dimensions(data: bytes) -> tuple[int, int]:
    index = 2
    sof = {0xC0, 0xC1, 0xC2, 0xC3, 0xC5, 0xC6, 0xC7, 0xC9, 0xCA, 0xCB, 0xCD, 0xCE, 0xCF}
    while index + 4 <= len(data):
        if data[index] != 0xFF:
            index += 1
            continue
        marker = data[index + 1]
        index += 2
        if marker in {0xD8, 0xD9}:
            continue
        if index + 2 > len(data):
            break
        length = int.from_bytes(data[index:index + 2], "big")
        if length < 2 or index + length > len(data):
            break
        if marker in sof:
            if length < 7:
                break
            height = int.from_bytes(data[index + 3:index + 5], "big")
            width = int.from_bytes(data[index + 5:index + 7], "big")
            if width > 0 and height > 0:
                return width, height
            break
        index += length
    raise PluginFailure(
        "INVALID_IMAGE_PAYLOAD",
        "Generated-image API returned an invalid JPEG payload.",
        retryable=True,
    )


def extension_for_mime(mime_type: str) -> str:
    if mime_type == "image/png":
        return "png"
    if mime_type == "image/jpeg":
        return "jpg"
    raise PluginFailure("INVALID_IMAGE_PAYLOAD", "Unsupported generated image MIME type.")


def _retry_after_seconds(error: HTTPError) -> int | None:
    raw = error.headers.get("Retry-After") if error.headers is not None else None
    if not raw:
        return None
    try:
        return max(0, int(raw))
    except (TypeError, ValueError):
        return None


def map_http_error(error: HTTPError) -> PluginFailure:
    code = int(error.code)
    retry_after = _retry_after_seconds(error)
    if code in {401, 403}:
        return PluginFailure(
            "AUTHENTICATION_FAILED",
            "Generated-image API rejected the configured credential.",
            retryable=False,
        )
    if code == 429:
        return PluginFailure(
            "RATE_LIMITED",
            "Generated-image API rate limit was reached.",
            retryable=True,
            retry_after_seconds=retry_after,
            suggested_fallback="retry",
        )
    if code >= 500 or code in {408, 425}:
        return PluginFailure(
            "PROVIDER_SERVER_ERROR",
            f"Generated-image API returned HTTP {code}.",
            retryable=True,
            retry_after_seconds=retry_after,
            suggested_fallback="retry",
        )
    return PluginFailure(
        "PROVIDER_HTTP_ERROR",
        f"Generated-image API returned HTTP {code}.",
        retryable=False,
    )


class ApiImageClient:
    def __init__(
        self,
        api_key: str,
        endpoint: str,
        timeout_seconds: int,
        opener: Callable[..., Any] = urlopen,
    ) -> None:
        if not api_key.strip():
            raise PluginFailure("CREDENTIAL_MISSING", "Generated-image API credential is missing.")
        self._api_key = api_key.strip()
        self._endpoint = validate_endpoint(endpoint)
        if type(timeout_seconds) is not int or not 1 <= timeout_seconds <= 300:
            raise PluginFailure(
                "INVALID_CONFIGURATION",
                "timeout_seconds must be an integer between 1 and 300.",
            )
        self._timeout_seconds = timeout_seconds
        self._opener = opener

    def generate(
        self,
        *,
        prompt: str,
        negative_prompt: str | None,
        width: int,
        height: int,
        seed: int | None,
        model_id: str,
        model_version: str,
    ) -> ProviderImageResponse:
        payload: dict[str, Any] = {
            "model": model_id,
            "prompt": prompt,
            "size": f"{width}x{height}",
            "response_format": "b64_json",
        }
        if negative_prompt:
            payload["negative_prompt"] = negative_prompt
        if seed is not None:
            payload["seed"] = seed

        request = Request(
            self._endpoint,
            data=json.dumps(payload, separators=(",", ":"), ensure_ascii=False).encode("utf-8"),
            method="POST",
            headers={
                "Authorization": f"Bearer {self._api_key}",
                "Content-Type": "application/json",
                "Accept": "application/json",
                "User-Agent": "OmniCreator-Generated-Image-API/1.0",
            },
        )
        try:
            with self._opener(request, timeout=self._timeout_seconds) as response:
                raw = response.read()
                headers = getattr(response, "headers", {})
        except HTTPError as error:
            raise map_http_error(error) from error
        except (URLError, TimeoutError, socket.timeout, OSError) as error:
            raise PluginFailure(
                "NETWORK_ERROR",
                "Generated-image API request failed.",
                retryable=True,
                suggested_fallback="retry",
            ) from error

        try:
            data = json.loads(raw.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise PluginFailure(
                "MALFORMED_PROVIDER_RESPONSE",
                "Generated-image API returned malformed JSON.",
                retryable=True,
            ) from error
        if not isinstance(data, dict):
            raise PluginFailure(
                "MALFORMED_PROVIDER_RESPONSE",
                "Generated-image API response must be an object.",
                retryable=True,
            )

        items = data.get("data")
        if not isinstance(items, list) or not items or not isinstance(items[0], dict):
            raise PluginFailure(
                "IMAGE_MISSING",
                "Generated-image API response did not contain an image.",
                retryable=True,
            )
        encoded = items[0].get("b64_json")
        if not isinstance(encoded, str) or not encoded.strip():
            raise PluginFailure(
                "IMAGE_MISSING",
                "Generated-image API response did not contain b64_json image data.",
                retryable=True,
            )
        try:
            image_bytes = base64.b64decode(encoded, validate=True)
        except (ValueError, base64.binascii.Error) as error:
            raise PluginFailure(
                "INVALID_IMAGE_PAYLOAD",
                "Generated-image API returned invalid base64 image data.",
                retryable=True,
            ) from error

        mime_type, actual_width, actual_height = inspect_image(image_bytes)
        if actual_width != width or actual_height != height:
            raise PluginFailure(
                "INVALID_IMAGE_PAYLOAD",
                f"Generated image dimensions {actual_width}x{actual_height} do not match requested {width}x{height}.",
                retryable=True,
            )

        response_model = data.get("model")
        if response_model is not None and (
            not isinstance(response_model, str) or response_model.strip() != model_id
        ):
            raise PluginFailure(
                "MODEL_MISMATCH",
                "Generated-image API response model does not match the resolved model.",
            )
        response_version = data.get("model_version")
        if response_version is not None and (
            not isinstance(response_version, str) or response_version.strip() != model_version
        ):
            raise PluginFailure(
                "MODEL_MISMATCH",
                "Generated-image API response model version does not match the resolved model version.",
            )

        request_id = data.get("request_id")
        if not isinstance(request_id, str) or not request_id.strip():
            request_id = headers.get("x-request-id") if hasattr(headers, "get") else None
        if not isinstance(request_id, str) or not request_id.strip():
            request_id = None

        return ProviderImageResponse(
            image_bytes=image_bytes,
            mime_type=mime_type,
            width=actual_width,
            height=actual_height,
            request_id=request_id.strip() if request_id else None,
            model_id=model_id,
            model_version=model_version,
        )
