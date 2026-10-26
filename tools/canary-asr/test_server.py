"""Focused protocol regressions; no NeMo import, microphone, model or benchmark."""

import hashlib
import io
import json
from pathlib import Path
import socket
import tempfile
import threading
import time
import unittest
from unittest.mock import patch
import wave

import server


TOKEN = b"a" * 64


def wav(rate=16000, channels=1, width=2, frames=160):
    output = io.BytesIO()
    with wave.open(output, "wb") as clip:
        clip.setnchannels(channels)
        clip.setsampwidth(width)
        clip.setframerate(rate)
        clip.writeframes(b"\0" * frames * width * channels)
    return output.getvalue()


def multipart(audio, model=server.MODEL, language="en"):
    boundary = "BonsaiCanaryFixture"
    body = bytearray()
    for name, value in (("model", model), ("language", language)):
        body.extend(
            f'--{boundary}\r\nContent-Disposition: form-data; name="{name}"\r\n\r\n{value}\r\n'.encode()
        )
    body.extend(
        f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="clip.wav"\r\n'
        "Content-Type: audio/wav\r\n\r\n".encode()
    )
    body.extend(audio)
    body.extend(f"\r\n--{boundary}--\r\n".encode())
    return f"multipart/form-data; boundary={boundary}", bytes(body)


class FakeRecognizer:
    ready = True

    def __init__(self):
        self.calls = []

    def transcribe(self, audio, deadline, peer):
        self.calls.append(audio)
        return "A reviewed transcript."


class HttpContractTests(unittest.TestCase):
    def setUp(self):
        self.recognizer = FakeRecognizer()
        self.service = server.Server(0, TOKEN, self.recognizer)
        self.port = self.service.server_address[1]
        self.thread = threading.Thread(target=self.service.serve_forever, daemon=True)
        self.thread.start()

    def tearDown(self):
        self.service.shutdown()
        self.thread.join(timeout=3)
        self.service.server_close()

    def exchange(self, request, headers=(), body=b"", token=TOKEN):
        fields = [
            f"{request} HTTP/1.1",
            f"Host: 127.0.0.1:{self.port}",
            "Authorization: Bearer " + token.decode(),
            "Connection: close",
        ]
        fields.extend(headers)
        message = "\r\n".join(fields).encode() + b"\r\n\r\n" + body
        with socket.create_connection(("127.0.0.1", self.port), timeout=3) as peer:
            peer.sendall(message)
            response = bytearray()
            while chunk := peer.recv(65536):
                response.extend(chunk)
        head, payload = bytes(response).split(b"\r\n\r\n", 1)
        self.assertNotIn(TOKEN, response)
        return int(head.split()[1]), json.loads(payload)

    def test_authenticated_health_and_transcription_use_exact_model(self):
        status, health = self.exchange("GET /health")
        self.assertEqual((status, health), (200, server.capabilities(True)))
        audio = wav()
        content_type, body = multipart(audio)
        status, result = self.exchange(
            "POST /v1/audio/transcriptions",
            (f"Content-Type: {content_type}", f"Content-Length: {len(body)}"), body,
        )
        self.assertEqual(status, 200)
        self.assertEqual(result, {"text": "A reviewed transcript.", "model": server.MODEL})
        self.assertEqual(self.recognizer.calls, [audio])

    def test_auth_framing_models_and_language_fail_before_inference(self):
        status, _ = self.exchange("GET /health", token=b"b" * 64)
        self.assertEqual(status, 401)
        status, _ = self.exchange("GET /health", ("Content-Length: 0", "Content-Length: 0"))
        self.assertEqual(status, 400)
        for model, language in (("other", "en"), (server.MODEL, "fr")):
            content_type, body = multipart(wav(), model, language)
            status, _ = self.exchange(
                "POST /v1/audio/transcriptions",
                (f"Content-Type: {content_type}", f"Content-Length: {len(body)}"), body,
            )
            self.assertEqual(status, 400)
        status, _ = self.exchange(
            "POST /v1/audio/transcriptions", ("Content-Length: 1048577",)
        )
        self.assertEqual(status, 413)
        self.assertEqual(self.recognizer.calls, [])

    def test_invalid_pcm_and_unavailable_model_never_transcribe(self):
        for audio in (wav(rate=8000), wav(channels=2), wav(width=1), wav(frames=0), wav()[:-1]):
            content_type, body = multipart(audio)
            status, _ = self.exchange(
                "POST /v1/audio/transcriptions",
                (f"Content-Type: {content_type}", f"Content-Length: {len(body)}"), body,
            )
            self.assertEqual(status, 400)
        self.recognizer.ready = False
        content_type, body = multipart(wav())
        status, result = self.exchange(
            "POST /v1/audio/transcriptions",
            (f"Content-Type: {content_type}", f"Content-Length: {len(body)}"), body,
        )
        self.assertEqual((status, result), (503, {"error": {"code": "model_unavailable"}}))
        self.assertEqual(self.recognizer.calls, [])


