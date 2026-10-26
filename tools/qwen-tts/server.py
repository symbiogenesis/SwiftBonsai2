"""Authenticated local Qwen3-TTS VoiceDesign inference, separate from native UI."""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import hmac
import io
import json
import multiprocessing
import os
from pathlib import Path
import re
import select
import socket
import socketserver
import stat
import tempfile
import time
from typing import Protocol
import wave

MODEL = "Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign"
SAMPLE_RATE = 24000
MAX_TEXT = 2048
MAX_VOICE = 1024
MAX_AUDIO_SECONDS = 120
MAX_AUDIO_SAMPLES = SAMPLE_RATE * MAX_AUDIO_SECONDS
MAX_AUDIO_BYTES = MAX_AUDIO_SAMPLES * 2 + 44
MAX_BODY = 65536
MAX_HEAD = 16384
INTAKE_SECONDS = 10.0
SYNTHESIS_SECONDS = 180.0
WRITE_SECONDS = 5.0
MAX_CODEC_TOKENS = 1500  # Locked codec: 1920 output samples per 12.5 Hz frame.
LANGUAGES = frozenset((
    "Auto", "Chinese", "English", "Japanese", "Korean", "German", "French",
    "Russian", "Portuguese", "Spanish", "Italian",
))
LOCK_PATH = Path(__file__).with_name("model.lock.json")


class RequestError(Exception):
    def __init__(self, status: int, code: str):
        self.status = status
        self.code = code


class Synthesizer(Protocol):
    ready: bool

    def synthesize(self, request: dict, deadline: float, peer: socket.socket) -> bytes: ...


def capabilities(ready: bool) -> dict:
    return {
        "model": MODEL,
        "ready": ready,
        "voice_design": True,
        "sample_rate": SAMPLE_RATE,
        "max_text_characters": MAX_TEXT,
        "max_voice_characters": MAX_VOICE,
    }


def _unique_object(pairs):
    result = {}
    for name, value in pairs:
        if name in result:
            raise RequestError(400, "duplicate_field")
        result[name] = value
    return result


def parse_speech(body: bytes) -> dict:
    try:
        request = json.loads(body.decode("utf-8"), object_pairs_hook=_unique_object)
    except (UnicodeDecodeError, ValueError, RecursionError):
        raise RequestError(400, "invalid_json") from None
    if not isinstance(request, dict) or set(request) != {
        "model", "input", "voice_description", "language", "response_format"
    } or any(not isinstance(value, str) for value in request.values()):
        raise RequestError(400, "invalid_schema")
    if request["model"] != MODEL:
        raise RequestError(400, "unsupported_model")
    if request["language"] not in LANGUAGES:
        raise RequestError(400, "unsupported_language")
    if request["response_format"] != "wav":
        raise RequestError(400, "unsupported_audio_format")
    if not request["input"].strip() or len(request["input"]) > MAX_TEXT:
        raise RequestError(400, "text_limit")
    if not request["voice_description"].strip() or len(request["voice_description"]) > MAX_VOICE:
        raise RequestError(400, "voice_description_limit")
    # Reject lone JSON-surrogate escapes before they reach tokenizer/native code.
    try:
        request["input"].encode("utf-8")
        request["voice_description"].encode("utf-8")
    except UnicodeEncodeError:
        raise RequestError(400, "invalid_unicode") from None
    return request


def validate_output(audio: bytes) -> None:
    if not isinstance(audio, bytes) or not 44 < len(audio) <= MAX_AUDIO_BYTES:
        raise RequestError(502, "invalid_audio_output")
    try:
        with wave.open(io.BytesIO(audio), "rb") as clip:
            frames = clip.getnframes()
            if (
                clip.getnchannels() != 1 or clip.getsampwidth() != 2
                or clip.getframerate() != SAMPLE_RATE or clip.getcomptype() != "NONE"
                or not 0 < frames <= MAX_AUDIO_SAMPLES
                or len(clip.readframes(frames + 1)) != frames * 2
            ):
                raise RequestError(502, "invalid_audio_output")
    except (wave.Error, EOFError, ValueError):
        raise RequestError(502, "invalid_audio_output") from None


