import importlib.util
import json
import os
import pathlib
import sys
import tempfile
import unittest
from unittest.mock import patch

PLUGIN_PATH = pathlib.Path(__file__).resolve().parents[1] / "plugin.py"
SPEC = importlib.util.spec_from_file_location("unsplash_plugin", PLUGIN_PATH)
plugin = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = plugin
SPEC.loader.exec_module(plugin)


PHOTO = {
    "id": "abc_DEF-123",
    "width": 4000,
    "height": 2667,
    "description": "Morning light through a quiet forest",
    "alt_description": "sunlight through forest trees",
    "urls": {
        "raw": "https://images.unsplash.com/photo-abc?ixid=raw123",
        "full": "https://images.unsplash.com/photo-abc?ixid=full123&q=85",
        "regular": "https://images.unsplash.com/photo-abc?ixid=regular123&w=1080",
        "small": "https://images.unsplash.com/photo-abc?ixid=small123&w=400",
        "thumb": "https://images.unsplash.com/photo-abc?ixid=thumb123&w=200",
    },
    "links": {
        "html": "https://unsplash.com/photos/abc_DEF-123",
        "download": "https://unsplash.com/photos/abc_DEF-123/download",
        "download_location": "https://api.unsplash.com/photos/abc_DEF-123/download?ixid=tracking123",
    },
    "user": {
        "name": "Annie Example",
        "username": "annie",
        "links": {
            "html": "https://unsplash.com/@annie",
        },
    },
    "tags": [
        {"title": "forest"},
        {"title": "morning light"},
        {"title": "forest"},
    ],
}


class FakeClient:
    def __init__(self, access_key):
        self.access_key = access_key
        self.searches = []
        self.events = []
        self.downloads = []
        self.fail_tracking = False

    def search_photos(self, query, **kwargs):
        self.searches.append((query, kwargs))
        return plugin.UnsplashResponse(
            data={"total": 1, "total_pages": 1, "results": [PHOTO]},
            quota={"limit": 50, "remaining": 49},
        )

    def get_photo(self, asset_id):
        self.events.append(("detail", asset_id))
        return plugin.UnsplashResponse(
            data=PHOTO,
            quota={"limit": 50, "remaining": 48},
        )

    def track_download(self, download_location):
        self.events.append(("track", download_location))
        if self.fail_tracking:
            raise plugin.PluginFailure(
                "UNSPLASH_DOWNLOAD_TRACKING_ERROR",
                "tracking failed",
                retryable=True,
            )
        return plugin.UnsplashResponse(
            data={"url": "https://images.unsplash.com/tracked"},
            quota={"limit": 50, "remaining": 47},
        )

    def download_to_path(self, url, destination):
        self.events.append(("download", url))
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(b"unsplash fixture bytes")
        self.downloads.append((url, destination))


class FakeResponse:
    def __init__(self, payload, headers=None):
        if isinstance(payload, bytes):
            self._payload = payload
        else:
            self._payload = json.dumps(payload).encode("utf-8")
        self.headers = headers or {}

    def read(self, size=-1):
        if size is None or size < 0:
            payload = self._payload
            self._payload = b""
            return payload
        payload = self._payload[:size]
        self._payload = self._payload[size:]
        return payload

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        return False


class RecordingOpener:
    def __init__(self, payload):
        self.payload = payload
        self.requests = []

    def __call__(self, request, timeout):
        self.requests.append((request, timeout))
        return FakeResponse(
            self.payload,
            {"X-Ratelimit-Limit": "50", "X-Ratelimit-Remaining": "49"},
        )


