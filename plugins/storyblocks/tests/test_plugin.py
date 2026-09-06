import importlib.util
import json
import os
import pathlib
import sys
import tempfile
import unittest
from unittest.mock import patch
from urllib.parse import parse_qs, urlparse

PLUGIN_PATH = pathlib.Path(__file__).resolve().parents[1] / "plugin.py"
SPEC = importlib.util.spec_from_file_location("storyblocks_plugin", PLUGIN_PATH)
plugin = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = plugin
SPEC.loader.exec_module(plugin)


VIDEO = {
    "id": 11851,
    "title": "Quiet Forest Dawn",
    "type": "footage",
    "contentClass": "video",
    "thumbnail_url": "https://d2v9y0dukr6mq2.cloudfront.net/video/thumbnail/forest.jpg",
    "preview_urls": {
        "_360p": "https://d2v9y0dukr6mq2.cloudfront.net/video/preview/forest-360.mp4",
        "_720p": "https://d2v9y0dukr6mq2.cloudfront.net/video/preview/forest-720.mp4",
    },
    "download_formats": {
        "MP4": {
            "_720p": {
                "format_name": "HDMP4",
                "file_size_bytes": 12000000,
                "height": 720,
                "width": 1280,
                "frame_rate": 29.97,
            },
            "_1080p": {
                "format_name": "HDMP4",
                "file_size_bytes": 30000000,
                "height": 1080,
                "width": 1920,
                "frame_rate": 29.97,
            },
            "_2160p": {
                "format_name": "4KMP4",
                "file_size_bytes": 90000000,
                "height": 2160,
                "width": 3840,
                "frame_rate": 29.97,
            },
        }
    },
    "keywords": ["forest", "dawn", "mist"],
    "description": "Quiet mist moving through forest trees",
    "duration": 12,
    "durationMs": 12000,
    "orientation": "horizontal",
    "content_id": 71090305,
    "asset_id": "SBV-71090305",
    "is_editorial": False,
    "has_talent_released": True,
    "has_property_released": True,
    "contributor": {"username": "Storyblocks Video"},
}

IMAGE = {
    "id": 800953,
    "title": "Deep Forest Background",
    "type": "photo",
    "contentClass": "image",
    "thumbnail_url": "https://d1yn1kh78jj1rr.cloudfront.net/image/thumbnail/forest.jpg",
    "preview_url": "https://d1yn1kh78jj1rr.cloudfront.net/image/preview/forest.jpg",
    "download_formats": {
        "JPG": {
            "format_name": "JPG",
            "file_size_bytes": 1385263,
            "height": 2239,
            "width": 3000,
        }
    },
    "keywords": ["forest", "nature", "background"],
    "description": "Deep green forest background",
    "content_id": 3000949,
    "asset_id": "SBI-3000949",
    "aspect_ratio": 1.34,
    "is_editorial": False,
    "has_talent_released": False,
    "has_property_released": True,
    "contributor": {"username": "Storyblocks Images"},
}

VIDEO_LINKS = {
    "MOV": {
        "_1080p": "https://d2v9y0dukr6mq2.cloudfront.net/video/download/forest-1080.mov"
    },
    "MP4": {
        "_720p": "https://d2v9y0dukr6mq2.cloudfront.net/video/download/forest-720.mp4",
        "_1080p": "https://d2v9y0dukr6mq2.cloudfront.net/video/download/forest-1080.mp4",
        "_2160p": "https://d2v9y0dukr6mq2.cloudfront.net/video/download/forest-2160.mp4",
    },
}

IMAGE_LINKS = {
    "JPG": "https://d1yn1kh78jj1rr.cloudfront.net/image/download/forest.jpg",
    "PSD": "https://d1yn1kh78jj1rr.cloudfront.net/image/download/forest.psd",
}


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


