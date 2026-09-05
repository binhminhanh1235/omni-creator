import base64
import importlib.util
import io
import json
import os
import pathlib
import sys
import tempfile
import unittest
from email.message import Message
from urllib.error import HTTPError, URLError

PLUGIN_DIR = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(PLUGIN_DIR))
import image_api_support as support

SPEC = importlib.util.spec_from_file_location(
    "generated_image_api_plugin",
    PLUGIN_DIR / "plugin.py",
)
plugin = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = plugin
assert SPEC.loader is not None
SPEC.loader.exec_module(plugin)

SECRET_SENTINEL = "OMNICREATOR_P2B_SECRET_SENTINEL"
SECRET_ENV = "OMNICREATOR_P2B_TEST_API_KEY"
PNG_1X1_B64 = (
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="
)
PNG_1X1 = base64.b64decode(PNG_1X1_B64)


def payload(width=1, height=1):
    return {
        "schema": "omnicreator.generated-image-request",
        "version": 1,
        "scene": {"id": "SC-P2B"},
        "prompt": "A warm studio still",
        "negative_prompt": "logos",
        "resolution": {"width": width, "height": height},
        "seed": 7,
        "prompt_sha256": "a" * 64,
        "settings_fingerprint": "b" * 64,
    }


class FakeResponse:
    def __init__(self, body, headers=None):
        self._body = body
        self.headers = headers or Message()

    def read(self):
        return self._body

    def __enter__(self):
        return self

    def __exit__(self, *_):
        return False


def ok_body(**extra):
    body = {
        "data": [{"b64_json": PNG_1X1_B64}],
        "request_id": "provider-request-1",
    }
    body.update(extra)
    return json.dumps(body).encode()


