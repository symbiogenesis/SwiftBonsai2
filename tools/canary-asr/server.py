"""Bounded loopback HTTP companion for explicitly provisioned Canary-Qwen-2.5B.

The desktop UI stays Rust/native. This module has no microphone access and never
downloads a checkpoint. NeMo imports occur only in the isolated model process.
"""

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
import shutil
import socket
import socketserver
import stat
import struct
import tempfile
import time
from typing import Protocol
import wave

MODEL = "nvidia/canary-qwen-2.5b"
SAMPLE_RATE = 16000
MAX_CLIP_SECONDS = 30
MAX_BODY = 1024 * 1024
MAX_HEAD = 16 * 1024
MAX_TEXT = 32768
INTAKE_SECONDS = 10.0
TRANSCRIBE_SECONDS = 120.0
WRITE_SECONDS = 5.0
MAX_NEW_TOKENS = 256
LOCK_PATH = Path(__file__).with_name("model.lock.json")
TOKENIZER_LOCK_PATH = Path(__file__).with_name("tokenizer.lock.json")
TOKENIZER_MODEL = "Qwen/Qwen3-1.7B"
TOKENIZER_FILES = frozenset((
    "config.json", "generation_config.json", "merges.txt", "tokenizer.json",
    "tokenizer_config.json", "vocab.json",
))


class RequestError(Exception):
    """A value-free error suitable for the public response."""

    def __init__(self, status: int, code: str):
        self.status = status
        self.code = code


class Recognizer(Protocol):
    ready: bool

    def transcribe(self, audio: bytes, deadline: float, peer: socket.socket) -> str: ...


def capabilities(ready: bool) -> dict:
    return {
        "model": MODEL,
        "ready": ready,
        "sample_rate": SAMPLE_RATE,
        "max_clip_seconds": MAX_CLIP_SECONDS,
        "input_format": "pcm_s16le_wav",
        "channels": 1,
    }


def validate_wav(audio: bytes) -> None:
    """Accept complete, bounded mono 16 kHz signed-16 PCM WAV clips only."""
    if len(audio) < 44 or len(audio) > MAX_BODY - 4096:
        raise RequestError(400, "invalid_audio")
    if audio[:4] != b"RIFF" or audio[8:12] != b"WAVE":
        raise RequestError(400, "invalid_audio")
    if struct.unpack_from("<I", audio, 4)[0] + 8 != len(audio):
        raise RequestError(400, "invalid_audio")
    try:
        with wave.open(io.BytesIO(audio), "rb") as clip:
            frames = clip.getnframes()
            if (
                clip.getnchannels() != 1
                or clip.getsampwidth() != 2
                or clip.getframerate() != SAMPLE_RATE
                or clip.getcomptype() != "NONE"
                or not 0 < frames <= SAMPLE_RATE * MAX_CLIP_SECONDS
            ):
                raise RequestError(400, "invalid_audio")
            if len(clip.readframes(frames + 1)) != frames * 2:
                raise RequestError(400, "invalid_audio")
    except (wave.Error, EOFError, ValueError, struct.error):
        raise RequestError(400, "invalid_audio") from None


def parse_multipart(content_type: str, body: bytes) -> bytes:
    """Parse a deliberately small, strict multipart subset without disk spooling."""
    match = re.fullmatch(
        r'multipart/form-data\s*;\s*boundary=(?:"([A-Za-z0-9_-]{1,70})"|([A-Za-z0-9_-]{1,70}))',
        content_type,
        re.IGNORECASE,
    )
    if not match:
        raise RequestError(415, "unsupported_media_type")
    boundary = (match[1] or match[2]).encode("ascii")
    delimiter = b"--" + boundary
    if not body.startswith(delimiter + b"\r\n") or not body.endswith(
        b"\r\n" + delimiter + b"--\r\n"
    ):
        raise RequestError(400, "invalid_multipart")
    fields = {}
    for part in body[len(delimiter) + 2 : -len(delimiter) - 6].split(
        b"\r\n" + delimiter + b"\r\n"
    ):
        head, separator, payload = part.partition(b"\r\n\r\n")
        if not separator or len(head) > 1024:
            raise RequestError(400, "invalid_multipart")
        headers = {}
        for line in head.split(b"\r\n"):
            name, colon, value = line.partition(b":")
            name = name.lower()
            if not colon or name in headers or name not in {
                b"content-disposition", b"content-type"
            }:
                raise RequestError(400, "invalid_multipart")
            headers[name] = value.strip()
        disposition = headers.get(b"content-disposition", b"")
        field = re.fullmatch(
            rb'form-data;\s*name="(model|language|file)"(?:;\s*filename="[^"\r\n]{1,128}")?',
            disposition,
        )
        if not field or field[1] in fields:
            raise RequestError(400, "invalid_multipart")
        name = field[1]
        if name == b"file":
            if b"filename=" not in disposition or headers.get(b"content-type") not in {
                b"audio/wav", b"audio/x-wav", b"application/octet-stream"
            }:
                raise RequestError(400, "invalid_audio")
        elif b"filename=" in disposition or b"content-type" in headers:
            raise RequestError(400, "invalid_multipart")
        fields[name] = payload
    if fields.get(b"model") != MODEL.encode() or b"file" not in fields:
        raise RequestError(400, "unsupported_model")
    if fields.get(b"language", b"en") != b"en":
        raise RequestError(400, "unsupported_language")
    audio = fields[b"file"]
    validate_wav(audio)
    return audio