class UnsplashPluginTests(unittest.TestCase):
    def test_client_search_uses_v1_auth_and_documented_filters(self):
        opener = RecordingOpener({"total": 0, "total_pages": 0, "results": []})
        client = plugin.UnsplashClient("machine-secret", opener=opener)

        response = client.search_photos(
            "quiet forest",
            orientation="landscape",
            order_by="relevant",
            content_filter="high",
            per_page=8,
        )

        self.assertEqual(response.quota["remaining"], 49)
        self.assertEqual(len(opener.requests), 1)
        request, timeout = opener.requests[0]
        self.assertEqual(timeout, plugin.REQUEST_TIMEOUT_SECONDS)
        self.assertIn("/search/photos?", request.full_url)
        self.assertIn("query=quiet+forest", request.full_url)
        self.assertIn("orientation=landscape", request.full_url)
        self.assertIn("content_filter=high", request.full_url)
        self.assertEqual(request.get_header("Authorization"), "Client-ID machine-secret")
        self.assertEqual(request.get_header("Accept-version"), "v1")

    def test_track_download_preserves_provider_query_and_authorizes(self):
        opener = RecordingOpener({"url": "https://images.unsplash.com/tracked"})
        client = plugin.UnsplashClient("machine-secret", opener=opener)
        location = (
            "https://api.unsplash.com/photos/abc_DEF-123/download"
            "?ixid=tracking123&another=value"
        )

        client.track_download(location)

        request, _ = opener.requests[0]
        self.assertEqual(request.full_url, location)
        self.assertEqual(request.get_header("Authorization"), "Client-ID machine-secret")

    def test_photo_normalization_hotlinks_preview_and_adds_utm_attribution(self):
        candidate = plugin.normalize_photo("SC17", PHOTO)
        encoded = json.dumps(candidate)

        self.assertEqual(candidate["candidate_id"], "unsplash:image:abc_DEF-123")
        self.assertEqual(candidate["source_provider"], "unsplash")
        self.assertEqual(candidate["creator_name"], "Annie Example")
        self.assertEqual(candidate["tags"], ["forest", "morning light"])
        self.assertEqual(
            candidate["previews"][0]["url"],
            PHOTO["urls"]["regular"],
        )
        self.assertIn("utm_source=omnicreator", candidate["source_page_url"])
        self.assertIn("utm_medium=referral", candidate["creator_url"])
        self.assertNotIn("download_location", encoded)
        self.assertNotIn("tracking123", encoded)
        self.assertNotIn("full123", encoded)

    def test_visual_resolve_deduplicates_and_maps_scene_orientation(self):
        state = plugin.PluginState()
        state.initialize({"settings": {"per_query": 4}})
        fake = FakeClient("machine-secret")

        with patch.dict(os.environ, {"UNSPLASH_ACCESS_KEY": "machine-secret"}, clear=False):
            result = plugin.execute_visual_resolve(
                {
                    "scene": {
                        "id": "SC17",
                        "aspect_ratio": "9:16",
                        "search_queries": [
                            "quiet forest",
                            "quiet forest",
                            "morning trees",
                        ],
                    }
                },
                state,
                client_factory=lambda access_key: fake,
            )

        self.assertEqual(result["queries"], ["quiet forest", "morning trees"])
        self.assertEqual(len(result["candidates"]), 1)
        self.assertEqual(fake.searches[0][1]["orientation"], "portrait")
        self.assertTrue(result["hotlink_previews"])
        self.assertEqual(result["media_type"], "image")

    def test_unsplash_rejects_video_only_request_with_provider_fallback(self):
        state = plugin.PluginState()
        fake = FakeClient("machine-secret")

        with patch.dict(os.environ, {"UNSPLASH_ACCESS_KEY": "machine-secret"}, clear=False):
            with self.assertRaises(plugin.PluginFailure) as raised:
                plugin.execute_visual_resolve(
                    {
                        "scene": {
                            "id": "SC17",
                            "search_queries": ["forest"],
                        },
                        "media_type": "video",
                    },
                    state,
                    client_factory=lambda access_key: fake,
                )

        self.assertEqual(raised.exception.code, "UNSUPPORTED_MEDIA_TYPE")
        self.assertEqual(raised.exception.suggested_fallback, "next-stock-provider")

    def test_fetch_selected_tracks_before_copy_and_keeps_tracking_url_noncanonical(self):
        with tempfile.TemporaryDirectory() as root:
            output = pathlib.Path(root) / "output"
            temp = pathlib.Path(root) / "temp"
            output.mkdir()
            temp.mkdir()

            state = plugin.PluginState()
            state.initialize({"job_workspace": {"output": str(output), "temp": str(temp)}})
            fake = FakeClient("machine-secret")

            with patch.dict(os.environ, {"UNSPLASH_ACCESS_KEY": "machine-secret"}, clear=False):
                result = plugin.execute_fetch_selected(
                    {
                        "selection_ref": "unsplash:image:abc_DEF-123",
                        "quality_mode": "high",
                    },
                    state,
                    client_factory=lambda access_key: fake,
                )

            self.assertEqual(
                [event[0] for event in fake.events],
                ["detail", "track", "download"],
            )
            self.assertEqual(fake.events[1][1], PHOTO["links"]["download_location"])
            self.assertEqual(fake.events[2][1], PHOTO["urls"]["full"])
            self.assertTrue((output / result["relative_output"]).is_file())
            self.assertEqual((result["width"], result["height"]), (4000, 2667))
            self.assertTrue(result["provenance"]["download_tracked"])
            self.assertEqual(result["provenance"]["creator_name"], "Annie Example")
            self.assertEqual(result["provenance"]["attribution"], "Photo by Annie Example on Unsplash")
            self.assertIn("utm_source=omnicreator", result["provenance"]["creator_url"])
            self.assertIn("utm_source=omnicreator", result["provenance"]["unsplash_url"])

            encoded = json.dumps(result)
            self.assertNotIn("download_location", encoded)
            self.assertNotIn("tracking123", encoded)
            self.assertNotIn("full123", encoded)

    def test_tracking_failure_prevents_selected_photo_copy(self):
        with tempfile.TemporaryDirectory() as root:
            output = pathlib.Path(root) / "output"
            temp = pathlib.Path(root) / "temp"
            output.mkdir()
            temp.mkdir()

            state = plugin.PluginState()
            state.initialize({"job_workspace": {"output": str(output), "temp": str(temp)}})
            fake = FakeClient("machine-secret")
            fake.fail_tracking = True

            with patch.dict(os.environ, {"UNSPLASH_ACCESS_KEY": "machine-secret"}, clear=False):
                with self.assertRaises(plugin.PluginFailure) as raised:
                    plugin.execute_fetch_selected(
                        {"selection_ref": "unsplash:image:abc_DEF-123"},
                        state,
                        client_factory=lambda access_key: fake,
                    )

            self.assertEqual(raised.exception.code, "UNSPLASH_DOWNLOAD_TRACKING_ERROR")
            self.assertEqual([event[0] for event in fake.events], ["detail", "track"])
            self.assertEqual(list(output.rglob("*")), [])

    def test_attribution_and_media_urls_are_host_constrained(self):
        with self.assertRaises(plugin.PluginFailure):
            plugin.validate_media_url("https://evil.example/photo.jpg")
        with self.assertRaises(plugin.PluginFailure):
            plugin.validate_download_location(
                "https://evil.example/photos/abc/download?ixid=x"
            )
        with self.assertRaises(plugin.PluginFailure):
            plugin.with_utm("https://evil.example/@creator")

    def test_secret_values_in_portable_settings_are_rejected(self):
        state = plugin.PluginState()
        for key in ("access_key", "client_id", "api_key"):
            with self.subTest(key=key):
                with self.assertRaises(plugin.PluginFailure) as raised:
                    state.initialize({"settings": {key: "must-not-be-stored"}})
                self.assertEqual(raised.exception.code, "SECRET_SETTING_REJECTED")

    def test_health_reports_machine_local_credential_state(self):
        state = plugin.PluginState()
        with patch.dict(os.environ, {}, clear=True):
            missing = plugin.health_result(state)
        with patch.dict(
            os.environ,
            {"UNSPLASH_ACCESS_KEY": "machine-secret"},
            clear=True,
        ):
            ready = plugin.health_result(state)

        self.assertEqual(missing["status"], "needs_attention")
        self.assertFalse(missing["credential_present"])
        self.assertEqual(ready["status"], "ready")
        self.assertTrue(ready["credential_present"])
        self.assertTrue(ready["download_tracking_required"])

    def test_protocol_execute_returns_plugin_api_v1_response(self):
        state = plugin.PluginState()
        fake = FakeClient("machine-secret")
        request = {
            "api_version": 1,
            "request_id": "req_unsplash",
            "method": "plugin.execute",
            "params": {
                "operation": "visual.resolve",
                "payload": {
                    "scene": {
                        "id": "SC17",
                        "aspect_ratio": "1:1",
                        "search_queries": ["forest"],
                    }
                },
            },
        }

        with patch.dict(os.environ, {"UNSPLASH_ACCESS_KEY": "machine-secret"}, clear=False):
            response, shutdown = plugin.handle_request(
                request,
                state,
                client_factory=lambda access_key: fake,
            )

        self.assertFalse(shutdown)
        self.assertEqual(response["api_version"], 1)
        self.assertEqual(response["request_id"], "req_unsplash")
        self.assertEqual(fake.searches[0][1]["orientation"], "squarish")

    def test_selection_ref_is_provider_bound_and_workspace_is_required(self):
        with self.assertRaises(plugin.PluginFailure):
            plugin.parse_selection_ref("pixabay:image:123")

        state = plugin.PluginState()
        fake = FakeClient("machine-secret")
        with patch.dict(os.environ, {"UNSPLASH_ACCESS_KEY": "machine-secret"}, clear=False):
            with self.assertRaises(plugin.PluginFailure) as raised:
                plugin.execute_fetch_selected(
                    {"selection_ref": "unsplash:image:abc_DEF-123"},
                    state,
                    client_factory=lambda access_key: fake,
                )
        self.assertEqual(raised.exception.code, "WORKSPACE_REQUIRED")
        self.assertEqual([event[0] for event in fake.events], ["detail", "track"])


if __name__ == "__main__":
    unittest.main()