class ArtifactContractTests(unittest.TestCase):
    def test_tokenizer_cache_contains_only_reviewed_revision_and_assets(self):
        with tempfile.TemporaryDirectory(prefix="Canary-tokenizer-contract-") as directory:
            root = Path(directory)
            snapshot, scratch = root / "source", root / "worker"
            snapshot.mkdir()
            scratch.mkdir()
            files = {}
            for name in server.TOKENIZER_FILES:
                content = ("synthetic reviewed " + name).encode()
                (snapshot / name).write_bytes(content)
                files[name] = {"bytes": len(content), "sha256": hashlib.sha256(content).hexdigest()}
            revision = "a" * 40
            lock = root / "tokenizer.lock.json"
            lock.write_text(json.dumps({"schema_version": 1, "model": server.TOKENIZER_MODEL,
                                        "revision": revision, "files": files}), encoding="utf-8")
            with patch.object(server, "TOKENIZER_LOCK_PATH", lock):
                cache = server.seal_tokenizer_cache(snapshot, scratch)
                repo = cache / "models--Qwen--Qwen3-1.7B"
                self.assertEqual((repo / "refs" / "main").read_text(), revision)
                copied = repo / "snapshots" / revision
                self.assertEqual({entry.name for entry in copied.iterdir()}, server.TOKENIZER_FILES)
                for name in server.TOKENIZER_FILES:
                    self.assertEqual((copied / name).read_bytes(), (snapshot / name).read_bytes())
                (snapshot / "added_tokens.json").write_bytes(b"unreviewed")
                with self.assertRaises(ValueError):
                    server.verified_tokenizer_snapshot(snapshot)
                (snapshot / "added_tokens.json").unlink()
                (snapshot / "tokenizer.json").write_bytes(b"changed")
                with self.assertRaises(ValueError):
                    server.verified_tokenizer_snapshot(snapshot)

    def test_pinned_artifact_mismatch_rejects_before_model_import(self):
        with tempfile.TemporaryDirectory(prefix="Canary-contract-") as directory:
            artifact = Path(directory) / "model.safetensors"
            artifact.write_bytes(b"synthetic artifact")
            server.verified_file(artifact, hashlib.sha256(artifact.read_bytes()).hexdigest(), 18)
            with self.assertRaises(ValueError):
                server.verified_file(artifact, "0" * 64, 18)

    def test_multipart_duplicates_and_oversized_duration_reject(self):
        content_type, body = multipart(wav())
        repeated = body.replace(
            b'Content-Disposition: form-data; name="language"',
            b'Content-Disposition: form-data; name="model"',
        )
        with self.assertRaises(server.RequestError):
            server.parse_multipart(content_type, repeated)
        with self.assertRaises(server.RequestError):
            server.validate_wav(wav(frames=server.SAMPLE_RATE * server.MAX_CLIP_SECONDS + 1))


class ProcessCancellationTests(unittest.TestCase):
    def test_timeout_terminates_model_and_cleans_external_audio_storage(self):
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

            def send(self, audio):
                pass

            def poll(self, timeout):
                return False

            def close(self):
                self.closed = True

        recognizer = server.ProcessRecognizer.__new__(server.ProcessRecognizer)
        recognizer.process = Process()
        recognizer.connection = Connection()
        recognizer.scratch = tempfile.TemporaryDirectory(prefix="Canary-cancel-contract-")
        scratch = Path(recognizer.scratch.name)
        (scratch / "synthetic.wav").write_bytes(wav())
        recognizer.ready = True
        with socket.socket() as peer:
            with self.assertRaises(server.RequestError) as error:
                recognizer.transcribe(wav(), time.monotonic() - 1, peer)
        self.assertEqual(error.exception.status, 504)
        self.assertFalse(recognizer.ready)
        self.assertTrue(recognizer.connection.closed)
        self.assertFalse(scratch.exists())


if __name__ == "__main__":
    unittest.main()