def receive_request(peer: socket.socket, port: int, token: bytes) -> tuple[str, bytes]:
    deadline = time.monotonic() + INTAKE_SECONDS
    raw = bytearray()
    while b"\r\n\r\n" not in raw:
        if len(raw) >= MAX_HEAD:
            raise RequestError(431, "header_limit")
        peer.settimeout(max(0.001, deadline - time.monotonic()))
        if time.monotonic() >= deadline:
            raise RequestError(408, "intake_timeout")
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
        if name in headers or any(byte < 32 and byte != 9 for byte in value):
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
        return "health", b""
    if request != b"POST /v1/audio/transcriptions HTTP/1.1":
        raise RequestError(404, "unsupported_route")
    length = headers.get(b"content-length", b"")
    if not re.fullmatch(rb"[0-9]{1,8}", length) or not 0 < int(length) <= MAX_BODY:
        raise RequestError(413, "body_limit")
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
    try:
        content_type = headers.get(b"content-type", b"").decode("ascii")
    except UnicodeDecodeError:
        raise RequestError(415, "unsupported_media_type") from None
    return "transcribe", parse_multipart(content_type, bytes(body))


def send_json(peer: socket.socket, status: int, value: dict) -> None:
    body = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    head = (
        f"HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\n"
        f"Content-Length: {len(body)}\r\nConnection: close\r\nCache-Control: no-store\r\n"
        "X-Content-Type-Options: nosniff\r\n\r\n"
    ).encode("ascii")
    peer.settimeout(WRITE_SECONDS)
    peer.sendall(head + body)


class Handler(socketserver.BaseRequestHandler):
    def handle(self) -> None:
        try:
            route, audio = receive_request(self.request, self.server.server_address[1], self.server.token)
            if route == "health":
                send_json(self.request, 200, capabilities(self.server.recognizer.ready))
                return
            if not self.server.recognizer.ready:
                raise RequestError(503, "model_unavailable")
            text = self.server.recognizer.transcribe(
                audio, time.monotonic() + TRANSCRIBE_SECONDS, self.request
            )
            if not isinstance(text, str) or len(text.encode("utf-8")) > MAX_TEXT:
                raise RequestError(502, "invalid_transcript")
            send_json(self.request, 200, {"text": text, "model": MODEL})
        except RequestError as error:
            with contextlib.suppress(OSError):
                send_json(self.request, error.status, {"error": {"code": error.code}})
        except (OSError, ValueError):
            # Disconnects and malformed input do not enter process logs.
            pass


class Server(socketserver.TCPServer):
    """One active connection and a bounded OS backlog; no handler threads."""

    allow_reuse_address = False
    request_queue_size = 1

    def __init__(self, port: int, token: bytes, recognizer: Recognizer):
        self.token = token
        self.recognizer = recognizer
        super().__init__(("127.0.0.1", port), Handler)

    def server_bind(self) -> None:
        if hasattr(socket, "SO_EXCLUSIVEADDRUSE"):
            self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        super().server_bind()

    def handle_error(self, request, client_address) -> None:
        # Never let socketserver print tracebacks containing request/model data.
        pass


