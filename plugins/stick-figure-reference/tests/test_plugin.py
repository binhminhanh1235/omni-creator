import importlib.util
import json
import pathlib
import sys
import tempfile
import unittest

PLUGIN_PATH = pathlib.Path(__file__).resolve().parents[1] / "plugin.py"
SPEC = importlib.util.spec_from_file_location("stick_figure_reference_plugin", PLUGIN_PATH)
plugin = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = plugin
SPEC.loader.exec_module(plugin)


def payload(seed=42, preset="christian-stick-explainer"):
    prompt = (
        "Purpose: Show two friends rebuilding trust in a relationship.\n"
        "Scene type: conceptual\n"
        "Narration: Forgiveness can be offered while trust is carefully rebuilt.\n"
        "Visual ideas: repairing a bridge, rebuilding a fence\n"
        "Style preset: christian-stick-explainer\n"
        "Aspect ratio: 16:9"
    )
    return {
        "schema": "omnicreator.generated-image-request",
        "version": 1,
        "scene": {
            "schema": "omnicreator.scene-intent",
            "schema_version": 1,
            "id": "SC17",
            "segment_id": "S17",
            "narration": "Forgiveness can be offered while trust is carefully rebuilt.",
            "purpose": "Show two friends rebuilding trust in a relationship.",
            "scene_type": "conceptual",
            "emotion_before": "guarded",
            "emotion_after": "cautious hope",
            "duration_hint": 8.0,
            "visual_ideas": ["repairing a bridge", "rebuilding a fence"],
            "search_queries": ["two friends rebuilding trust"],
            "avoid": ["logos"],
            "continuity": {},
            "aspect_ratio": "16:9",
        },
        "prompt": prompt,
        "negative_prompt": "logos",
        "style": {"preset": preset, "description": None},
        "resolution": {"width": 1280, "height": 720},
        "aspect_ratio": "16:9",
        "seed": seed,
        "settings": {},
        "prompt_sha256": "a" * 64,
        "settings_fingerprint": "b" * 64,
    }


class StickFigureReferenceTests(unittest.TestCase):
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

    def test_scene_intent_maps_to_characters_actions_and_objects(self):
        request = plugin.validate_request(payload())
        plan = plugin.map_scene_intent(request["scene"], request["preset"])

        self.assertGreaterEqual(len(plan["characters"]), 2)
        self.assertIn("friend", [item["role"] for item in plan["characters"]])
        self.assertIn("offer_grace", plan["actions"])
        self.assertIn("rebuild_trust", plan["actions"])
        self.assertIn("bridge", plan["objects"])
        self.assertIn("fence", plan["objects"])
        self.assertEqual(plan["animation_preset"], "minimal-motion")
        self.assertFalse(plan["thumbnail_style"])

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

    def test_different_seed_changes_artifact(self):
        with tempfile.TemporaryDirectory() as root:
            state, _ = self.initialized_state(root)
            first = plugin.execute_visual_generate(payload(seed=7), state)
            second = plugin.execute_visual_generate(payload(seed=8), state)
            self.assertNotEqual(first["sha256"], second["sha256"])

    def test_minimal_motion_preset_embeds_svg_animation(self):
        with tempfile.TemporaryDirectory() as root:
            state, output = self.initialized_state(root)
            result = plugin.execute_visual_generate(payload(), state)
            svg = (output / result["relative_output"]).read_text()

            self.assertIn("<animateTransform", svg)
            self.assertIn('repeatCount="indefinite"', svg)
            self.assertEqual(result["metadata"]["animation_preset"], "minimal-motion")

    def test_thumbnail_preset_uses_thumbnail_composition_without_motion(self):
        with tempfile.TemporaryDirectory() as root:
            state, output = self.initialized_state(root)
            result = plugin.execute_visual_generate(
                payload(preset="stick-figure-thumbnail"),
                state,
            )
            svg = (output / result["relative_output"]).read_text()

            self.assertNotIn("<animateTransform", svg)
            self.assertTrue(result["metadata"]["thumbnail_style"])
            self.assertEqual(
                result["metadata"]["semantic_plan"]["layout"],
                "thumbnail-focus",
            )
            self.assertEqual(result["metadata"]["animation_preset"], "none")

    def test_generation_requires_scene_intent_v1(self):
        invalid = payload()
        invalid["scene"]["schema_version"] = 2
        state = plugin.PluginState()

        with self.assertRaises(plugin.PluginFailure) as raised:
            plugin.validate_request(invalid)
        self.assertEqual(raised.exception.code, "INVALID_INPUT")

    def test_unsupported_preset_is_rejected(self):
        with self.assertRaises(plugin.PluginFailure) as raised:
            plugin.validate_request(payload(preset="whiteboard-v2"))
        self.assertEqual(raised.exception.code, "UNSUPPORTED_PRESET")

    def test_output_path_escape_is_rejected(self):
        with tempfile.TemporaryDirectory() as root:
            state, _ = self.initialized_state(root)
            with self.assertRaises(plugin.PluginFailure) as raised:
                plugin.resolve_output(state, "../escape.svg")
            self.assertEqual(raised.exception.code, "INVALID_OUTPUT_PATH")

    def test_result_is_portable_and_contains_semantic_plan(self):
        with tempfile.TemporaryDirectory() as root:
            state, output = self.initialized_state(root)
            result = plugin.execute_visual_generate(payload(), state)
            encoded = json.dumps(result)

            self.assertEqual(result["mime_type"], "image/svg+xml")
            self.assertEqual(result["model_id"], "stick-figure-reference-svg")
            self.assertTrue((output / result["relative_output"]).is_file())
            self.assertNotIn(str(output), encoded)
            self.assertIn("semantic_plan", result["metadata"])
            self.assertEqual(result["provenance"]["provider"], "stick-figure-reference")
            self.assertTrue(result["provenance"]["offline"])

    def test_protocol_capabilities_advertise_exact_stick_capability(self):
        state = plugin.PluginState()
        request = {
            "api_version": 1,
            "request_id": "req_caps",
            "method": "plugin.capabilities",
            "params": {},
        }

        response, shutdown = plugin.handle_request(request, state)

        self.assertFalse(shutdown)
        self.assertTrue(response["result"]["stick_figure_visual"])
        self.assertTrue(response["result"]["visual_generate"])
        self.assertTrue(response["result"]["deterministic_seed"])
        self.assertEqual(response["result"]["operations"], ["visual.generate"])
        self.assertNotIn("generated_still", response["result"])

    def test_protocol_visual_generate_returns_v1_result(self):
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
            self.assertTrue((output / result["relative_output"]).is_file())
            self.assertEqual(
                result["metadata"]["renderer"],
                "stick-figure-procedural-svg-v1",
            )


if __name__ == "__main__":
    unittest.main()