class GeneratedImageApiTests(unittest.TestCase):
    def tearDown(self):
        os.environ.pop(SECRET_ENV, None)

    def client(self, opener):
        return support.ApiImageClient(
            SECRET_SENTINEL,
            "https://api.example.invalid/v1/images/generations",
            3,
            opener=opener,
        )

    def test_success_http_request_keeps_secret_only_in_authorization_header(self):
        seen = {}

        def opener(request, timeout):
            seen["authorization"] = request.get_header("Authorization")
            seen["body"] = json.loads(request.data.decode())
            seen["timeout"] = timeout
            return FakeResponse(ok_body())

        result = self.client(opener).generate(
            prompt="hello",
            negative_prompt=None,
            width=1,
            height=1,
            seed=4,
            model_id="fixture-image",
            model_version="fixture-v1",
        )
        self.assertEqual(seen["authorization"], f"Bearer {SECRET_SENTINEL}")
        self.assertNotIn(SECRET_SENTINEL, json.dumps(seen["body"]))
        self.assertEqual(seen["timeout"], 3)
        self.assertEqual(result.mime_type, "image/png")
        self.assertEqual((result.width, result.height), (1, 1))

    def http_error(self, code, retry_after=None):
        headers = Message()
        if retry_after is not None:
            headers["Retry-After"] = str(retry_after)
        return HTTPError(
            "https://api.example.invalid",
            code,
            "failure",
            headers,
            io.BytesIO(b"{}"),
        )

    def generate_with_error(self, error):
        def opener(_request, timeout):
            del timeout
            raise error

        return self.client(opener).generate(
            prompt="hello",
            negative_prompt=None,
            width=1,
            height=1,
            seed=None,
            model_id="fixture-image",
            model_version="fixture-v1",
        )

    def test_authentication_failure_is_non_retryable(self):
        with self.assertRaises(support.PluginFailure) as raised:
            self.generate_with_error(self.http_error(401))
        self.assertEqual(raised.exception.code, "AUTHENTICATION_FAILED")
        self.assertFalse(raised.exception.retryable)

    def test_rate_limit_is_retryable(self):
        with self.assertRaises(support.PluginFailure) as raised:
            self.generate_with_error(self.http_error(429, 9))
        self.assertEqual(raised.exception.code, "RATE_LIMITED")
        self.assertTrue(raised.exception.retryable)
        self.assertEqual(raised.exception.retry_after_seconds, 9)

    def test_server_error_is_retryable(self):
        with self.assertRaises(support.PluginFailure) as raised:
            self.generate_with_error(self.http_error(503))
        self.assertEqual(raised.exception.code, "PROVIDER_SERVER_ERROR")
        self.assertTrue(raised.exception.retryable)

    def test_network_error_is_retryable(self):
        with self.assertRaises(support.PluginFailure) as raised:
            self.generate_with_error(URLError("timeout"))
        self.assertEqual(raised.exception.code, "NETWORK_ERROR")
        self.assertTrue(raised.exception.retryable)

    def test_malformed_and_missing_image_responses_are_rejected(self):
        for body, code in [
            (b"{not-json", "MALFORMED_PROVIDER_RESPONSE"),
            (json.dumps({"data": []}).encode(), "IMAGE_MISSING"),
            (json.dumps({"data": [{}]}).encode(), "IMAGE_MISSING"),
        ]:
            with self.subTest(code=code, body=body):
                with self.assertRaises(support.PluginFailure) as raised:
                    self.client(lambda *_args, **_kwargs: FakeResponse(body)).generate(
                        prompt="hello",
                        negative_prompt=None,
                        width=1,
                        height=1,
                        seed=None,
                        model_id="fixture-image",
                        model_version="fixture-v1",
                    )
                self.assertEqual(raised.exception.code, code)

    def test_invalid_image_payload_and_dimensions_are_rejected(self):
        bad = base64.b64encode(b"not-an-image").decode()
        for body in [
            json.dumps({"data": [{"b64_json": bad}]}).encode(),
            ok_body(),
        ]:
            width = 1 if b"not-an-image" in base64.b64decode(
                json.loads(body)["data"][0]["b64_json"]
            ) else 2
            with self.assertRaises(support.PluginFailure) as raised:
                self.client(lambda *_args, body=body, **_kwargs: FakeResponse(body)).generate(
                    prompt="hello",
                    negative_prompt=None,
                    width=width,
                    height=1,
                    seed=None,
                    model_id="fixture-image",
                    model_version="fixture-v1",
                )
            self.assertEqual(raised.exception.code, "INVALID_IMAGE_PAYLOAD")

    def initialized_state(self, root):
        output = pathlib.Path(root) / "output"
        output.mkdir()
        state = plugin.PluginState()
        state.initialize(
            {
                "job_workspace": {"output": str(output)},
                "settings": {
                    "api_endpoint": "https://api.example.invalid/v1/images/generations",
                    "api_key_env": SECRET_ENV,
                    "timeout_seconds": 3,
                    "model": "fixture-image",
                    "model_version": "fixture-v1",
                },
            }
        )
        return state, output

    def test_missing_credential_fails_before_client_construction(self):
        with tempfile.TemporaryDirectory() as root:
            state, output = self.initialized_state(root)
            called = False

            def factory(*_args):
                nonlocal called
                called = True
                raise AssertionError("client must not be created")

            with self.assertRaises(plugin.PluginFailure) as raised:
                plugin.execute_visual_generate(payload(), state, factory)
            self.assertEqual(raised.exception.code, "CREDENTIAL_MISSING")
            self.assertFalse(called)
            self.assertEqual(list(output.rglob("*")), [])

    def test_invalid_configuration_and_direct_secret_values_are_rejected(self):
        with self.assertRaises(plugin.PluginFailure) as raised:
            plugin.merge_settings(
                plugin.DEFAULT_SETTINGS,
                {"api_key": SECRET_SENTINEL},
            )
        self.assertEqual(raised.exception.code, "SECRET_SETTING_REJECTED")

        with self.assertRaises(plugin.PluginFailure) as raised:
            plugin.merge_settings(
                plugin.DEFAULT_SETTINGS,
                {"api_endpoint": "http://example.com/generate"},
            )
        self.assertEqual(raised.exception.code, "INVALID_CONFIGURATION")

    def test_success_writes_only_workspace_and_public_outputs_never_contain_secret(self):
        class Client:
            def __init__(self, api_key, *_args):
                self.api_key = api_key

            def generate(self, **kwargs):
                self.assert_secret()
                return support.ProviderImageResponse(
                    image_bytes=PNG_1X1,
                    mime_type="image/png",
                    width=1,
                    height=1,
                    request_id="safe-request-id",
                    model_id=kwargs["model_id"],
                    model_version=kwargs["model_version"],
                )

            def assert_secret(self):
                if self.api_key != SECRET_SENTINEL:
                    raise AssertionError("secret did not resolve machine-locally")

        with tempfile.TemporaryDirectory() as root:
            os.environ[SECRET_ENV] = SECRET_SENTINEL
            state, output = self.initialized_state(root)
            result = plugin.execute_visual_generate(payload(), state, Client)
            destination = output / result["relative_output"]
            self.assertTrue(destination.is_file())
            self.assertEqual(destination.read_bytes(), PNG_1X1)

            public = json.dumps(
                {
                    "request": payload(),
                    "initialize": state.initialize(
                        {
                            "job_workspace": {"output": str(output)},
                            "settings": {
                                "api_endpoint": "https://api.example.invalid/v1/images/generations",
                                "api_key_env": SECRET_ENV,
                                "timeout_seconds": 3,
                                "model": "fixture-image",
                                "model_version": "fixture-v1",
                            },
                        }
                    ),
                    "health": plugin.health_result(state),
                    "capabilities": plugin.capabilities_result(state),
                    "result": result,
                },
                sort_keys=True,
            )
            self.assertNotIn(SECRET_SENTINEL, public)
            self.assertEqual(result["provenance"]["execution_target"], "api")
            self.assertEqual(result["provenance"]["model_id"], "fixture-image")

    def test_health_is_provider_neutral_and_symbolic(self):
        state = plugin.PluginState()
        state.settings["api_key_env"] = SECRET_ENV
        missing = plugin.health_result(state)
        self.assertEqual(missing["api_execution"]["credential"], "missing")
        os.environ[SECRET_ENV] = SECRET_SENTINEL
        ready = plugin.health_result(state)
        self.assertEqual(ready["api_execution"]["credential"], "available")
        self.assertNotIn(SECRET_SENTINEL, json.dumps(ready))

    def test_protocol_preserves_retryable_failure(self):
        class Client:
            def __init__(self, *_args):
                pass

            def generate(self, **_kwargs):
                raise plugin.PluginFailure("RATE_LIMITED", "slow down", retryable=True)

        with tempfile.TemporaryDirectory() as root:
            os.environ[SECRET_ENV] = SECRET_SENTINEL
            state, _ = self.initialized_state(root)
            response, shutdown = plugin.handle_request(
                {
                    "api_version": 1,
                    "request_id": "req-rate",
                    "method": "plugin.execute",
                    "params": {
                        "operation": "visual.generate",
                        "payload": payload(),
                    },
                },
                state,
                Client,
            )
            self.assertFalse(shutdown)
            self.assertEqual(response["error"]["code"], "RATE_LIMITED")
            self.assertTrue(response["error"]["retryable"])


if __name__ == "__main__":
    unittest.main()
