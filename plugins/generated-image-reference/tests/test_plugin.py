import importlib.util
import json
import pathlib
import sys
import tempfile
import unittest

PLUGIN_PATH = pathlib.Path(__file__).resolve().parents[1] / "plugin.py"
SPEC = importlib.util.spec_from_file_location("generated_image_reference_plugin", PLUGIN_PATH)
plugin = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = plugin
SPEC.loader.exec_module(plugin)


def payload(seed=42):
    prompt = (
        "Purpose: Show patient restoration.\n"
        "Scene type: conceptual\n"
        "Narration: A quiet craftsperson repairs a weathered wooden gate.\n"
        "Style preset: cinematic-warm\n"
        "Aspect ratio: 16:9"
    )
    return {
        "schema": "omnicreator.generated-image-request",
        "version": 1,
        "scene": {
            "schema": "omnicreator.scene-intent",
            "schema_version": 1,
            "id": "SC01",
            "segment_id": "S01",
            "narration": "A quiet craftsperson repairs a weathered wooden gate.",
            "purpose": "Show patient restoration.",
            "scene_type": "conceptual",
            "emotion_before": "worn",
            "emotion_after": "hopeful",
            "duration_hint": 5.0,
            "visual_ideas": ["warm dawn light"],
            "search_queries": ["repairing wooden gate"],
            "avoid": ["logos"],
            "continuity": {},
            "aspect_ratio": "16:9",
        },
        "prompt": prompt,
        "negative_prompt": "logos",
        "style": {"preset": "cinematic-warm", "description": None},
        "resolution": {"width": 1280, "height": 720},
        "aspect_ratio": "16:9",
        "seed": seed,
        "settings": {},
        "prompt_sha256": "a" * 64,
        "settings_fingerprint": "b" * 64,
    }


class GeneratedImageReferenceTests(unittest.TestCase):
    def initialized_state(self, root):
        output = pathlib.Path(root) / "output"
        temp = pathlib.Path(root) / "temp"
        input_dir = pathlib.Path(root) / "input"
        output.mkdir()
        temp.mkdir()
        input_dir.mkdir()
        state = plugin.PluginState()
        state.initialize(
            {
                "job_workspace": {
                    "root": str(pathlib.Path(root)),
                    "input": str(input_dir),
                    "output": str(output),
                    "temp": str(temp),
                }
            }
        )
        return state, output

    def test_generation_is_byte_deterministic_for_fixed_request(self):
        with tempfile.TemporaryDirectory() as first_root, tempfile.TemporaryDirectory() as second_root:
            first_state, first_output = self.initialized_state(first_root)
            second_state, second_output = self.initialized_state(second_root)

            first = plugin.execute_visual_generate(payload(), first_state)
            second = plugin.execute_visual_generate(payload(), second_state)

            self.assertEqual(first["sha256"], second["sha256"])
            self.assertEqual(first["relative_output"], second["relative_output"])
            self.assertEqual(
                (first_output / first["relative_output"]).read_bytes(),
                (second_output / second["relative_output"]).read_bytes(),
            )
            self.assertEqual(first["seed"], 42)

    def test_different_seed_changes_deterministic_artifact(self):
        with tempfile.TemporaryDirectory() as root:
            state, _ = self.initialized_state(root)
            first = plugin.execute_visual_generate(payload(seed=7), state)
            second = plugin.execute_visual_generate(payload(seed=8), state)
            self.assertNotEqual(first["sha256"], second["sha256"])

    def test_generation_requires_initialized_workspace(self):
        state = plugin.PluginState()
        with self.assertRaises(plugin.PluginFailure) as raised:
            plugin.execute_visual_generate(payload(), state)
        self.assertEqual(raised.exception.code, "WORKSPACE_REQUIRED")

    def test_protocol_visual_generate_returns_v1_structured_result(self):
        with tempfile.TemporaryDirectory() as root:
            state, output = self.initialized_state(root)
            request = {
                "api_version": 1,
                "request_id": "req_generate",
                "method": "plugin.execute",
                "params": {
                    "operation": "visual.generate",
                    "payload": payload(),
                },
            }
            response, shutdown = plugin.handle_request(request, state)

            self.assertFalse(shutdown)
            self.assertEqual(response["api_version"], 1)
            self.assertEqual(response["request_id"], "req_generate")
            result = response["result"]
            self.assertEqual(result["mime_type"], "image/svg+xml")
            self.assertTrue((output / result["relative_output"]).is_file())
            self.assertEqual(result["model_id"], "reference-svg")
            self.assertEqual(result["model_version"], "1")
            self.assertNotIn(str(output), json.dumps(result))

    def test_capabilities_advertise_visual_generate_and_seed(self):
        capabilities = plugin.capabilities_result()
        self.assertIn("visual.generate", capabilities["operations"])
        self.assertTrue(capabilities["deterministic_seed"])

    def test_invalid_scene_id_is_rejected_before_file_write(self):
        with tempfile.TemporaryDirectory() as root:
            state, output = self.initialized_state(root)
            invalid = payload()
            invalid["scene"]["id"] = "../escape"

            with self.assertRaises(plugin.PluginFailure) as raised:
                plugin.execute_visual_generate(invalid, state)

            self.assertEqual(raised.exception.code, "INVALID_INPUT")
            self.assertEqual(list(output.rglob("*")), [])

    def test_unsupported_operation_is_stable_fatal_error(self):
        state = plugin.PluginState()
        response, _ = plugin.handle_request(
            {
                "api_version": 1,
                "request_id": "req_bad",
                "method": "plugin.execute",
                "params": {"operation": "visual.video.generate", "payload": {}},
            },
            state,
        )

        self.assertEqual(response["error"]["code"], "UNSUPPORTED_OPERATION")
        self.assertFalse(response["error"]["retryable"])
        self.assertIsNone(response["error"]["retry_after_seconds"])
        self.assertIsNone(response["error"]["suggested_fallback"])


if __name__ == "__main__":
    unittest.main()