def receive_request(peer: socket.socket, port: int, token: bytes) -> tuple[str, dict]:
    deadline = time.monotonic() + INTAKE_SECONDS
    raw = bytearray()
    while b"\r\n\r\n" not in raw:
        if len(raw) >= MAX_HEAD:
            raise RequestError(431, "header_limit")
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise RequestError(408, "intake_timeout")
        peer.settimeout(remaining)
        chunk = peer.recv(min(4096, MAX_HEAD - len(raw)))
        if not chunk:
            raise RequestError(400, "incomplete_request")
        raw.extend(chunk)
    head, prefix = bytes(raw).split(b"\r\n\r\n", 1)
    lines = head.split(b"\r\n")
    if len(lines) > 65 or any(b"\r" in line or b"\n" in line for line in lines):
        raise RequestError(400, "invalid_headers")
    request = lines.pop(0)
    headers = {}
    for line in lines:
        name, colon, value = line.partition(b":")
        if not colon or not re.fullmatch(rb"[A-Za-z0-9!#$%&'*+.^_`|~-]+", name):
            raise RequestError(400, "invalid_headers")
        name = name.lower()
        if name in headers or any(byte < 32 and byte != 9 or byte == 127 for byte in value):
            raise RequestError(400, "invalid_headers")
        headers[name] = value.strip()
    if not hmac.compare_digest(headers.get(b"authorization", b""), b"Bearer " + token):
        raise RequestError(401, "unauthorized")
    if headers.get(b"host") != f"127.0.0.1:{port}".encode():
        raise RequestError(400, "invalid_host")
    if any(name in headers for name in (
        b"transfer-encoding", b"content-encoding", b"expect", b"trailer", b"upgrade",
        b"origin", b"forwarded",
    )) or any(name.startswith(b"x-forwarded-") for name in headers):
        raise RequestError(400, "unsupported_headers")
    if headers.get(b"connection", b"close").lower() != b"close":
        raise RequestError(400, "unsupported_connection")
    if request == b"GET /health HTTP/1.1":
        if prefix or headers.get(b"content-length", b"0") != b"0":
            raise RequestError(400, "invalid_framing")
        return "health", {}
    if request != b"POST /v1/audio/speech HTTP/1.1":
        raise RequestError(404, "unsupported_route")
    length = headers.get(b"content-length", b"")
    if not re.fullmatch(rb"[0-9]{1,8}", length) or not 0 < int(length) <= MAX_BODY:
        raise RequestError(413, "body_limit")
    if headers.get(b"content-type", b"").lower() not in {
        b"application/json", b"application/json; charset=utf-8"
    }:
        raise RequestError(415, "unsupported_media_type")
    length = int(length)
    if len(prefix) > length:
        raise RequestError(400, "invalid_framing")
    body = bytearray(prefix)
    while len(body) < length:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise RequestError(408, "intake_timeout")
        peer.settimeout(remaining)
        chunk = peer.recv(min(65536, length - len(body)))
        if not chunk:
            raise RequestError(400, "incomplete_request")
        body.extend(chunk)
    return "speech", parse_speech(bytes(body))


def send_response(peer: socket.socket, status: int, content_type: str, body: bytes) -> None:
    head = (
        f"HTTP/1.1 {status} Response\r\nContent-Type: {content_type}\r\n"
        f"Content-Length: {len(body)}\r\nConnection: close\r\nCache-Control: no-store\r\n"
        "X-Content-Type-Options: nosniff\r\n\r\n"
    ).encode("ascii")
    peer.settimeout(WRITE_SECONDS)
    peer.sendall(head + body)


def send_json(peer: socket.socket, status: int, value: dict) -> None:
    send_response(peer, status, "application/json", json.dumps(value, separators=(",", ":")).encode())


class Handler(socketserver.BaseRequestHandler):
    def handle(self) -> None:
        try:
            route, request = receive_request(self.request, self.server.server_address[1], self.server.token)
            if route == "health":
                send_json(self.request, 200, capabilities(self.server.synthesizer.ready))
                return
            if not self.server.synthesizer.ready:
                raise RequestError(503, "model_unavailable")
            audio = self.server.synthesizer.synthesize(
                request, time.monotonic() + SYNTHESIS_SECONDS, self.request
            )
            validate_output(audio)
            send_response(self.request, 200, "audio/wav", audio)
        except RequestError as error:
            with contextlib.suppress(OSError):
                send_json(self.request, error.status, {"error": {"code": error.code}})
        except (OSError, ValueError):
            pass


class Server(socketserver.TCPServer):
    allow_reuse_address = False
    request_queue_size = 1

    def __init__(self, port: int, token: bytes, synthesizer: Synthesizer):
        self.token = token
        self.synthesizer = synthesizer
        super().__init__(("127.0.0.1", port), Handler)

    def server_bind(self) -> None:
        if hasattr(socket, "SO_EXCLUSIVEADDRUSE"):
            self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        super().server_bind()

    def handle_error(self, request, client_address) -> None:
        pass  # Prevent request-bearing third-party tracebacks in process logs.


