"""Protocol-only tests with a fake voice designer; no model or benchmark."""

import hashlib
import importlib.util
import io
import json
from pathlib import Path
import socket
import tempfile
import threading
import time
import unittest
from unittest import mock
import wave

spec = importlib.util.spec_from_file_location("bonsai_qwen_tts_server", Path(__file__).with_name("server.py"))
server = importlib.util.module_from_spec(spec)
spec.loader.exec_module(server)
TOKEN = b"c" * 64


def wav(rate=24000):
    output = io.BytesIO()
    with wave.open(output, "wb") as clip:
        clip.setnchannels(1)
        clip.setsampwidth(2)
        clip.setframerate(rate)
        clip.writeframes(b"\0" * 480)
    return output.getvalue()


def speech(**overrides):
    request = {
        "model": server.MODEL, "input": "Hello, 世界.",
        "voice_description": "A gentle, low-pitched invented narrator with a measured rhythm.",
        "language": "Auto", "response_format": "wav",
    }
    request.update(overrides)
    return request


class FakeSynthesizer:
    ready = True

    def __init__(self):
        self.calls = []
        self.audio = wav()

    def synthesize(self, request, deadline, peer):
        self.calls.append(request)
        return self.audio


class HttpContractTests(unittest.TestCase):
    def setUp(self):
        self.synthesizer = FakeSynthesizer()
        self.service = server.Server(0, TOKEN, self.synthesizer)
        self.port = self.service.server_address[1]
        self.thread = threading.Thread(target=self.service.serve_forever, daemon=True)
        self.thread.start()

    def tearDown(self):
        self.service.shutdown()
        self.thread.join(timeout=3)
        self.service.server_close()

    def exchange(self, request, value=None, token=TOKEN, extra_headers=()):
        body = b"" if value is None else json.dumps(value, ensure_ascii=False).encode()
        fields = [f"{request} HTTP/1.1", f"Host: 127.0.0.1:{self.port}",
                  "Authorization: Bearer " + token.decode(), "Connection: close"]
        if value is not None:
            fields.extend(("Content-Type: application/json", f"Content-Length: {len(body)}"))
        fields.extend(extra_headers)
        with socket.create_connection(("127.0.0.1", self.port), timeout=3) as peer:
            peer.sendall("\r\n".join(fields).encode() + b"\r\n\r\n" + body)
            response = bytearray()
            while chunk := peer.recv(65536):
                response.extend(chunk)
        self.assertNotIn(TOKEN, response)
        head, payload = bytes(response).split(b"\r\n\r\n", 1)
        return int(head.split()[1]), head, payload

    def test_health_and_freeform_voice_design_return_pcm_without_presets(self):
        status, _, payload = self.exchange("GET /health")
        self.assertEqual((status, json.loads(payload)), (200, server.capabilities(True)))
        request = speech()
        status, head, audio = self.exchange("POST /v1/audio/speech", request)
        self.assertEqual(status, 200)
        self.assertIn(b"Content-Type: audio/wav", head)
        self.assertEqual(audio, self.synthesizer.audio)
        self.assertEqual(self.synthesizer.calls, [request])
        server.validate_output(audio)

    def test_auth_unknown_model_language_and_text_limits_fail_before_generation(self):
        status, _, _ = self.exchange("GET /health", token=b"d" * 64)
        self.assertEqual(status, 401)
        for request in (
            speech(model="Qwen/other"), speech(language="unsupported"),
            speech(response_format="mp3"), speech(input=""),
            speech(input="x" * (server.MAX_TEXT + 1)),
            speech(voice_description="x" * (server.MAX_VOICE + 1)),
            speech(speaker="preset"), speech(ref_audio="http://private"),
        ):
            status, _, _ = self.exchange("POST /v1/audio/speech", request)
            self.assertEqual(status, 400)
        self.assertEqual(self.synthesizer.calls, [])

    def test_invalid_generated_audio_is_never_forwarded(self):
        self.synthesizer.audio = wav(rate=16000)
        status, head, payload = self.exchange("POST /v1/audio/speech", speech())
        self.assertEqual(status, 502)
        self.assertIn(b"Content-Type: application/json", head)
        self.assertEqual(json.loads(payload), {"error": {"code": "invalid_audio_output"}})

    def test_unavailable_model_does_not_fallback(self):
        self.synthesizer.ready = False
        status, _, payload = self.exchange("POST /v1/audio/speech", speech())
        self.assertEqual(status, 503)
        self.assertEqual(json.loads(payload), {"error": {"code": "model_unavailable"}})
        self.assertEqual(self.synthesizer.calls, [])


