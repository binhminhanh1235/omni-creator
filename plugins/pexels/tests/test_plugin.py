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
SPEC = importlib.util.spec_from_file_location("pexels_plugin", PLUGIN_PATH)
plugin = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = plugin
SPEC.loader.exec_module(plugin)


class FakeResponse:
    def __init__(self, data, headers=None):
        self._payload = json.dumps(data).encode("utf-8")
        self.headers = headers or {}

    def read(self):
        return self._payload

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
                "X-Ratelimit-Limit": "20000",
                "X-Ratelimit-Remaining": "19999",
                "X-Ratelimit-Reset": "1999999999",
            },
        )


VIDEO_FIXTURE = {
    "id": 2499611,
    "width": 1920,
    "height": 1080,
    "duration": 8,
    "url": "https://www.pexels.com/video/example-2499611/",
    "image": "https://static-videos.pexels.com/videos/2499611/preview.jpg",
    "user": {
        "id": 1,
        "name": "Video Creator",
        "url": "https://www.pexels.com/@video-creator",
    },
    "video_files": [
        {
            "id": 7,
            "quality": "hd",
            "file_type": "video/mp4",
            "width": 4096,
            "height": 2160,
            "fps": 24.0,
            "link": "https://player.vimeo.com/external/video-4k.mp4",
        },
        {
            "id": 8,
            "quality": "hd",
            "file_type": "video/mp4",
            "width": 1920,
            "height": 1080,
            "fps": 24.0,
            "link": "https://player.vimeo.com/external/video-1080.mp4",
        },
        {
            "id": 9,
            "quality": "hd",
            "file_type": "video/mp4",
            "width": 1280,
            "height": 720,
            "fps": 24.0,
            "link": "https://player.vimeo.com/external/video-720.mp4",
        }
    ],
    "video_pictures": [
        {
            "id": 11,
            "nr": 0,
            "picture": "https://static-videos.pexels.com/videos/2499611/frame-0.jpg",
        }
    ],
}

PHOTO_FIXTURE = {
    "id": 2014422,
    "width": 3024,
    "height": 3024,
    "url": "https://www.pexels.com/photo/example-2014422/",
    "photographer": "Photo Creator",
    "photographer_url": "https://www.pexels.com/@photo-creator",
    "src": {
        "original": "https://images.pexels.com/photos/2014422/original.jpeg",
        "large": "https://preview.example/large.jpg",
        "medium": "https://preview.example/medium.jpg",
        "small": "https://preview.example/small.jpg",
    },
    "alt": "Craftsperson repairing a wooden gate",
}


class FakeClient:
    def __init__(self, api_key):
        self.api_key = api_key
        self.video_queries = []
        self.photo_queries = []
        self.downloads = []

    def search_videos(self, query, **kwargs):
        self.video_queries.append((query, kwargs))
        return plugin.PexelsResponse(
            data={"videos": [VIDEO_FIXTURE]},
            quota={"limit": 20000, "remaining": 19998, "reset": 1999999999},
        )

    def search_photos(self, query, **kwargs):
        self.photo_queries.append((query, kwargs))
        return plugin.PexelsResponse(
            data={"photos": [PHOTO_FIXTURE]},
            quota={"limit": 20000, "remaining": 19997, "reset": 1999999999},
        )

    def get_video(self, asset_id):
        return plugin.PexelsResponse(
            data=VIDEO_FIXTURE,
            quota={"limit": 20000, "remaining": 19996, "reset": 1999999999},
        )

    def get_photo(self, asset_id):
        return plugin.PexelsResponse(
            data=PHOTO_FIXTURE,
            quota={"limit": 20000, "remaining": 19995, "reset": 1999999999},
        )

    def download_to_path(self, url, destination):
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(b"fixture selected media")
        self.downloads.append((url, destination))