def _model_worker(connection, snapshot: str, tokenizer_snapshot: str, device: str, scratch: str) -> None:
    # Suppress third-party diagnostics: transcripts, file paths and model errors
    # never become companion logs. The parent receives value-free categories.
    with open(os.devnull, "w", encoding="utf-8") as quiet:
        with contextlib.redirect_stdout(quiet), contextlib.redirect_stderr(quiet):
            try:
                os.chdir(scratch)
                os.environ["HF_HUB_OFFLINE"] = "1"
                os.environ["TRANSFORMERS_OFFLINE"] = "1"
                cache = seal_tokenizer_cache(Path(tokenizer_snapshot), Path(scratch))
                # Set all supported cache names before importing HF/NeMo. The
                # inherited unversioned cache is never consulted by this worker.
                os.environ["HF_HOME"] = str(Path(scratch) / "hf-home")
                os.environ["HF_HUB_CACHE"] = str(cache)
                os.environ["HUGGINGFACE_HUB_CACHE"] = str(cache)
                os.environ["TRANSFORMERS_CACHE"] = str(cache)
                import torch
                from nemo.collections.speechlm2.models import SALM

                model = SALM.from_pretrained(
                    snapshot, local_files_only=True, trust_remote_code=False,
                    map_location=device, strict=True,
                ).eval().to(device)
                if int(model.sampling_rate) != SAMPLE_RATE:
                    connection.send((False, "model_format"))
                    return
                connection.send((True, "ready"))
                while True:
                    audio = connection.recv()
                    if audio is None:
                        return
                    # A private external directory is removed after every clip.
                    with tempfile.TemporaryDirectory(prefix="clip-", dir=scratch) as clip_dir:
                        path = Path(clip_dir) / "speech.wav"
                        path.write_bytes(audio)
                        with torch.inference_mode():
                            answer_ids = model.generate(
                                prompts=[[{
                                    "role": "user",
                                    "content": f"Transcribe the following: {model.audio_locator_tag}",
                                    "audio": [str(path)],
                                }]],
                                max_new_tokens=MAX_NEW_TOKENS,
                            )
                        ids = answer_ids[0].cpu()
                        if len(ids) >= MAX_NEW_TOKENS and int(ids[-1]) != int(model.text_eos_id):
                            connection.send((False, "transcript_limit"))
                        else:
                            text = model.tokenizer.ids_to_text(ids)
                            if len(text.encode("utf-8")) > MAX_TEXT:
                                connection.send((False, "transcript_limit"))
                            else:
                                connection.send((True, text))
            except BaseException:
                with contextlib.suppress(BrokenPipeError, EOFError, OSError):
                    connection.send((False, "model_failure"))
            finally:
                connection.close()


class ProcessRecognizer:
    """Terminate model work on timeout/disconnect; require explicit restart."""

    def __init__(self, snapshot: Path, tokenizer_snapshot: Path, device: str, startup_seconds: float):
        self.ready = False
        self.scratch = tempfile.TemporaryDirectory(prefix="Bonsai-Canary-")
        context = multiprocessing.get_context("spawn")
        self.connection, child = context.Pipe()
        self.process = context.Process(
            target=_model_worker,
            args=(child, str(snapshot), str(tokenizer_snapshot), device, self.scratch.name),
            daemon=True,
        )
        self.process.start()
        child.close()
        if not self.connection.poll(startup_seconds):
            self.close()
            raise ValueError("model startup timed out")
        try:
            self.ready, _ = self.connection.recv()
        except (EOFError, OSError):
            self.ready = False
        if not self.ready:
            self.close()
            raise ValueError("model could not start from local cache")

    @property
    def ready(self) -> bool:
        return self._ready and self.process.is_alive()

    @ready.setter
    def ready(self, value: bool) -> None:
        self._ready = value

    def transcribe(self, audio: bytes, deadline: float, peer: socket.socket) -> str:
        if not self.ready:
            raise RequestError(503, "model_unavailable")
        try:
            self.connection.send(audio)
            while time.monotonic() < deadline:
                if self.connection.poll(0.05):
                    success, result = self.connection.recv()
                    if not success:
                        if result != "transcript_limit":
                            self.close()
                        raise RequestError(502, result)
                    return result
                readable, _, _ = select.select([peer], [], [], 0)
                if readable:
                    # Sending EOF or pipelined bytes cancels the utterance.
                    self.close()
                    raise RequestError(400, "client_cancelled")
            self.close()
            raise RequestError(504, "transcription_timeout")
        except (EOFError, BrokenPipeError, OSError):
            self.close()
            raise RequestError(503, "model_unavailable") from None

    def close(self) -> None:
        self.ready = False
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
    digest = hashlib.sha256()
    total = 0
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
        lock_bytes = source.read(8193)
    if len(lock_bytes) > 8192:
        raise ValueError("invalid model lock")
    pin = json.loads(lock_bytes)
    if pin.get("schema_version") != 1 or pin.get("model") != MODEL:
        raise ValueError("invalid model lock")
    if not re.fullmatch(r"[a-f0-9]{40}", pin.get("revision", "")):
        raise ValueError("invalid model revision")
    files = pin.get("files", {})
    if set(files) != {"config.json", "model.safetensors"}:
        raise ValueError("invalid model artifacts")
    reject_unpinned_files(snapshot, set(files))
    for name, artifact in files.items():
        verified_file(snapshot / name, artifact["sha256"], artifact["bytes"])