class InputAndArtifactTests(unittest.TestCase):
    def test_optional_runtime_assets_cannot_override_verified_tokenizer(self):
        with tempfile.TemporaryDirectory(prefix="Qwen-extra-artifact-") as directory:
            root = Path(directory)
            (root / "speech_tokenizer").mkdir()
            (root / "config.json").write_bytes(b"verified")
            (root / "README.md").write_text("Publisher attribution", encoding="utf-8")
            expected = {"config.json", "speech_tokenizer/config.json"}
            server.reject_unpinned_files(root, expected)
            for name in ("tokenizer.json", "added_tokens.json", "model.safetensors.index.json"):
                extra = root / name
                extra.write_bytes(b"unreviewed")
                with self.assertRaises(ValueError):
                    server.reject_unpinned_files(root, expected)
                extra.unlink()
            (root / "unexpected").mkdir()
            with self.assertRaises(ValueError):
                server.reject_unpinned_files(root, expected)

    def test_duplicate_json_surrogates_and_changed_artifacts_reject(self):
        body = json.dumps(speech())
        with self.assertRaises(server.RequestError):
            server.parse_speech((body[:-1] + ',"input":"replace"}').encode())
        with self.assertRaises(server.RequestError):
            server.parse_speech(json.dumps(speech(input="\ud800")).encode())
        with tempfile.TemporaryDirectory(prefix="Qwen-pin-contract-") as directory:
            artifact = Path(directory) / "model.safetensors"
            artifact.write_bytes(b"synthetic")
            server.verified_file(artifact, hashlib.sha256(b"synthetic").hexdigest(), 9)
            with self.assertRaises(ValueError):
                server.verified_file(artifact, "0" * 64, 9)


class ProcessCancellationTests(unittest.TestCase):
    def test_requested_precision_reaches_worker_and_invalid_precision_never_spawns(self):
        with tempfile.TemporaryDirectory(prefix="Qwen-precision-contract-") as directory:
            scratch = mock.Mock(name=directory)
            scratch.name = directory
            parent, child = mock.Mock(), mock.Mock()
            parent.poll.return_value = True
            parent.recv.return_value = (True, "ready")
            process = mock.Mock()
            process.is_alive.return_value = True
            context = mock.Mock()
            context.Pipe.return_value = (parent, child)
            context.Process.return_value = process
            with mock.patch.object(server.multiprocessing, "get_context", return_value=context), \
                    mock.patch.object(server.tempfile, "TemporaryDirectory", return_value=scratch):
                synthesizer = server.ProcessSynthesizer(Path(directory), "cpu", 1,
                                                       precision="float16")
                self.assertTrue(synthesizer.ready)
                self.assertEqual(context.Process.call_args.kwargs["args"],
                                 (child, directory, "cpu", directory, "float16"))
                process.start.assert_called_once()
                synthesizer.close()
                scratch.cleanup.assert_called_once()
            with mock.patch.object(server.multiprocessing, "get_context") as get_context:
                with self.assertRaises(ValueError):
                    server.ProcessSynthesizer(Path(directory), "cpu", 1,
                                              precision="automatic-fallback")
                get_context.assert_not_called()

    def test_timeout_terminates_worker_and_removes_external_storage(self):
        class Process:
            alive = True

            def is_alive(self):
                return self.alive

            def terminate(self):
                self.alive = False

            def kill(self):
                self.alive = False

            def join(self, timeout):
                pass

        class Connection:
            closed = False

            def send(self, request):
                pass

            def poll(self, timeout):
                return False

            def close(self):
                self.closed = True

        synthesizer = server.ProcessSynthesizer.__new__(server.ProcessSynthesizer)
        synthesizer.process = Process()
        synthesizer.connection = Connection()
        synthesizer.scratch = tempfile.TemporaryDirectory(prefix="Qwen-cancel-contract-")
        scratch = Path(synthesizer.scratch.name)
        synthesizer._ready = True
        with socket.socket() as peer:
            with self.assertRaises(server.RequestError) as error:
                synthesizer.synthesize(speech(), time.monotonic() - 1, peer)
        self.assertEqual(error.exception.status, 504)
        self.assertFalse(synthesizer.ready)
        self.assertTrue(synthesizer.connection.closed)
        self.assertFalse(scratch.exists())


if __name__ == "__main__":
    unittest.main()