def _model_worker(connection, snapshot: str, device: str, scratch: str) -> None:
    with open(os.devnull, "w", encoding="utf-8") as quiet:
        with contextlib.redirect_stdout(quiet), contextlib.redirect_stderr(quiet):
            try:
                os.chdir(scratch)
                os.environ["HF_HUB_OFFLINE"] = "1"
                os.environ["TRANSFORMERS_OFFLINE"] = "1"
                import numpy as np
                import torch
                from qwen_tts import Qwen3TTSModel

                model = Qwen3TTSModel.from_pretrained(
                    snapshot, local_files_only=True, trust_remote_code=False,
                    device_map="cpu" if device == "cpu" else "cuda:0",
                    dtype=torch.float32 if device == "cpu" else torch.bfloat16,
                    attn_implementation="eager",
                )
                if model.model.tts_model_type != "voice_design":
                    connection.send((False, "model_format"))
                    return
                connection.send((True, "ready"))
                while True:
                    request = connection.recv()
                    if request is None:
                        return
                    with torch.inference_mode():
                        waves, rate = model.generate_voice_design(
                            text=request["input"], language=request["language"],
                            instruct=request["voice_description"],
                            max_new_tokens=MAX_CODEC_TOKENS,
                        )
                    if len(waves) != 1 or rate != SAMPLE_RATE:
                        connection.send((False, "invalid_audio_output"))
                        continue
                    samples = np.asarray(waves[0])
                    # The public wrapper does not expose an EOS finish reason.
                    # Reject output near the codec ceiling conservatively rather
                    # than playing an answer possibly cut by the token budget.
                    if samples.ndim != 1 or not 0 < samples.size < (MAX_CODEC_TOKENS - 2) * 1920:
                        connection.send((False, "audio_limit"))
                        continue
                    if not np.isfinite(samples).all():
                        connection.send((False, "invalid_audio_output"))
                        continue
                    pcm = (np.clip(samples, -1, 1) * 32767).astype("<i2").tobytes()
                    output = io.BytesIO()
                    with wave.open(output, "wb") as clip:
                        clip.setnchannels(1)
                        clip.setsampwidth(2)
                        clip.setframerate(SAMPLE_RATE)
                        clip.writeframes(pcm)
                    connection.send((True, output.getvalue()))
            except BaseException:
                with contextlib.suppress(BrokenPipeError, EOFError, OSError):
                    connection.send((False, "model_failure"))
            finally:
                connection.close()


class ProcessSynthesizer:
    def __init__(self, snapshot: Path, device: str, startup_seconds: float):
        self._ready = False
        self.scratch = tempfile.TemporaryDirectory(prefix="Bonsai-QwenTTS-")
        context = multiprocessing.get_context("spawn")
        self.connection, child = context.Pipe()
        self.process = context.Process(
            target=_model_worker,
            args=(child, str(snapshot), device, self.scratch.name), daemon=True,
        )
        self.process.start()
        child.close()
        if not self.connection.poll(startup_seconds):
            self.close()
            raise ValueError("model startup timed out")
        try:
            self._ready, _ = self.connection.recv()
        except (EOFError, OSError):
            self._ready = False
        if not self.ready:
            self.close()
            raise ValueError("model could not start from local snapshot")

    @property
    def ready(self) -> bool:
        return self._ready and self.process.is_alive()

    def synthesize(self, request: dict, deadline: float, peer: socket.socket) -> bytes:
        if not self.ready:
            raise RequestError(503, "model_unavailable")
        try:
            self.connection.send(request)
            while time.monotonic() < deadline:
                if self.connection.poll(0.05):
                    success, result = self.connection.recv()
                    if not success:
                        if result == "model_failure":
                            self.close()
                        raise RequestError(502, result)
                    return result
                readable, _, _ = select.select([peer], [], [], 0)
                if readable:
                    self.close()
                    raise RequestError(400, "client_cancelled")
            self.close()
            raise RequestError(504, "synthesis_timeout")
        except (EOFError, BrokenPipeError, OSError):
            self.close()
            raise RequestError(503, "model_unavailable") from None

    def close(self) -> None:
        self._ready = False
        if self.process.is_alive():
            self.process.terminate()
        self.process.join(timeout=5)
        if self.process.is_alive():
            self.process.kill()
            self.process.join(timeout=5)
        self.connection.close()
        self.scratch.cleanup()


