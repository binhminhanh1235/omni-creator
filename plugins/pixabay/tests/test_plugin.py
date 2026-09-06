import importlib.util
import json
import os
import pathlib
import sys
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError

PLUGIN_PATH = pathlib.Path(__file__).resolve().parents[1] / "plugin.py"
SPEC = importlib.util.spec_from_file_location("pixabay_plugin", PLUGIN_PATH)
plugin = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = plugin
SPEC.loader.exec_module(plugin)


class FakeResponse:
    def __init__(self, data, headers=None):
        self._payload = json.dumps(data).encode("utf-8")
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
    def __init__(self, data):
        self.data = data
        self.requests = []

    def __call__(self, request, timeout):
        self.requests.append((request, timeout))
        return FakeResponse(
            self.data,
            {
                "X-RateLimit-Limit": "100",
                "X-RateLimit-Remaining": "99",
                "X-RateLimit-Reset": "42",
            },
        )


IMAGE_FIXTURE = {
    "id": 195893,
    "pageURL": "https://pixabay.com/photos/blossom-bloom-flower-195893/",
    "type": "photo",
    "tags": "blossom, bloom, flower",
    "previewURL": "https://cdn.pixabay.com/photo/preview-195893_150.jpg",
    "previewWidth": 150,
    "previewHeight": 84,
    "webformatURL": "https://pixabay.com/get/image-195893_640.jpg",
    "webformatWidth": 640,
    "webformatHeight": 360,
    "largeImageURL": "https://pixabay.com/get/image-195893_1280.jpg",
    "fullHDURL": "https://pixabay.com/get/image-195893_1920.jpg",
    "imageURL": "https://pixabay.com/get/image-195893.jpg",
    "imageWidth": 4000,
    "imageHeight": 2250,
    "user_id": 48777,
    "user": "PhotoCreator",
}

VIDEO_FIXTURE = {
    "id": 1253,
    "pageURL": "https://pixabay.com/videos/forest-mist-trees-1253/",
    "type": "film",
    "tags": "forest, mist, trees",
    "duration": 12,
    "user_id": 321,
    "user": "VideoCreator",
    "videos": {
        "large": {
            "url": "https://cdn.pixabay.com/video/2026/large-1253.mp4",
            "width": 3840,
            "height": 2160,
            "size": 90000000,
            "thumbnail": "https://cdn.pixabay.com/video/2026/large-1253.jpg",
        },
        "medium": {
            "url": "https://cdn.pixabay.com/video/2026/medium-1253.mp4",
            "width": 1920,
            "height": 1080,
            "size": 30000000,
            "thumbnail": "https://cdn.pixabay.com/video/2026/medium-1253.jpg",
        },
        "small": {
            "url": "https://cdn.pixabay.com/video/2026/small-1253.mp4",
            "width": 1280,
            "height": 720,
            "size": 12000000,
            "thumbnail": "https://cdn.pixabay.com/video/2026/small-1253.jpg",
        },
        "tiny": {
            "url": "https://cdn.pixabay.com/video/2026/tiny-1253.mp4",
            "width": 640,
            "height": 360,
            "size": 5000000,
            "thumbnail": "https://cdn.pixabay.com/video/2026/tiny-1253.jpg",
        },
    },
}


class FakeClient:
    def __init__(self, api_key):
        self.api_key = api_key
        self.video_queries = []
        self.image_queries = []
        self.downloads = []

    def search_videos(self, query, **kwargs):
        self.video_queries.append((query, kwargs))
        return plugin.PixabayResponse(
            data={"hits": [VIDEO_FIXTURE]},
            quota={"limit": 100, "remaining": 98, "reset": 40},
            cache_hit=False,
        )

    def search_images(self, query, **kwargs):
        self.image_queries.append((query, kwargs))
        return plugin.PixabayResponse(
            data={"hits": [IMAGE_FIXTURE]},
            quota={"limit": 100, "remaining": 97, "reset": 40},
            cache_hit=True,
        )

    def get_video(self, asset_id):
        return plugin.PixabayResponse(
            data={"hits": [VIDEO_FIXTURE]},
            quota={"limit": 100, "remaining": 96, "reset": 40},
        )

    def get_image(self, asset_id):
        return plugin.PixabayResponse(
            data={"hits": [IMAGE_FIXTURE]},
            quota={"limit": 100, "remaining": 95, "reset": 40},
        )

    def download_to_path(self, url, destination):
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(b"fixture selected media")
        self.downloads.append((url, destination))