class FakeClient:
    def __init__(self, public_key, private_key, user_id):
        self.public_key = public_key
        self.private_key = private_key
        self.user_id = user_id
        self.video_searches = []
        self.image_searches = []
        self.events = []
        self.downloads = []

    def search_videos(self, project_id, keywords, **kwargs):
        self.video_searches.append((project_id, keywords, kwargs))
        return plugin.StoryblocksResponse(
            data={"total_results": 1, "results": [VIDEO]},
            quota={"limit": 100, "remaining": 99, "reset": 42},
        )

    def search_images(self, project_id, keywords, **kwargs):
        self.image_searches.append((project_id, keywords, kwargs))
        return plugin.StoryblocksResponse(
            data={"total_results": 1, "results": [IMAGE]},
            quota={"limit": 100, "remaining": 98, "reset": 42},
        )

    def get_video_details(self, asset_id):
        self.events.append(("video_details", asset_id))
        return plugin.StoryblocksResponse(
            data=VIDEO,
            quota={"limit": 100, "remaining": 97, "reset": 42},
        )

    def get_image_details(self, asset_id):
        self.events.append(("image_details", asset_id))
        return plugin.StoryblocksResponse(
            data=IMAGE,
            quota={"limit": 100, "remaining": 97, "reset": 42},
        )

    def get_video_download_links(self, project_id, asset_id):
        self.events.append(("video_links", project_id, asset_id))
        return plugin.StoryblocksResponse(
            data=VIDEO_LINKS,
            quota={"limit": 100, "remaining": 96, "reset": 42},
        )

    def get_image_download_links(self, project_id, asset_id):
        self.events.append(("image_links", project_id, asset_id))
        return plugin.StoryblocksResponse(
            data=IMAGE_LINKS,
            quota={"limit": 100, "remaining": 96, "reset": 42},
        )

    def download_to_path(self, url, destination):
        self.events.append(("download", url))
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(b"storyblocks selected fixture")
        self.downloads.append((url, destination))