def verified_file(path: Path, expected_hash: str, expected_size: int) -> None:
    if not re.fullmatch(r"[a-f0-9]{64}", expected_hash) or path.is_symlink() or not path.is_file():
        raise ValueError("invalid pinned artifact")
    if path.stat().st_size != expected_size:
        raise ValueError("pinned artifact size mismatch")
    digest, total = hashlib.sha256(), 0
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            total += len(chunk)
            if total > expected_size:
                raise ValueError("pinned artifact size mismatch")
            digest.update(chunk)
    if total != expected_size or not hmac.compare_digest(digest.hexdigest(), expected_hash):
        raise ValueError("pinned artifact hash mismatch")


def verified_snapshot(snapshot: Path) -> None:
    with LOCK_PATH.open("rb") as source:
        lock_bytes = source.read(16385)
    if len(lock_bytes) > 16384:
        raise ValueError("invalid model lock")
    pin = json.loads(lock_bytes, object_pairs_hook=_unique_object)
    if pin.get("schema_version") != 1 or pin.get("model") != MODEL:
        raise ValueError("invalid model lock")
    if not re.fullmatch(r"[a-f0-9]{40}", pin.get("revision", "")):
        raise ValueError("invalid model revision")
    expected = {
        "config.json", "generation_config.json", "merges.txt", "model.safetensors",
        "preprocessor_config.json", "tokenizer_config.json", "vocab.json",
        "speech_tokenizer/config.json", "speech_tokenizer/configuration.json",
        "speech_tokenizer/model.safetensors", "speech_tokenizer/preprocessor_config.json",
    }
    if set(pin.get("files", {})) != expected:
        raise ValueError("invalid model artifacts")
    reject_unpinned_files(snapshot, expected)
    for name, artifact in pin["files"].items():
        verified_file(snapshot / name, artifact["sha256"], artifact["bytes"])


def reject_unpinned_files(snapshot: Path, expected: set[str]) -> None:
    # Transformers may prefer optional tokenizer/config files when present.
    # Reject these before importing it rather than hashing only the required set.
    documentation = {"README.md", "LICENSE", "LICENSE.txt", "NOTICE", "NOTICE.txt", ".gitattributes"}
    directories = {str(Path(name).parent).replace("\\", "/") for name in expected}
    pending = [snapshot]
    while pending:
        directory = pending.pop()
        with os.scandir(directory) as entries:
            for entry in entries:
                metadata = entry.stat(follow_symlinks=False)
                if entry.is_symlink() or getattr(metadata, "st_file_attributes", 0) & 0x400:
                    raise ValueError("reparse model entry")
                relative = Path(entry.path).relative_to(snapshot).as_posix()
                if stat.S_ISDIR(metadata.st_mode):
                    if relative not in directories:
                        raise ValueError("unverified model directory")
                    pending.append(Path(entry.path))
                elif not stat.S_ISREG(metadata.st_mode) or relative not in expected | documentation:
                    raise ValueError("unverified model artifact")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--snapshot", required=True, type=Path)
    parser.add_argument("--token-file", required=True, type=Path)
    parser.add_argument("--port", type=int, default=8767)
    parser.add_argument("--device", choices=("cpu", "cuda"), default="cpu")
    parser.add_argument("--startup-seconds", type=int, default=900)
    args = parser.parse_args()
    try:
        if not 1024 <= args.port <= 65535 or not 1 <= args.startup_seconds <= 1800:
            raise ValueError("invalid service bounds")
        if not args.snapshot.is_absolute() or not args.token_file.is_absolute():
            raise ValueError("absolute local paths required")
        snapshot = args.snapshot.resolve(strict=True)
        verified_snapshot(snapshot)
        if args.token_file.is_symlink() or args.token_file.stat().st_size != 64:
            raise ValueError("invalid protected token file")
        with args.token_file.open("rb") as source:
            token = source.read(65)
        if not re.fullmatch(rb"[a-f0-9]{64}", token):
            raise ValueError("invalid protected token file")
        synthesizer = ProcessSynthesizer(snapshot, args.device, args.startup_seconds)
        try:
            with Server(args.port, token, synthesizer) as service:
                print(f"Qwen VoiceDesign ready at 127.0.0.1:{args.port}", flush=True)
                service.serve_forever(poll_interval=0.1)
        finally:
            synthesizer.close()
    except KeyboardInterrupt:
        return 0
    except (OSError, ValueError):
        print("Qwen VoiceDesign startup failed; check local artifacts, dependencies and configuration.")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