class PixabayPluginTests(unittest.TestCase):
    def test_client_uses_documented_query_shape_and_24_hour_cache(self):
        with tempfile.TemporaryDirectory() as root:
            opener = RecordingOpener({"hits": [IMAGE_FIXTURE]})
            cache = pathlib.Path(root)
            clock = lambda: 1000.0
            client = plugin.PixabayClient(
                "secret-key",
                cache_dir=cache,
                opener=opener,
                clock=clock,
            )

            first = client.search_images(
                "quiet forest dawn",
                orientation="horizontal",
                language="en",
                safe_search=True,
                order="popular",
                minimum_width=1280,
                minimum_height=720,
                per_page=8,
            )
            second = client.search_images(
                "quiet forest dawn",
                orientation="horizontal",
                language="en",
                safe_search=True,
                order="popular",
                minimum_width=1280,
                minimum_height=720,
                per_page=8,
            )

            self.assertFalse(first.cache_hit)
            self.assertTrue(second.cache_hit)
            self.assertEqual(len(opener.requests), 1)
            request, timeout = opener.requests[0]
            self.assertEqual(timeout, plugin.REQUEST_TIMEOUT_SECONDS)
            self.assertIn("/api/?", request.full_url)
            self.assertIn("key=secret-key", request.full_url)
            self.assertIn("q=quiet+forest+dawn", request.full_url)
            self.assertIn("orientation=horizontal", request.full_url)
            self.assertIn("safesearch=true", request.full_url)
            self.assertIn("min_width=1280", request.full_url)
            self.assertIn("min_height=720", request.full_url)
            self.assertEqual(first.quota["remaining"], 99)

            cache_files = list(cache.glob("*.json"))
            self.assertEqual(len(cache_files), 1)
            cache_text = cache_files[0].read_text(encoding="utf-8")
            self.assertNotIn("secret-key", cache_files[0].name)
            self.assertNotIn("secret-key", cache_text)
            self.assertEqual(plugin.CACHE_TTL_SECONDS, 86400)

    def test_expired_cache_refetches(self):
        with tempfile.TemporaryDirectory() as root:
            now = [1000.0]
            opener = RecordingOpener({"hits": [IMAGE_FIXTURE]})
            client = plugin.PixabayClient(
                "secret-key",
                cache_dir=pathlib.Path(root),
                opener=opener,
                clock=lambda: now[0],
            )
            kwargs = dict(
                orientation=None,
                language="en",
                safe_search=True,
                order="popular",
                minimum_width=0,
                minimum_height=0,
                per_page=8,
            )
            client.search_images("forest", **kwargs)
            now[0] += plugin.CACHE_TTL_SECONDS + 1
            client.search_images("forest", **kwargs)
            self.assertEqual(len(opener.requests), 2)

    def test_image_normalization_exposes_preview_not_download_urls(self):
        candidate = plugin.normalize_image("SC17", IMAGE_FIXTURE)
        encoded = json.dumps(candidate)

        self.assertEqual(candidate["candidate_id"], "pixabay:image:195893")
        self.assertEqual(candidate["creator_name"], "PhotoCreator")
        self.assertEqual(candidate["tags"], ["blossom", "bloom", "flower"])
        self.assertIn("preview-195893", candidate["previews"][0]["url"])
        self.assertNotIn("largeImageURL", encoded)
        self.assertNotIn("image-195893_1280.jpg", encoded)
        self.assertNotIn("image-195893.jpg", encoded)

    def test_video_normalization_exposes_thumbnail_not_video_url(self):
        candidate = plugin.normalize_video("SC17", VIDEO_FIXTURE)
        encoded = json.dumps(candidate)

        self.assertEqual(candidate["candidate_id"], "pixabay:video:1253")
        self.assertEqual((candidate["width"], candidate["height"]), (1920, 1080))
        self.assertIn("medium-1253.jpg", candidate["previews"][0]["url"])
        self.assertNotIn("medium-1253.mp4", encoded)
        self.assertNotIn("large-1253.mp4", encoded)

    def test_visual_resolve_deduplicates_and_maps_scene_orientation(self):
        state = plugin.PluginState()
        state.initialize({"settings": {"media_type": "both", "per_query": 4}})
        fake = FakeClient("machine-secret")

        with patch.dict(os.environ, {"PIXABAY_API_KEY": "machine-secret"}, clear=False):
            result = plugin.execute_visual_resolve(
                {
                    "scene": {
                        "id": "SC17",
                        "aspect_ratio": "16:9",
                        "search_queries": [
                            "quiet forest dawn",
                            "quiet forest dawn",
                            "misty trees at sunrise",
                        ],
                    }
                },
                state,
                client_factory=lambda api_key, cache_dir: fake,
            )

        self.assertEqual(result["queries"], ["quiet forest dawn", "misty trees at sunrise"])
        self.assertEqual(
            [candidate["candidate_id"] for candidate in result["candidates"]],
            ["pixabay:video:1253", "pixabay:image:195893"],
        )
        self.assertEqual(fake.video_queries[0][1]["orientation"], "horizontal")
        self.assertEqual(result["cache_hits"], 2)
        encoded = json.dumps(result)
        self.assertNotIn(".mp4", encoded)
        self.assertNotIn("image-195893_1280.jpg", encoded)

    def test_long_provider_query_is_trimmed_to_pixabay_limit(self):
        query = "word " * 40
        normalized = plugin.normalize_provider_query(query)
        self.assertLessEqual(len(normalized), 100)
        self.assertFalse(normalized.endswith(" "))

    def test_standard_video_prefers_medium_and_high_prefers_large(self):
        standard = plugin.preferred_video_rendition(VIDEO_FIXTURE, "standard")
        high = plugin.preferred_video_rendition(VIDEO_FIXTURE, "high")

        self.assertEqual(standard["name"], "medium")
        self.assertEqual(high["name"], "large")

    def test_fetch_selected_video_downloads_only_chosen_asset_into_workspace(self):
        with tempfile.TemporaryDirectory() as root:
            output = pathlib.Path(root) / "output"
            temp = pathlib.Path(root) / "temp"
            cache = pathlib.Path(root) / "cache"
            output.mkdir()
            temp.mkdir()
            cache.mkdir()

            state = plugin.PluginState()
            state.initialize(
                {
                    "job_workspace": {"output": str(output), "temp": str(temp)},
                    "provider_cache": str(cache),
                }
            )
            fake = FakeClient("machine-secret")

            with patch.dict(os.environ, {"PIXABAY_API_KEY": "machine-secret"}, clear=False):
                result = plugin.execute_fetch_selected(
                    {
                        "selection_ref": "pixabay:video:1253",
                        "quality_mode": "standard",
                    },
                    state,
                    client_factory=lambda api_key, cache_dir: fake,
                )

            selected = output / result["relative_output"]
            self.assertTrue(selected.is_file())
            self.assertEqual((result["width"], result["height"]), (1920, 1080))
            self.assertEqual(result["provenance"]["creator_name"], "VideoCreator")
            self.assertEqual(result["provenance"]["license"], "Pixabay Content License")
            self.assertIn("Video by VideoCreator on Pixabay", result["provenance"]["attribution"])
            self.assertEqual(len(fake.downloads), 1)
            self.assertIn("medium-1253.mp4", fake.downloads[0][0])
            self.assertNotIn("medium-1253.mp4", json.dumps(result))

    def test_fetch_selected_image_downloads_to_workspace_and_preserves_attribution(self):
        with tempfile.TemporaryDirectory() as root:
            output = pathlib.Path(root) / "output"
            temp = pathlib.Path(root) / "temp"
            output.mkdir()
            temp.mkdir()

            state = plugin.PluginState()
            state.initialize({"job_workspace": {"output": str(output), "temp": str(temp)}})
            fake = FakeClient("machine-secret")

            with patch.dict(os.environ, {"PIXABAY_API_KEY": "machine-secret"}, clear=False):
                result = plugin.execute_fetch_selected(
                    {
                        "selection_ref": "pixabay:image:195893",
                        "quality_mode": "high",
                    },
                    state,
                    client_factory=lambda api_key, cache_dir: fake,
                )

            selected = output / result["relative_output"]
            self.assertTrue(selected.is_file())
            self.assertEqual((result["width"], result["height"]), (4000, 2250))
            self.assertEqual(result["provenance"]["creator_name"], "PhotoCreator")
            self.assertEqual(result["provenance"]["quality_mode"], "original")
            self.assertIn("Image by PhotoCreator on Pixabay", result["provenance"]["attribution"])
            self.assertEqual(fake.downloads[0][0], "https://pixabay.com/get/image-195893.jpg")
            self.assertNotIn("https://pixabay.com/get/image-195893.jpg", json.dumps(result))

    def test_health_reports_credential_and_cache_readiness(self):
        with tempfile.TemporaryDirectory() as root:
            state = plugin.PluginState()
            state.initialize({"provider_cache": root})
            with patch.dict(os.environ, {}, clear=True):
                result = plugin.health_result(state)

        self.assertEqual(result["status"], "needs_attention")
        self.assertFalse(result["credential_present"])
        self.assertTrue(result["provider_cache_ready"])
        self.assertEqual(result["credential_env"], "PIXABAY_API_KEY")

    def test_secret_value_in_portable_settings_is_rejected(self):
        state = plugin.PluginState()
        with self.assertRaises(plugin.PluginFailure) as raised:
            state.initialize({"settings": {"api_key": "must-not-be-stored"}})
        self.assertEqual(raised.exception.code, "SECRET_SETTING_REJECTED")

    def test_rate_limit_error_uses_pixabay_reset_seconds(self):
        error = HTTPError(
            "https://pixabay.com/api/",
            429,
            "Too Many Requests",
            {"X-RateLimit-Reset": "19"},
            None,
        )
        mapped = plugin.map_http_error(error)
        self.assertEqual(mapped.code, "PIXABAY_RATE_LIMITED")
        self.assertTrue(mapped.retryable)
        self.assertEqual(mapped.retry_after_seconds, 19)

    def test_protocol_execute_returns_plugin_api_v1_response(self):
        state = plugin.PluginState()
        fake = FakeClient("machine-secret")
        request = {
            "api_version": 1,
            "request_id": "req_123",
            "method": "plugin.execute",
            "params": {
                "operation": "visual.resolve",
                "payload": {
                    "scene": {
                        "id": "SC17",
                        "aspect_ratio": "9:16",
                        "search_queries": ["quiet forest dawn"],
                    },
                    "settings": {"media_type": "video"},
                },
            },
        }

        with patch.dict(os.environ, {"PIXABAY_API_KEY": "machine-secret"}, clear=False):
            response, shutdown = plugin.handle_request(
                request,
                state,
                client_factory=lambda api_key, cache_dir: fake,
            )

        self.assertFalse(shutdown)
        self.assertEqual(response["api_version"], 1)
        self.assertEqual(response["request_id"], "req_123")
        self.assertTrue(response["result"]["preview_only"])
        self.assertEqual(fake.video_queries[0][1]["orientation"], "vertical")

    def test_fetch_selected_requires_workspace_and_selection_ref_is_provider_bound(self):
        state = plugin.PluginState()
        fake = FakeClient("machine-secret")

        with self.assertRaises(plugin.PluginFailure) as raised:
            plugin.parse_selection_ref("pexels:video:1253")
        self.assertEqual(raised.exception.code, "INVALID_SELECTION_REF")

        with patch.dict(os.environ, {"PIXABAY_API_KEY": "machine-secret"}, clear=False):
            with self.assertRaises(plugin.PluginFailure) as raised:
                plugin.execute_fetch_selected(
                    {"selection_ref": "pixabay:video:1253"},
                    state,
                    client_factory=lambda api_key, cache_dir: fake,
                )
        self.assertEqual(raised.exception.code, "WORKSPACE_REQUIRED")


if __name__ == "__main__":
    unittest.main()