def reject_unpinned_files(snapshot: Path, expected: set[str]) -> None:
    documentation = {"README.md", "LICENSE", "LICENSE.txt", "NOTICE", "NOTICE.txt", ".gitattributes"}
    with os.scandir(snapshot) as entries:
        for entry in entries:
            metadata = entry.stat(follow_symlinks=False)
            if (entry.is_symlink() or getattr(metadata, "st_file_attributes", 0) & 0x400
                    or not stat.S_ISREG(metadata.st_mode)
                    or entry.name not in expected | documentation):
                raise ValueError("unverified model entry")


def verified_tokenizer_snapshot(snapshot: Path) -> dict:
    with TOKENIZER_LOCK_PATH.open("rb") as source:
        encoded = source.read(8193)
    if len(encoded) > 8192:
        raise ValueError("invalid tokenizer lock")
    pin = json.loads(encoded)
    if (pin.get("schema_version") != 1 or pin.get("model") != TOKENIZER_MODEL
            or not re.fullmatch(r"[a-f0-9]{40}", pin.get("revision", ""))
            or set(pin.get("files", {})) != TOKENIZER_FILES):
        raise ValueError("invalid tokenizer lock")
    reject_unpinned_files(snapshot, TOKENIZER_FILES)
    for name, artifact in pin["files"].items():
        verified_file(snapshot / name, artifact["sha256"], artifact["bytes"])
    return pin


def seal_tokenizer_cache(snapshot: Path, scratch: Path) -> Path:
    pin = verified_tokenizer_snapshot(snapshot)
    cache = scratch / "reviewed-hf-cache"
    repo = cache / "models--Qwen--Qwen3-1.7B"
    destination = repo / "snapshots" / pin["revision"]
    destination.mkdir(parents=True, exist_ok=False)
    for name, artifact in pin["files"].items():
        copied = destination / name
        shutil.copyfile(snapshot / name, copied)
        # Hash the copied bytes too: mutation during copying never becomes ready.
        verified_file(copied, artifact["sha256"], artifact["bytes"])
    references = repo / "refs"
    references.mkdir()
    (references / "main").write_text(pin["revision"], encoding="ascii")
    return cache


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--snapshot", required=True, type=Path)
    parser.add_argument("--tokenizer-snapshot", required=True, type=Path)
    parser.add_argument("--token-file", required=True, type=Path)
    parser.add_argument("--port", type=int, default=8766)
    parser.add_argument("--device", choices=("cpu", "cuda"), default="cpu")
    parser.add_argument("--startup-seconds", type=int, default=900)
    args = parser.parse_args()
    try:
        if not 1024 <= args.port <= 65535 or not 1 <= args.startup_seconds <= 1800:
            raise ValueError("invalid service bounds")
        if not args.snapshot.is_absolute() or not args.tokenizer_snapshot.is_absolute() or not args.token_file.is_absolute():
            raise ValueError("absolute local paths required")
        snapshot = args.snapshot.resolve(strict=True)
        verified_snapshot(snapshot)
        tokenizer_snapshot = args.tokenizer_snapshot.resolve(strict=True)
        verified_tokenizer_snapshot(tokenizer_snapshot)
        if args.token_file.is_symlink() or args.token_file.stat().st_size != 64:
            raise ValueError("invalid protected token file")
        with args.token_file.open("rb") as source:
            token = source.read(65)
        if not re.fullmatch(rb"[a-f0-9]{64}", token):
            raise ValueError("invalid protected token file")
        recognizer = ProcessRecognizer(snapshot, tokenizer_snapshot, args.device, args.startup_seconds)
        try:
            with Server(args.port, token, recognizer) as server:
                print(f"Canary ready at 127.0.0.1:{args.port}", flush=True)
                server.serve_forever(poll_interval=0.1)
        finally:
            recognizer.close()
    except KeyboardInterrupt:
        return 0
    except (OSError, ValueError):
        print("Canary startup failed; check local artifacts, dependencies and service configuration.")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