class StoryblocksPluginTests(unittest.TestCase):
    def machine_env(self, mode="test"):
        return {
            "STORYBLOCKS_PUBLIC_KEY": "public-key",
            "STORYBLOCKS_PRIVATE_KEY": "private-secret",
            "STORYBLOCKS_USER_ID": "creator-local-01",
            "STORYBLOCKS_API_MODE": mode,
        }

    def test_hmac_signing_matches_storyblocks_resource_contract(self):
        client = plugin.StoryblocksClient(
            "public-key",
            "private-secret",
            "creator-local-01",
            clock=lambda: 1700000000.0,
        )

        url = client.signed_request_url(
            "/api/v2/videos/search",
            {
                "project_id": "PRJ_123",
                "user_id": "creator-local-01",
                "keywords": "quiet forest",
            },
        )
        parsed = urlparse(url)
        params = parse_qs(parsed.query)

        self.assertEqual(parsed.scheme, "https")
        self.assertEqual(parsed.netloc, "api.storyblocks.com")
        self.assertEqual(params["APIKEY"], ["public-key"])
        self.assertEqual(params["EXPIRES"], ["1700000100"])
        self.assertEqual(
            params["HMAC"],
            ["354404fbfdd27950f91099467d50edad23a7eb78d9d25710dda0d3fc64df6c04"],
        )
        self.assertEqual(params["project_id"], ["PRJ_123"])
        self.assertNotIn("private-secret", url)

    def test_client_search_uses_project_user_and_documented_filters(self):
        opener = RecordingOpener({"total_results": 0, "results": []})
        client = plugin.StoryblocksClient(
            "public-key",
            "private-secret",
            "creator-local-01",
            opener=opener,
            clock=lambda: 1700000000.0,
        )

        response = client.search_images(
            "PRJ_123",
            "quiet forest",
            orientation="horizontal",
            safe_search=True,
            sort_by="most_relevant",
            results_per_page=8,
        )

        request, timeout = opener.requests[0]
        self.assertEqual(timeout, plugin.REQUEST_TIMEOUT_SECONDS)
        self.assertIn("/api/v2/images/search?", request.full_url)
        self.assertIn("project_id=PRJ_123", request.full_url)
        self.assertIn("user_id=creator-local-01", request.full_url)
        self.assertIn("keywords=quiet+forest", request.full_url)
        self.assertIn("orientation=landscape", request.full_url)
        self.assertIn("safe_search=true", request.full_url)
        self.assertIn("is_editorial=false", request.full_url)
        self.assertEqual(response.quota["remaining"], 99)

    def test_video_and_image_normalization_remain_preview_only(self):
        video = plugin.normalize_video("SC17", VIDEO)
        image = plugin.normalize_image("SC17", IMAGE)

        self.assertEqual(video["candidate_id"], "storyblocks:video:11851")
        self.assertEqual(image["candidate_id"], "storyblocks:image:800953")
        self.assertEqual(video["creator_name"], "Storyblocks Video")
        self.assertEqual(image["creator_name"], "Storyblocks Images")
        self.assertEqual((video["width"], video["height"]), (1920, 1080))
        self.assertEqual((image["width"], image["height"]), (3000, 2239))

        encoded = json.dumps({"video": video, "image": image})
        self.assertIn("forest-360.mp4", encoded)
        self.assertIn("image/preview/forest.jpg", encoded)
        self.assertNotIn("video/download/forest-1080.mp4", encoded)
        self.assertNotIn("image/download/forest.jpg", encoded)

    def test_visual_resolve_uses_canonical_project_id_and_machine_user_id(self):
        state = plugin.PluginState()
        state.initialize({"settings": {"media_type": "both", "per_query": 4}})
        fake = FakeClient("public-key", "private-secret", "creator-local-01")

        with patch.dict(os.environ, self.machine_env("test"), clear=False):
            result = plugin.execute_visual_resolve(
                {
                    "project_id": "PRJ_123",
                    "scene": {
                        "id": "SC17",
                        "aspect_ratio": "16:9",
                        "search_queries": [
                            "quiet forest",
                            "quiet forest",
                            "misty trees",
                        ],
                    },
                },
                state,
                client_factory=lambda public, private, user: fake,
            )

        self.assertEqual(result["project_id"], "PRJ_123")
        self.assertEqual(result["api_mode"], "test")
        self.assertFalse(result["production_download_ready"])
        self.assertEqual(
            [candidate["candidate_id"] for candidate in result["candidates"]],
            ["storyblocks:video:11851", "storyblocks:image:800953"],
        )
        self.assertEqual(fake.video_searches[0][0], "PRJ_123")
        self.assertEqual(fake.image_searches[0][0], "PRJ_123")
        self.assertEqual(fake.video_searches[0][2]["orientation"], "horizontal")
        self.assertEqual(fake.image_searches[0][2]["orientation"], "horizontal")

    def test_square_scene_maps_image_square_and_video_all(self):
        state = plugin.PluginState()
        fake = FakeClient("public-key", "private-secret", "creator-local-01")
        with patch.dict(os.environ, self.machine_env("test"), clear=False):
            plugin.execute_visual_resolve(
                {
                    "project_id": "PRJ_123",
                    "scene": {
                        "id": "SC17",
                        "aspect_ratio": "1:1",
                        "search_queries": ["forest"],
                    },
                    "media_type": "both",
                },
                state,
                client_factory=lambda public, private, user: fake,
            )

        self.assertEqual(fake.video_searches[0][2]["orientation"], "square")
        self.assertEqual(fake.image_searches[0][2]["orientation"], "square")
        self.assertEqual(plugin.video_orientation("square"), "all")
        self.assertEqual(plugin.image_orientation("square"), "square")

    def test_test_mode_blocks_promotable_selected_download_before_provider_calls(self):
        with tempfile.TemporaryDirectory() as root:
            output = pathlib.Path(root) / "output"
            temp = pathlib.Path(root) / "temp"
            output.mkdir()
            temp.mkdir()

            state = plugin.PluginState()
            state.initialize({"job_workspace": {"output": str(output), "temp": str(temp)}})
            fake = FakeClient("public-key", "private-secret", "creator-local-01")

            with patch.dict(os.environ, self.machine_env("test"), clear=False):
                with self.assertRaises(plugin.PluginFailure) as raised:
                    plugin.execute_fetch_selected(
                        {
                            "project_id": "PRJ_123",
                            "selection_ref": "storyblocks:video:11851",
                        },
                        state,
                        client_factory=lambda public, private, user: fake,
                    )

            self.assertEqual(
                raised.exception.code,
                "STORYBLOCKS_PRODUCTION_ACCESS_REQUIRED",
            )
            self.assertEqual(fake.events, [])
            self.assertEqual(list(output.rglob("*")), [])

    def test_production_video_download_prefers_1080p_standard_and_never_persists_url(self):
        with tempfile.TemporaryDirectory() as root:
            output = pathlib.Path(root) / "output"
            temp = pathlib.Path(root) / "temp"
            output.mkdir()
            temp.mkdir()

            state = plugin.PluginState()
            state.initialize({"job_workspace": {"output": str(output), "temp": str(temp)}})
            fake = FakeClient("public-key", "private-secret", "creator-local-01")

            with patch.dict(os.environ, self.machine_env("production"), clear=False):
                result = plugin.execute_fetch_selected(
                    {
                        "project_id": "PRJ_123",
                        "selection_ref": "storyblocks:video:11851",
                        "quality_mode": "standard",
                    },
                    state,
                    client_factory=lambda public, private, user: fake,
                )

            self.assertEqual(
                [event[0] for event in fake.events],
                ["video_details", "video_links", "download"],
            )
            self.assertIn("forest-1080.mp4", fake.events[2][1])
            self.assertEqual((result["width"], result["height"]), (1920, 1080))
            self.assertEqual(result["duration"], 12.0)
            self.assertTrue((output / result["relative_output"]).is_file())
            self.assertEqual(
                result["provenance"]["provider_asset_code"],
                "SBV-71090305",
            )
            self.assertEqual(
                result["provenance"]["license_mode"],
                "production_api",
            )
            self.assertEqual(
                result["provenance"]["licensed_project_id"],
                "PRJ_123",
            )
            encoded = json.dumps(result)
            self.assertNotIn("cloudfront.net/video/download", encoded)
            self.assertNotIn("creator-local-01", encoded)
            self.assertNotIn("private-secret", encoded)

    def test_production_high_quality_video_prefers_largest_mp4(self):
        selected = plugin.select_video_download(VIDEO, VIDEO_LINKS, "high")
        self.assertEqual(selected["rendition"], "_2160p")
        self.assertEqual((selected["width"], selected["height"]), (3840, 2160))

    def test_production_image_download_uses_jpg_and_preserves_license_facts(self):
        with tempfile.TemporaryDirectory() as root:
            output = pathlib.Path(root) / "output"
            temp = pathlib.Path(root) / "temp"
            output.mkdir()
            temp.mkdir()

            state = plugin.PluginState()
            state.initialize({"job_workspace": {"output": str(output), "temp": str(temp)}})
            fake = FakeClient("public-key", "private-secret", "creator-local-01")

            with patch.dict(os.environ, self.machine_env("production"), clear=False):
                result = plugin.execute_fetch_selected(
                    {
                        "project_id": "PRJ_123",
                        "selection_ref": "storyblocks:image:800953",
                    },
                    state,
                    client_factory=lambda public, private, user: fake,
                )

            self.assertEqual(
                [event[0] for event in fake.events],
                ["image_details", "image_links", "download"],
            )
            self.assertIn("image/download/forest.jpg", fake.events[2][1])
            self.assertEqual((result["width"], result["height"]), (3000, 2239))
            self.assertEqual(result["provenance"]["download_format"], "JPG")
            self.assertEqual(
                result["provenance"]["creator_name"],
                "Storyblocks Images",
            )
            self.assertNotIn("image/download/forest.jpg", json.dumps(result))

    def test_health_exposes_readiness_without_secret_values(self):
        state = plugin.PluginState()
        with patch.dict(os.environ, self.machine_env("production"), clear=True):
            result = plugin.health_result(state)

        self.assertEqual(result["status"], "ready")
        self.assertTrue(result["credential_ready"])
        self.assertTrue(result["user_id_ready"])
        self.assertTrue(result["production_download_ready"])
        encoded = json.dumps(result)
        self.assertNotIn("private-secret", encoded)
        self.assertNotIn("creator-local-01", encoded)

    def test_secret_and_machine_local_values_are_rejected_from_portable_settings(self):
        state = plugin.PluginState()
        for key in ("public_key", "private_key", "user_id", "api_mode"):
            with self.subTest(key=key):
                with self.assertRaises(plugin.PluginFailure) as raised:
                    state.initialize({"settings": {key: "must-not-persist"}})
                self.assertEqual(raised.exception.code, "SECRET_SETTING_REJECTED")

    def test_project_id_must_be_portable_not_path(self):
        self.assertEqual(plugin.validate_project_id("PRJ_123"), "PRJ_123")
        for value in ("/Users/me/project", "C:\\project", "../project"):
            with self.subTest(value=value):
                with self.assertRaises(plugin.PluginFailure):
                    plugin.validate_project_id(value)

    def test_media_host_is_restricted_to_documented_storyblocks_cdns(self):
        self.assertEqual(
            plugin.validate_media_url(
                "https://d2v9y0dukr6mq2.cloudfront.net/video/download/a.mp4"
            ),
            "https://d2v9y0dukr6mq2.cloudfront.net/video/download/a.mp4",
        )
        with self.assertRaises(plugin.PluginFailure):
            plugin.validate_media_url("https://evil.example/a.mp4")

    def test_selection_ref_is_provider_bound(self):
        self.assertEqual(
            plugin.parse_selection_ref("storyblocks:video:11851"),
            ("video", "11851"),
        )
        with self.assertRaises(plugin.PluginFailure):
            plugin.parse_selection_ref("shutterstock:video:11851")

    def test_protocol_resolve_returns_plugin_api_v1_response(self):
        state = plugin.PluginState()
        fake = FakeClient("public-key", "private-secret", "creator-local-01")
        request = {
            "api_version": 1,
            "request_id": "req_storyblocks",
            "method": "plugin.execute",
            "params": {
                "operation": "visual.resolve",
                "payload": {
                    "project_id": "PRJ_123",
                    "scene": {
                        "id": "SC17",
                        "aspect_ratio": "9:16",
                        "search_queries": ["forest"],
                    },
                    "media_type": "video",
                },
            },
        }

        with patch.dict(os.environ, self.machine_env("test"), clear=False):
            response, shutdown = plugin.handle_request(
                request,
                state,
                client_factory=lambda public, private, user: fake,
            )

        self.assertFalse(shutdown)
        self.assertEqual(response["api_version"], 1)
        self.assertEqual(response["request_id"], "req_storyblocks")
        self.assertTrue(response["result"]["preview_only"])
        self.assertEqual(fake.video_searches[0][2]["orientation"], "vertical")


if __name__ == "__main__":
    unittest.main()