class PexelsPluginTests(unittest.TestCase):
    def test_client_uses_v1_video_search_authorization_and_filters(self):
        opener = RecordingOpener({"videos": []})
        client = plugin.PexelsClient("secret-key", opener=opener)

        response = client.search_videos(
            "quiet station dawn",
            orientation="landscape",
            size="medium",
            locale="en-US",
            per_page=8,
        )

        request, timeout = opener.requests[0]
        self.assertEqual(request.get_header("Authorization"), "secret-key")
        self.assertEqual(timeout, plugin.REQUEST_TIMEOUT_SECONDS)
        self.assertIn("/v1/videos/search?", request.full_url)
        self.assertIn("query=quiet+station+dawn", request.full_url)
        self.assertIn("orientation=landscape", request.full_url)
        self.assertIn("size=medium", request.full_url)
        self.assertIn("per_page=8", request.full_url)
        self.assertEqual(response.quota["remaining"], 19999)

    def test_photo_normalization_keeps_preview_and_attribution_not_original(self):
        candidate = plugin.normalize_photo("SC17", PHOTO_FIXTURE)
        encoded = json.dumps(candidate)

        self.assertEqual(candidate["candidate_id"], "pexels:image:2014422")
        self.assertEqual(candidate["creator_name"], "Photo Creator")
        self.assertEqual(
            candidate["previews"][0]["url"],
            "https://preview.example/medium.jpg",
        )
        self.assertNotIn("original", encoded)
        self.assertNotIn("https://images.pexels.com/photos/2014422/original.jpeg", encoded)

    def test_video_normalization_keeps_preview_frames_not_video_files(self):
        candidate = plugin.normalize_video("SC17", VIDEO_FIXTURE)
        encoded = json.dumps(candidate)

        self.assertEqual(candidate["candidate_id"], "pexels:video:2499611")
        self.assertEqual(candidate["creator_name"], "Video Creator")
        self.assertGreaterEqual(len(candidate["previews"]), 2)
        self.assertNotIn("video_files", encoded)
        self.assertNotIn("https://player.vimeo.com/external/video-1080.mp4", encoded)

    def test_visual_resolve_deduplicates_across_queries_and_never_returns_full_urls(self):
        state = plugin.PluginState()
        state.initialize(
            {
                "settings": {
                    "media_type": "both",
                    "per_query": 4,
                    "orientation": "auto",
                }
            }
        )
        fake = FakeClient("machine-secret")

        with patch.dict(os.environ, {"PEXELS_API_KEY": "machine-secret"}, clear=False):
            result = plugin.execute_visual_resolve(
                {
                    "scene": {
                        "id": "SC17",
                        "aspect_ratio": "16:9",
                        "search_queries": [
                            "repairing fence careful hands",
                            "craftsperson restoring gate",
                        ],
                    }
                },
                state,
                client_factory=lambda api_key: fake,
            )

        self.assertTrue(result["preview_only"])
        self.assertEqual(result["scene_id"], "SC17")
        self.assertEqual(len(result["candidates"]), 2)
        self.assertEqual(
            [candidate["candidate_id"] for candidate in result["candidates"]],
            ["pexels:video:2499611", "pexels:image:2014422"],
        )
        encoded = json.dumps(result)
        self.assertNotIn("video_files", encoded)
        self.assertNotIn("https://player.vimeo.com/external/video-", encoded)
        self.assertNotIn("https://images.pexels.com/photos/2014422/original.jpeg", encoded)
        self.assertEqual(
            fake.video_queries[0][1]["orientation"],
            "landscape",
        )

    def test_health_reports_missing_machine_credential_without_crashing(self):
        state = plugin.PluginState()
        with patch.dict(os.environ, {}, clear=True):
            result = plugin.health_result(state)

        self.assertEqual(result["status"], "needs_attention")
        self.assertFalse(result["credential_present"])
        self.assertEqual(result["credential_env"], "PEXELS_API_KEY")

    def test_secret_value_in_project_settings_is_rejected(self):
        state = plugin.PluginState()
        with self.assertRaises(plugin.PluginFailure) as raised:
            state.initialize({"settings": {"api_key": "must-not-be-stored"}})

        self.assertEqual(raised.exception.code, "SECRET_SETTING_REJECTED")

    def test_rate_limit_error_is_retryable(self):
        error = HTTPError(
            "https://api.pexels.com/v1/search",
            429,
            "Too Many Requests",
            {"Retry-After": "12"},
            None,
        )
        mapped = plugin.map_http_error(error)

        self.assertEqual(mapped.code, "PEXELS_RATE_LIMITED")
        self.assertTrue(mapped.retryable)
        self.assertEqual(mapped.retry_after_seconds, 12)

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
                        "aspect_ratio": "1:1",
                        "search_queries": ["repairing fence careful hands"],
                    },
                    "settings": {"media_type": "video"},
                },
            },
        }

        with patch.dict(os.environ, {"PEXELS_API_KEY": "machine-secret"}, clear=False):
            response, shutdown = plugin.handle_request(
                request,
                state,
                client_factory=lambda api_key: fake,
            )

        self.assertFalse(shutdown)
        self.assertEqual(response["api_version"], 1)
        self.assertEqual(response["request_id"], "req_123")
        self.assertTrue(response["result"]["preview_only"])
        self.assertEqual(fake.video_queries[0][1]["orientation"], "square")

    def test_client_get_by_id_uses_documented_pexels_paths(self):
        video_opener = RecordingOpener(VIDEO_FIXTURE)
        video_client = plugin.PexelsClient("secret-key", opener=video_opener)
        video_client.get_video("2499611")
        self.assertTrue(
            video_opener.requests[0][0].full_url.endswith(
                "/v1/videos/videos/2499611"
            )
        )

        photo_opener = RecordingOpener(PHOTO_FIXTURE)
        photo_client = plugin.PexelsClient("secret-key", opener=photo_opener)
        photo_client.get_photo("2014422")
        self.assertTrue(
            photo_opener.requests[0][0].full_url.endswith("/v1/photos/2014422")
        )

    def test_standard_video_source_prefers_1080p_and_high_prefers_largest(self):
        standard = plugin.select_video_file(VIDEO_FIXTURE, "standard")
        high = plugin.select_video_file(VIDEO_FIXTURE, "high")

        self.assertEqual((standard["width"], standard["height"]), (1920, 1080))
        self.assertEqual((high["width"], high["height"]), (4096, 2160))

    def test_fetch_selected_video_downloads_only_chosen_asset_into_workspace(self):
        with tempfile.TemporaryDirectory() as root:
            output = pathlib.Path(root) / "output"
            temp = pathlib.Path(root) / "temp"
            output.mkdir()
            temp.mkdir()

            state = plugin.PluginState()
            state.initialize(
                {
                    "job_workspace": {
                        "output": str(output),
                        "temp": str(temp),
                    },
                    "settings": {"api_key_env": "PEXELS_API_KEY"},
                }
            )
            fake = FakeClient("machine-secret")

            with patch.dict(
                os.environ,
                {"PEXELS_API_KEY": "machine-secret"},
                clear=False,
            ):
                result = plugin.execute_fetch_selected(
                    {
                        "selection_ref": "pexels:video:2499611",
                        "quality_mode": "standard",
                    },
                    state,
                    client_factory=lambda api_key: fake,
                )

            selected = output / result["relative_output"]
            self.assertTrue(selected.is_file())
            self.assertEqual(selected.read_bytes(), b"fixture selected media")
            self.assertEqual((result["width"], result["height"]), (1920, 1080))
            self.assertEqual(result["source_asset_id"], "2499611")
            self.assertEqual(result["provenance"]["creator_name"], "Video Creator")
            self.assertIn("Video by Video Creator on Pexels", result["provenance"]["attribution"])
            self.assertEqual(len(fake.downloads), 1)
            self.assertIn("video-1080.mp4", fake.downloads[0][0])
            encoded = json.dumps(result)
            self.assertNotIn("player.vimeo.com", encoded)

    def test_fetch_selected_photo_uses_original_and_preserves_attribution(self):
        with tempfile.TemporaryDirectory() as root:
            output = pathlib.Path(root) / "output"
            temp = pathlib.Path(root) / "temp"
            output.mkdir()
            temp.mkdir()

            state = plugin.PluginState()
            state.initialize(
                {
                    "job_workspace": {
                        "output": str(output),
                        "temp": str(temp),
                    }
                }
            )
            fake = FakeClient("machine-secret")

            with patch.dict(
                os.environ,
                {"PEXELS_API_KEY": "machine-secret"},
                clear=False,
            ):
                result = plugin.execute_fetch_selected(
                    {"selection_ref": "pexels:image:2014422"},
                    state,
                    client_factory=lambda api_key: fake,
                )

            selected = output / result["relative_output"]
            self.assertTrue(selected.is_file())
            self.assertTrue(result["relative_output"].endswith(".jpeg"))
            self.assertEqual(result["provenance"]["creator_name"], "Photo Creator")
            self.assertEqual(
                fake.downloads[0][0],
                "https://images.pexels.com/photos/2014422/original.jpeg",
            )
            self.assertNotIn(
                "https://images.pexels.com/photos/2014422/original.jpeg",
                json.dumps(result),
            )

    def test_fetch_selected_requires_initialized_workspace(self):
        state = plugin.PluginState()
        fake = FakeClient("machine-secret")

        with patch.dict(
            os.environ,
            {"PEXELS_API_KEY": "machine-secret"},
            clear=False,
        ):
            with self.assertRaises(plugin.PluginFailure) as raised:
                plugin.execute_fetch_selected(
                    {"selection_ref": "pexels:video:2499611"},
                    state,
                    client_factory=lambda api_key: fake,
                )

        self.assertEqual(raised.exception.code, "WORKSPACE_REQUIRED")

    def test_invalid_selection_ref_is_rejected_before_provider_call(self):
        state = plugin.PluginState()
        with self.assertRaises(plugin.PluginFailure) as raised:
            plugin.parse_selection_ref("other:video:2499611")

        self.assertEqual(raised.exception.code, "INVALID_SELECTION_REF")

    def test_unsupported_operation_returns_stable_fatal_error(self):
        state = plugin.PluginState()
        response, _ = plugin.handle_request(
            {
                "api_version": 1,
                "request_id": "req_bad",
                "method": "plugin.execute",
                "params": {"operation": "visual.download", "payload": {}},
            },
            state,
        )

        self.assertEqual(response["error"]["code"], "UNSUPPORTED_OPERATION")
        self.assertFalse(response["error"]["retryable"])


if __name__ == "__main__":
    unittest.main()
