#!/usr/bin/env python3
"""EngineBench Container - Pokemon TCG card implementation benchmark.

This container:
1. Sets up a sandbox with the overzealous repo (CG visible, DF stubbed)
2. Runs a coding agent (OpenCode/Claude Code) to implement the card(s)
3. Evaluates with cargo test (deterministic)
4. Scores based on compilation and test results

Usage:
    python -m src.container --port 8017
    uvicorn src.container:app --port 8017
"""

from __future__ import annotations

import asyncio
import hashlib
import json
import os
import re
import secrets
import shutil
import subprocess
import tempfile
import time
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any

from fastapi import Body, FastAPI, Header, HTTPException
from pydantic import BaseModel, Field
from synth_ai.sdk.task.contracts import (
    CandidateValidationIssue,
    RolloutMetrics,
    RolloutResponse,
    ValidateCandidateRequest,
    ValidateCandidateResponse,
)

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

BASE_DIR = Path(__file__).parent.parent
DATA_DIR = BASE_DIR / "data"
GOLD_DIR = BASE_DIR / "gold"
ALGO_BENCH_DIR = BASE_DIR / "auxiliary_tasks" / "algo_bench"
ALGO_BENCH_BENCHMARK = ALGO_BENCH_DIR / "benchmark_ai.py"
ALGO_BENCH_REFERENCE_ALGOS_DIR = ALGO_BENCH_DIR / "reference_algos"
ALGO_BENCH_DATA_DIR = ALGO_BENCH_DIR / "data"
ALGO_BENCH_SERVER_DB = ALGO_BENCH_DATA_DIR / "server.sqlite"
ALGO_BENCH_CARDS_DB = ALGO_BENCH_DATA_DIR / "cards.sqlite"
ALGO_BENCH_BUILD_CACHE_DIR = Path(
    os.getenv("ALGO_BENCH_BUILD_CACHE_DIR", str(BASE_DIR / ".algo_bench_build_cache"))
)
ALGO_BENCH_SHARED_TARGET_DIR = Path(
    os.getenv(
        "ALGO_BENCH_SHARED_TARGET_DIR",
        str(ALGO_BENCH_BUILD_CACHE_DIR / "_cargo_target"),
    )
)
_ALGO_BENCH_SOURCE_FINGERPRINT: str | None = None

# Path to the overzealous repo (source for sandboxes)
OVERZEALOUS_REPO = Path(os.getenv(
    "OVERZEALOUS_REPO_PATH",
    str(Path.home() / "Documents" / "GitHub" / "overzealous")
))


def _extract_trace_correlation_id_from_url(url: str | None) -> str | None:
    """Best-effort extraction for Synth interceptor correlation IDs.

    Supports path-based formats (…/v1/{trial}/{cid}) and canonical query param (?cid=…).
    """
    if not url or not isinstance(url, str):
        return None
    try:
        from urllib.parse import urlparse, parse_qs

        parsed = urlparse(url)
        parts = [p for p in (parsed.path or "").split("/") if p]
        for part in reversed(parts):
            if part.startswith("trace_") or part.startswith("cid_"):
                return part
        qs = parse_qs(parsed.query or "")
        cid = (qs.get("cid") or [None])[0]
        if isinstance(cid, str) and (cid.startswith("trace_") or cid.startswith("cid_")):
            return cid
    except Exception:
        return None
    return None


def _normalize_mode_token(raw: Any) -> str:
    if not isinstance(raw, str):
        return ""
    return raw.strip().lower().replace("-", "_")


def _policy_value(policy_config: dict[str, Any], key: str) -> Any:
    """Resolve policy value from either top-level policy config or nested config block."""
    if key in policy_config and policy_config.get(key) is not None:
        return policy_config.get(key)
    nested = policy_config.get("config")
    if isinstance(nested, dict):
        return nested.get(key)
    return None


def _looks_like_rust_candidate_code(raw: str) -> bool:
    text = raw.strip()
    if len(text) < 24:
        return False
    has_markers = any(
        marker in text
        for marker in (
            "impl ",
            "fn ",
            "pub struct ",
            "use ",
            "AiController",
            "Prompt::",
            "Action::",
        )
    )
    return has_markers and ("\n" in text or ";" in text)


def _extract_candidate_code(policy_config: dict[str, Any]) -> str | None:
    """Extract optimize-anything candidate code from known wrapper layouts."""
    candidates: list[dict[str, Any]] = []

    def _collect(value: Any) -> None:
        if isinstance(value, dict):
            candidates.append(value)

    _collect(policy_config)
    _collect(policy_config.get("config"))
    for root in list(candidates):
        for key in ("artifact_payload", "candidate_artifact", "candidate"):
            _collect(root.get(key))

    for payload in candidates:
        code = payload.get("candidate_code")
        if isinstance(code, str) and code.strip():
            return code

    # Strict: sometimes candidate is wrapped as JSON text in instruction/candidate_content.
    for payload in candidates:
        for text_key in ("candidate_content", "instruction"):
            raw = payload.get(text_key)
            if not isinstance(raw, str) or not raw.strip():
                continue
            try:
                parsed = json.loads(raw)
            except json.JSONDecodeError:
                if _looks_like_rust_candidate_code(raw):
                    return raw
                continue
            if isinstance(parsed, dict):
                code = parsed.get("candidate_code")
                if isinstance(code, str) and code.strip():
                    return code

    return None


def _extract_candidate_code_from_artifact_payload(artifact_payload: Any) -> str | None:
    """Extract candidate_code from validate-candidate artifact payloads."""
    if isinstance(artifact_payload, dict):
        return _extract_candidate_code(artifact_payload)
    if isinstance(artifact_payload, str):
        text = artifact_payload.strip()
        if not text:
            return None
        try:
            parsed = json.loads(text)
        except json.JSONDecodeError:
            if _looks_like_rust_candidate_code(text):
                return text
            return None
        if isinstance(parsed, dict):
            return _extract_candidate_code(parsed)
        if _looks_like_rust_candidate_code(text):
            return text
        return None
    return None


def _validate_algo_bench_candidate_code(candidate_code: str) -> tuple[
    list[CandidateValidationIssue],
    list[CandidateValidationIssue],
]:
    errors: list[CandidateValidationIssue] = []
    warnings: list[CandidateValidationIssue] = []
    stripped = candidate_code.strip()
    byte_len = len(candidate_code.encode("utf-8"))

    if not stripped:
        errors.append(
            CandidateValidationIssue(
                code="EMPTY_CANDIDATE_CODE",
                message="candidate_code is empty",
                path="artifact_payload.candidate_code",
            )
        )
        return errors, warnings

    if byte_len > 120_000:
        errors.append(
            CandidateValidationIssue(
                code="CANDIDATE_TOO_LARGE",
                message=f"candidate_code exceeds 120000 bytes ({byte_len} bytes)",
                path="artifact_payload.candidate_code",
                constraint="max_payload_bytes=120000",
            )
        )

    if "AiController" not in candidate_code:
        errors.append(
            CandidateValidationIssue(
                code="MISSING_AICONTROLLER_REFERENCE",
                message="candidate_code must reference the AiController trait",
                path="artifact_payload.candidate_code",
            )
        )
    elif "impl AiController for" not in candidate_code:
        warnings.append(
            CandidateValidationIssue(
                code="MISSING_DIRECT_IMPL_PATTERN",
                message="candidate_code does not contain `impl AiController for` pattern",
                path="artifact_payload.candidate_code",
            )
        )

    struct_name = _detect_ai_struct_name(candidate_code)
    if not struct_name:
        warnings.append(
            CandidateValidationIssue(
                code="STRUCT_NAME_NOT_DETECTED",
                message="no `pub struct <Name>` declaration detected in candidate_code",
                path="artifact_payload.candidate_code",
            )
        )

    return errors, warnings


def _parse_overall_win_rate(output: str) -> tuple[float | None, int | None, int | None]:
    match = re.search(
        r"Overall:\s*(\d+)\s*wins\s*/\s*(\d+)\s*matches\s*\(([\d.]+)%\)",
        output,
        flags=re.IGNORECASE,
    )
    if not match:
        return None, None, None
    wins = int(match.group(1))
    matches = int(match.group(2))
    rate = float(match.group(3))
    return rate, wins, matches


def _parse_benchmark_metrics_json(output: str) -> dict[str, Any] | None:
    marker = "BENCHMARK_METRICS_JSON:"
    for line in output.splitlines():
        if marker not in line:
            continue
        payload = line.split(marker, 1)[1].strip()
        if not payload:
            continue
        try:
            parsed = json.loads(payload)
        except json.JSONDecodeError:
            continue
        if isinstance(parsed, dict):
            return parsed
    return None


def _detect_ai_struct_name(candidate_code: str) -> str | None:
    match = re.search(r"\bpub\s+struct\s+([A-Za-z_][A-Za-z0-9_]*)\b", candidate_code)
    if not match:
        return None
    return match.group(1)


def _algo_bench_source_fingerprint() -> str:
    global _ALGO_BENCH_SOURCE_FINGERPRINT
    if _ALGO_BENCH_SOURCE_FINGERPRINT:
        return _ALGO_BENCH_SOURCE_FINGERPRINT
    hasher = hashlib.sha256()
    for path in [ALGO_BENCH_BENCHMARK]:
        if path.exists():
            hasher.update(path.read_bytes())
    if ALGO_BENCH_REFERENCE_ALGOS_DIR.exists():
        for path in sorted(ALGO_BENCH_REFERENCE_ALGOS_DIR.glob("*.rs")):
            hasher.update(path.read_bytes())
    _ALGO_BENCH_SOURCE_FINGERPRINT = hasher.hexdigest()[:16]
    return _ALGO_BENCH_SOURCE_FINGERPRINT


def _algo_bench_build_key(candidate_code: str, ai_name: str) -> str:
    material = f"{_algo_bench_source_fingerprint()}\n{ai_name}\n{candidate_code}"
    return hashlib.sha256(material.encode("utf-8")).hexdigest()


def _prune_algo_bench_build_cache(state: "EngineBenchState") -> None:
    now = time.time()
    directories: list[Path] = []
    if state.algo_bench_build_root.exists():
        directories = [
            p for p in state.algo_bench_build_root.iterdir() if p.is_dir() and not p.name.startswith("_")
        ]

    # Drop TTL-expired directories first.
    for directory in list(directories):
        try:
            age = now - directory.stat().st_mtime
        except OSError:
            continue
        if age <= state.algo_bench_build_ttl_seconds:
            continue
        key = directory.name
        lock = state.algo_bench_build_locks.get(key)
        if lock is not None and lock.locked():
            continue
        shutil.rmtree(directory, ignore_errors=True)
        directories.remove(directory)
        state.algo_bench_build_cache.pop(key, None)

    # Enforce max entries by mtime.
    directories.sort(key=lambda p: p.stat().st_mtime if p.exists() else 0.0, reverse=True)
    for directory in directories[state.algo_bench_build_max_entries :]:
        key = directory.name
        lock = state.algo_bench_build_locks.get(key)
        if lock is not None and lock.locked():
            continue
        shutil.rmtree(directory, ignore_errors=True)
        state.algo_bench_build_cache.pop(key, None)


async def ensure_algo_bench_binary(
    state: "EngineBenchState",
    *,
    candidate_code: str,
    ai_name: str,
    build_timeout_seconds: int,
) -> dict[str, Any]:
    """Compile once per candidate and return reusable benchmark binary metadata."""
    build_key = _algo_bench_build_key(candidate_code, ai_name)
    async with state.algo_bench_build_cache_lock:
        cached = state.algo_bench_build_cache.get(build_key)
        if cached and Path(cached["binary_path"]).exists():
            return {
                "success": True,
                "build_cache_hit": True,
                "build_key": build_key,
                "binary_path": cached["binary_path"],
                "workspace_dir": cached["workspace_dir"],
                "duration_seconds": 0.0,
            }
        lock = state.algo_bench_build_locks.get(build_key)
        if lock is None:
            lock = asyncio.Lock()
            state.algo_bench_build_locks[build_key] = lock

    async with lock:
        async with state.algo_bench_build_cache_lock:
            cached = state.algo_bench_build_cache.get(build_key)
            if cached and Path(cached["binary_path"]).exists():
                return {
                    "success": True,
                    "build_cache_hit": True,
                    "build_key": build_key,
                    "binary_path": cached["binary_path"],
                    "workspace_dir": cached["workspace_dir"],
                    "duration_seconds": 0.0,
                }

        workspace_dir = state.algo_bench_build_root / build_key
        binary_path = workspace_dir / "benchmark_bin"
        if workspace_dir.exists() and binary_path.exists():
            async with state.algo_bench_build_cache_lock:
                state.algo_bench_build_cache[build_key] = {
                    "binary_path": str(binary_path),
                    "workspace_dir": str(workspace_dir),
                    "updated_at": time.time(),
                }
            return {
                "success": True,
                "build_cache_hit": True,
                "build_key": build_key,
                "binary_path": str(binary_path),
                "workspace_dir": str(workspace_dir),
                "duration_seconds": 0.0,
            }

        shutil.rmtree(workspace_dir, ignore_errors=True)
        workspace_dir.mkdir(parents=True, exist_ok=True)
        candidate_path = workspace_dir / "candidate_ai.rs"
        candidate_path.write_text(candidate_code, encoding="utf-8")
        shared_binary_path = state.algo_bench_cargo_target_dir / "release" / "benchmark"
        build_env = os.environ.copy()
        build_env["CARGO_TARGET_DIR"] = str(state.algo_bench_cargo_target_dir)

        start_time = time.time()
        proc = await asyncio.create_subprocess_exec(
            "python3",
            str(ALGO_BENCH_BENCHMARK),
            "--mode",
            "build",
            "--ai-code-file",
            str(candidate_path),
            "--name",
            ai_name,
            "--work-dir",
            str(workspace_dir),
            "--overzealous-dir",
            str(OVERZEALOUS_REPO),
            cwd=str(ALGO_BENCH_DIR),
            env=build_env,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        timed_out = False
        try:
            stdout_bytes, stderr_bytes = await asyncio.wait_for(
                proc.communicate(),
                timeout=max(1, int(build_timeout_seconds)),
            )
        except asyncio.TimeoutError:
            timed_out = True
            proc.kill()
            try:
                stdout_bytes, stderr_bytes = await asyncio.wait_for(proc.communicate(), timeout=5)
            except asyncio.TimeoutError:
                stdout_bytes, stderr_bytes = b"", b""

        duration = time.time() - start_time
        stdout = stdout_bytes.decode("utf-8", errors="replace")
        stderr = stderr_bytes.decode("utf-8", errors="replace")
        returncode = int(proc.returncode if proc.returncode is not None else -9)

        if timed_out or returncode != 0 or not shared_binary_path.exists():
            return {
                "success": False,
                "build_cache_hit": False,
                "build_key": build_key,
                "binary_path": str(binary_path),
                "workspace_dir": str(workspace_dir),
                "duration_seconds": duration,
                "returncode": returncode,
                "timed_out": timed_out,
                "stdout": stdout,
                "stderr": stderr,
            }

        try:
            shutil.copy2(shared_binary_path, binary_path)
        except Exception as exc:
            return {
                "success": False,
                "build_cache_hit": False,
                "build_key": build_key,
                "binary_path": str(binary_path),
                "workspace_dir": str(workspace_dir),
                "duration_seconds": duration,
                "returncode": 2,
                "timed_out": False,
                "stdout": stdout,
                "stderr": f"{stderr}\nfailed to snapshot shared benchmark binary: {exc}",
            }

        async with state.algo_bench_build_cache_lock:
            state.algo_bench_build_cache[build_key] = {
                "binary_path": str(binary_path),
                "workspace_dir": str(workspace_dir),
                "updated_at": time.time(),
            }
            _prune_algo_bench_build_cache(state)
        return {
            "success": True,
            "build_cache_hit": False,
            "build_key": build_key,
            "binary_path": str(binary_path),
            "workspace_dir": str(workspace_dir),
            "duration_seconds": duration,
        }


async def run_algo_bench_candidate_eval(
    binary_path: str,
    *,
    workspace_dir: str,
    matches: int,
    seed_base: int,
    timeout_seconds: int,
) -> dict[str, Any]:
    """Evaluate a prebuilt candidate benchmark binary against v1-v4 and return reward."""
    proc = await asyncio.create_subprocess_exec(
        binary_path,
        str(ALGO_BENCH_SERVER_DB),
        str(ALGO_BENCH_CARDS_DB),
        str(matches),
        str(seed_base),
        cwd=workspace_dir,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    timed_out = False
    try:
        stdout_bytes, stderr_bytes = await asyncio.wait_for(
            proc.communicate(),
            timeout=max(1, int(timeout_seconds)),
        )
    except asyncio.TimeoutError:
        timed_out = True
        proc.kill()
        try:
            stdout_bytes, stderr_bytes = await asyncio.wait_for(proc.communicate(), timeout=5)
        except asyncio.TimeoutError:
            stdout_bytes, stderr_bytes = b"", b""
    stdout = stdout_bytes.decode("utf-8", errors="replace")
    stderr = stderr_bytes.decode("utf-8", errors="replace")
    combined = f"{stdout}\n{stderr}"
    win_rate, wins, total = _parse_overall_win_rate(combined)
    benchmark_metrics = _parse_benchmark_metrics_json(combined)

    reward = 0.0
    returncode = int(proc.returncode if proc.returncode is not None else -9)
    if not timed_out and returncode == 0 and win_rate is not None:
        reward = max(0.0, min(1.0, win_rate / 100.0))

    return {
        "returncode": returncode,
        "reward": reward,
        "overall_win_rate": win_rate,
        "wins": wins,
        "matches": total,
        "benchmark_metrics": benchmark_metrics,
        "timed_out": timed_out,
        "timeout_seconds": int(timeout_seconds),
        "stdout": stdout,
        "stderr": stderr,
    }


# ---------------------------------------------------------------------------
# Request/Response Models (matching monorepo convention)
# ---------------------------------------------------------------------------


class RolloutEnvSpec(BaseModel):
    env_id: str | None = None
    env_name: str | None = None
    config: dict[str, Any] = Field(default_factory=dict)
    seed: int | None = None


class RolloutPolicySpec(BaseModel):
    policy_id: str | None = None
    policy_name: str | None = None
    config: dict[str, Any] = Field(default_factory=dict)


class RolloutRecordConfig(BaseModel):
    return_trace: bool = True
    trace_format: str = "full"


class RolloutRequest(BaseModel):
    """Request model for /rollout endpoint."""
    run_id: str = ""
    trace_correlation_id: str = ""
    env: RolloutEnvSpec
    policy: RolloutPolicySpec
    ops: list[Any] = Field(default_factory=list)
    record: RolloutRecordConfig = Field(default_factory=RolloutRecordConfig)
    mode: str = "eval"


# RolloutMetrics and RolloutResponse imported from synth_ai.sdk.container.contracts


# ---------------------------------------------------------------------------
# Container State
# ---------------------------------------------------------------------------


@dataclass
class EngineBenchState:
    """State for the EngineBench container."""
    instance_ids: list[str]
    default_model: str = "gpt-4.1-mini"
    default_timeout: int = 600
    default_loop_limit: int = 30
    openai_api_key: str | None = None
    max_concurrent_rollouts: int = 1
    rollout_semaphore: asyncio.Semaphore = field(init=False, repr=False)
    algo_bench_cache: dict[str, dict[str, Any]] = field(default_factory=dict, repr=False)
    algo_bench_cache_lock: asyncio.Lock = field(init=False, repr=False)
    algo_bench_build_root: Path = ALGO_BENCH_BUILD_CACHE_DIR
    algo_bench_cargo_target_dir: Path = ALGO_BENCH_SHARED_TARGET_DIR
    algo_bench_build_cache: dict[str, dict[str, Any]] = field(default_factory=dict, repr=False)
    algo_bench_build_cache_lock: asyncio.Lock = field(init=False, repr=False)
    algo_bench_build_locks: dict[str, asyncio.Lock] = field(default_factory=dict, repr=False)
    algo_bench_build_max_entries: int = field(
        default_factory=lambda: max(1, int(os.getenv("ALGO_BENCH_BUILD_MAX_ENTRIES", "32")))
    )
    algo_bench_build_ttl_seconds: int = field(
        default_factory=lambda: max(300, int(os.getenv("ALGO_BENCH_BUILD_TTL_SECONDS", "86400")))
    )
    algo_bench_build_timeout_seconds: int = field(
        default_factory=lambda: max(
            60, int(os.getenv("ALGO_BENCH_BUILD_TIMEOUT_SECONDS", "900"))
        )
    )

    def __post_init__(self) -> None:
        self.rollout_semaphore = asyncio.Semaphore(max(1, self.max_concurrent_rollouts))
        self.algo_bench_cache_lock = asyncio.Lock()
        self.algo_bench_build_cache_lock = asyncio.Lock()
        self.algo_bench_build_root.mkdir(parents=True, exist_ok=True)
        self.algo_bench_cargo_target_dir.mkdir(parents=True, exist_ok=True)

    def pick_instance_id(self, seed: int) -> str:
        if not self.instance_ids:
            raise ValueError("No instance IDs configured.")
        return self.instance_ids[seed % len(self.instance_ids)]


# ---------------------------------------------------------------------------
# Instance Loading
# ---------------------------------------------------------------------------


def load_instance_ids() -> list[str]:
    """Load available instance IDs from data directory."""
    instances_dir = DATA_DIR / "instances" / "single"
    if not instances_dir.exists():
        return []
    return [p.stem for p in instances_dir.glob("*.json")]


def load_instance(instance_id: str) -> dict[str, Any]:
    """Load instance specification."""
    instance_path = DATA_DIR / "instances" / "single" / f"{instance_id}.json"
    if not instance_path.exists():
        raise ValueError(f"Instance not found: {instance_id}")
    return json.loads(instance_path.read_text())


# ---------------------------------------------------------------------------
# Sandbox Setup
# ---------------------------------------------------------------------------


async def setup_sandbox(instance_id: str, work_dir: Path) -> Path:
    """Set up a sandbox for the coding agent.

    Creates a copy of the overzealous repo with:
    - Crystal Guardians (CG) expansion fully visible as reference
    - Target expansion (DF/HP) stubbed out (implementation hidden)

    Returns the path to the sandbox repo.
    """
    sandbox_dir = work_dir / "overzealous"

    # Copy the repo
    if not OVERZEALOUS_REPO.exists():
        raise RuntimeError(f"Overzealous repo not found: {OVERZEALOUS_REPO}")

    await asyncio.to_thread(
        shutil.copytree,
        OVERZEALOUS_REPO,
        sandbox_dir,
        symlinks=True,
        ignore=shutil.ignore_patterns(".git", "target", "*.pyc", "__pycache__"),
    )

    # Load instance to get card_file path
    instance = load_instance(instance_id)
    card_file = instance.get("card_file", "")

    if card_file:
        # Use canonical stub from gold/stubs/ if available
        stub_file = GOLD_DIR / "stubs" / f"{instance_id.replace('-', '_')}.rs"
        stub_path = sandbox_dir / card_file

        if stub_file.exists():
            # Create parent directories (needed for HP expansion)
            stub_path.parent.mkdir(parents=True, exist_ok=True)
            stub_path.write_text(stub_file.read_text())

            # Setup expansion module structure if needed (e.g., for HP cards)
            expansion = instance_id.split("-")[0]
            expansion_dir = sandbox_dir / "tcg_expansions" / "src" / expansion

            if expansion != "df":  # DF already has full structure
                card_module = instance_id.replace("-", "_")

                # Create/update cards/mod.rs
                cards_mod_path = expansion_dir / "cards" / "mod.rs"
                if cards_mod_path.exists():
                    content = cards_mod_path.read_text()
                    if f"pub mod {card_module};" not in content:
                        cards_mod_path.write_text(content + f"\npub mod {card_module};")
                else:
                    cards_mod_path.write_text(f"pub mod {card_module};\n")

                # Create expansion/mod.rs if needed
                mod_path = expansion_dir / "mod.rs"
                if not mod_path.exists():
                    mod_path.write_text("pub mod cards;\n")
                elif "pub mod cards;" not in mod_path.read_text():
                    mod_path.write_text(mod_path.read_text() + "\npub mod cards;")

                # Update lib.rs if needed
                lib_path = sandbox_dir / "tcg_expansions" / "src" / "lib.rs"
                if lib_path.exists():
                    lib_content = lib_path.read_text()
                    if f"pub mod {expansion};" not in lib_content:
                        lib_path.write_text(lib_content + f"\npub mod {expansion};\n")
    else:
        # Strict: Stub out DF implementation (canonical behavior)
        df_dir = sandbox_dir / "tcg_expansions" / "src" / "dragon_frontiers"
        if df_dir.exists():
            cards_to_stub = instance.get("cards", [])
            for card in cards_to_stub:
                card_file_path = df_dir / f"{card['id'].replace('-', '_')}.rs"
                if card_file_path.exists():
                    stub = _generate_card_stub(card)
                    card_file_path.write_text(stub)

    return sandbox_dir


def _generate_card_stub(card: dict[str, Any]) -> str:
    """Generate a stub implementation for a card."""
    card_id = card["id"].replace("-", "_")
    card_name = card.get("name", card_id)

    return f'''//! {card_name} - Dragon Frontiers
//!
//! TODO: Implement this card based on the specification.

use tcg_core::prelude::*;

/// {card_name} card implementation.
///
/// See the card specification in the task prompt for details.
pub struct {card_id.title().replace("_", "")} {{
    // TODO: Add fields
}}

impl Card for {card_id.title().replace("_", "")} {{
    fn name(&self) -> &str {{
        "{card_name}"
    }}

    fn id(&self) -> &str {{
        "{card["id"]}"
    }}

    // TODO: Implement remaining Card trait methods
}}

// TODO: Implement card-specific abilities and attacks
'''


# ---------------------------------------------------------------------------
# Agent Backend (Docker / Daytona / Host)
# ---------------------------------------------------------------------------

# "docker" (default), "daytona" (remote sandbox), or "host" (local opencode install; dev-only)
CONTAINER_BACKEND = os.getenv("ENGINE_BENCH_BACKEND", "docker").lower()


def _build_prompt(instance: dict[str, Any], loop_limit: int) -> str:
    """Build the prompt for the coding agent."""
    cards = instance.get("cards", [])
    card_file = instance.get("card_file", "")
    instance_id = instance.get("id", "")

    # Detect expansion from instance
    expansion = instance.get("expansion", "dragon_frontiers")
    expansion_name = "Holon Phantoms" if expansion == "holon_phantoms" else "Dragon Frontiers"
    expansion_code = instance_id.split("-")[0] if instance_id else "df"

    card_specs = "\n\n".join([
        f"### {card['name']}\n{json.dumps(card, indent=2)}"
        for card in cards
    ])

    # Format tests
    tests = instance.get("tests", [])
    def format_test(t):
        desc = t.get('description')
        if desc:
            return f"- {t['name']}: {desc}"
        return f"- {t['name']}: expected={t.get('expected', '?')}"
    test_descriptions = "\n".join([format_test(t) for t in tests]) if tests else "- See card specification"

    return f'''You are implementing Pokemon TCG cards for the {expansion_name} expansion.

## Task
EDIT the file `{card_file}` to implement the card below. You MUST:
1. Actually WRITE code to the file - replace the TODO stubs with working implementations
2. Make sure it compiles without errors
3. Make sure all tests pass

## Cards to Implement
{card_specs}

## File to Edit
`{card_file}` - This file contains stub functions with TODO comments. Replace the TODO implementations with actual working code.

## Tests to Pass
{test_descriptions}

## Instructions
1. READ the stub file at `{card_file}`
2. Look at the Crystal Guardians expansion (tcg_expansions/src/cg/) for reference implementations
3. EDIT `{card_file}` to replace the TODO stubs with working implementations
4. Run `cargo check --package tcg_expansions` to verify compilation
5. Run `cargo test --package tcg_expansions -- {instance_id.replace("-", "_")}` to run tests

## Rules
- You have {loop_limit} steps maximum
- Focus on correct implementation over perfect style
- Use the existing patterns from Crystal Guardians
- You MUST edit the file and write actual code. Do not just describe what to do!
'''


def _docker_bootstrap_script() -> str:
    """Bootstrap script for Docker container."""
    return r'''#!/bin/bash
set -e

# Fast path: prebuilt images should already contain node + opencode.
if command -v opencode >/dev/null 2>&1 && command -v node >/dev/null 2>&1; then
  exit 0
fi

# Rust toolchain is already present in the base image (rust:*-bookworm).
command -v cargo >/dev/null

# Install Node.js for OpenCode (opencode-ai ships a node-based CLI).
export DEBIAN_FRONTEND=noninteractive
apt-get update -y
apt-get install -y nodejs npm ca-certificates

# Install OpenCode
npm install -g opencode-ai
'''


def _opencode_run_script(prompt: str, model: str, api_key: str, base_url: str | None) -> str:
    """Generate script to run OpenCode in container."""
    base = base_url or "https://api.openai.com/v1"
    # Escape the prompt for shell
    escaped_prompt = prompt.replace("'", "'\\''")

    return f'''#!/bin/bash
set -e

export PATH="$HOME/.bun/bin:$HOME/.cargo/bin:$PATH"
export OPENAI_API_KEY="{api_key}"

# Configure OpenCode
mkdir -p ~/.config/opencode
cat > ~/.config/opencode/opencode.json << 'EOFCONFIG'
{{
  "$schema": "https://opencode.ai/config.json",
  "model": "openai/{model}",
  "provider": {{
    "openai": {{
      "npm": "@ai-sdk/openai",
      "name": "OpenAI",
      "models": {{"{model}": {{}}}},
      "options": {{
        "baseURL": "{base}",
        "apiKey": "{api_key}"
      }}
    }}
  }},
  "agent": {{
    "build": {{
      "model": "openai/{model}",
      "permission": {{"edit": "allow", "bash": "allow"}}
    }}
  }}
}}
EOFCONFIG

# Run OpenCode
cd /workspace
opencode run --format json --model "openai/{model}" --title "engine_bench_eval" '{escaped_prompt}'
'''


async def run_agent_docker(
    prompt: str,
    sandbox_dir: Path,
    model: str,
    timeout: int,
    api_key: str,
    base_url: str | None = None,
) -> dict[str, Any]:
    """Run OpenCode agent in a Docker container."""
    import tarfile
    import io

    # Build a tar archive of the sandbox
    tar_buffer = io.BytesIO()
    with tarfile.open(fileobj=tar_buffer, mode="w:gz") as tar:
        tar.add(str(sandbox_dir), arcname="workspace")
    tar_data = tar_buffer.getvalue()

    # Docker image with Rust pre-installed
    image = os.getenv("ENGINE_BENCH_DOCKER_IMAGE", "rust:1.75-bookworm")

    # Create container
    container_name = f"engine_bench_{secrets.token_hex(8)}"

    try:
        # Create and start container
        create_cmd = [
            "docker", "run", "-d",
            "--name", container_name,
            "-e", f"OPENAI_API_KEY={api_key}",
            "--memory", os.getenv("ENGINE_BENCH_DOCKER_MEMORY", "4g"),
            "--cpus", os.getenv("ENGINE_BENCH_DOCKER_CPUS", "2"),
            image,
            "sleep", "infinity",
        ]

        proc = await asyncio.create_subprocess_exec(
            *create_cmd,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await proc.communicate()
        if proc.returncode != 0:
            return {
                "success": False,
                "stdout": stdout.decode("utf-8", errors="replace"),
                "stderr": stderr.decode("utf-8", errors="replace") or "Failed to create Docker container",
            }

        # Copy workspace to container
        copy_proc = await asyncio.create_subprocess_exec(
            "docker", "cp", "-", f"{container_name}:/",
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        cp_out, cp_err = await copy_proc.communicate(input=tar_data)
        if copy_proc.returncode != 0:
            return {
                "success": False,
                "stdout": cp_out.decode("utf-8", errors="replace"),
                "stderr": cp_err.decode("utf-8", errors="replace") or "Failed to copy workspace into Docker container",
            }

        # Install OpenCode in container
        bootstrap = _docker_bootstrap_script()
        bootstrap_cmd = ["docker", "exec", container_name, "bash", "-c", bootstrap]
        proc = await asyncio.create_subprocess_exec(
            *bootstrap_cmd,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        try:
            bs_out, bs_err = await asyncio.wait_for(proc.communicate(), timeout=300)  # 5 min for setup
        except asyncio.TimeoutError:
            return {"success": False, "stdout": "", "stderr": "Docker bootstrap timed out"}
        if proc.returncode != 0:
            return {
                "success": False,
                "stdout": bs_out.decode("utf-8", errors="replace"),
                "stderr": bs_err.decode("utf-8", errors="replace") or "Docker bootstrap failed",
            }

        # Run OpenCode
        run_script = _opencode_run_script(prompt, model, api_key, base_url)
        run_cmd = ["docker", "exec", container_name, "bash", "-c", run_script]

        proc = await asyncio.create_subprocess_exec(
            *run_cmd,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )

        try:
            stdout, stderr = await asyncio.wait_for(proc.communicate(), timeout=timeout)
            success = proc.returncode == 0
        except asyncio.TimeoutError:
            # Kill the process
            kill_cmd = ["docker", "exec", container_name, "pkill", "-9", "opencode"]
            kill_proc = await asyncio.create_subprocess_exec(*kill_cmd)
            await kill_proc.wait()
            stdout, stderr = b"", b"Timed out"
            success = False

        # Copy workspace back
        extract_cmd = ["docker", "cp", f"{container_name}:/workspace", str(sandbox_dir.parent)]
        proc = await asyncio.create_subprocess_exec(
            *extract_cmd,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        ex_out, ex_err = await proc.communicate()
        if proc.returncode != 0:
            # Still return agent output; extraction failure means we can't compute diff/test.
            return {
                "success": False,
                "stdout": stdout.decode("utf-8", errors="replace"),
                "stderr": (
                    "Failed to copy workspace out of Docker container: "
                    + (ex_err.decode("utf-8", errors="replace") or "")
                ),
            }

        return {
            "success": success,
            "stdout": stdout.decode("utf-8", errors="replace"),
            "stderr": stderr.decode("utf-8", errors="replace"),
        }

    finally:
        # Cleanup container
        cleanup_cmd = ["docker", "rm", "-f", container_name]
        proc = await asyncio.create_subprocess_exec(
            *cleanup_cmd,
            stdout=asyncio.subprocess.DEVNULL,
            stderr=asyncio.subprocess.DEVNULL,
        )
        await proc.wait()


async def run_agent_daytona(
    prompt: str,
    sandbox_dir: Path,
    model: str,
    timeout: int,
    api_key: str,
    base_url: str | None = None,
) -> dict[str, Any]:
    """Run OpenCode agent in a Daytona sandbox.

    Uses the DaytonaBackend for:
    - Snapshot-based caching (~10s startup vs ~100s cold start)
    - Automatic disk limit handling with cleanup
    - Proper Rust toolchain and OpenCode installation
    """
    try:
        from src.lib.daytona_backend import run_agent_in_daytona
    except ImportError:
        return {
            "success": False,
            "stdout": "",
            "stderr": "Daytona backend not available. Check daytona_backend.py",
        }

    daytona_api_key = os.getenv("DAYTONA_API_KEY")
    if not daytona_api_key:
        return {
            "success": False,
            "stdout": "",
            "stderr": "DAYTONA_API_KEY environment variable not set",
        }

    # Run using the new backend
    result = await run_agent_in_daytona(
        prompt=prompt,
        sandbox_dir=sandbox_dir,
        model=model,
        timeout=timeout,
        api_key=api_key,
        base_url=base_url,
    )

    # Download modified files back to sandbox_dir
    modified_files = result.get("modified_files", {})
    for rel_path, content in modified_files.items():
        local_path = sandbox_dir / rel_path
        try:
            local_path.parent.mkdir(parents=True, exist_ok=True)
            local_path.write_bytes(content)
        except Exception:
            continue

    return {
        "success": result.get("success", False),
        "stdout": result.get("stdout", ""),
        "stderr": result.get("stderr", ""),
    }


async def run_agent_host(
    prompt: str,
    sandbox_dir: Path,
    model: str,
    timeout: int,
    api_key: str,
    base_url: str | None = None,
) -> dict[str, Any]:
    """Run OpenCode directly on the host (no container).

    This is intended for local development where Docker bootstrap may be unavailable
    or too slow. The sandbox_dir is already a temp copy of the repo, so host runs
    remain isolated from the source checkout.
    """
    config_root = Path(tempfile.mkdtemp(prefix="enginebench_opencode_cfg_"))
    xdg_config_home = config_root / "config"
    xdg_config_home.mkdir(parents=True, exist_ok=True)

    base = base_url or "https://api.openai.com/v1"

    opencode_dir = xdg_config_home / "opencode"
    opencode_dir.mkdir(parents=True, exist_ok=True)
    config_path = opencode_dir / "opencode.json"
    config_path.write_text(
        json.dumps(
            {
                "$schema": "https://opencode.ai/config.json",
                "model": f"openai/{model}",
                "provider": {
                    "openai": {
                        "npm": "@ai-sdk/openai",
                        "name": "OpenAI",
                        "models": {model: {}},
                        "options": {"baseURL": base, "apiKey": api_key},
                    }
                },
                "agent": {
                    "build": {
                        "model": f"openai/{model}",
                        "permission": {"edit": "allow", "bash": "allow"},
                    }
                },
            }
        )
    )

    env = dict(os.environ)
    env["OPENAI_API_KEY"] = api_key
    env["XDG_CONFIG_HOME"] = str(xdg_config_home)
    # Ensure config resolution doesn't touch the user's global config.
    env["HOME"] = str(config_root)

    proc = await asyncio.create_subprocess_exec(
        "opencode",
        "run",
        "--format",
        "json",
        "--model",
        f"openai/{model}",
        "--title",
        "engine_bench_eval",
        prompt,
        cwd=str(sandbox_dir),
        env=env,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    try:
        stdout, stderr = await asyncio.wait_for(proc.communicate(), timeout=timeout)
    except asyncio.TimeoutError:
        proc.kill()
        await proc.wait()
        return {"success": False, "stdout": "", "stderr": "Timed out"}

    return {
        "success": proc.returncode == 0,
        "stdout": stdout.decode("utf-8", errors="replace"),
        "stderr": stderr.decode("utf-8", errors="replace"),
    }


async def run_agent(
    prompt: str,
    sandbox_dir: Path,
    model: str,
    timeout: int,
    api_key: str,
    base_url: str | None = None,
) -> dict[str, Any]:
    """Run the coding agent in the configured backend."""
    if CONTAINER_BACKEND == "daytona":
        return await run_agent_daytona(prompt, sandbox_dir, model, timeout, api_key, base_url)
    if CONTAINER_BACKEND == "host":
        return await run_agent_host(prompt, sandbox_dir, model, timeout, api_key, base_url)
    else:
        return await run_agent_docker(prompt, sandbox_dir, model, timeout, api_key, base_url)


# ---------------------------------------------------------------------------
# Evaluation
# ---------------------------------------------------------------------------


async def run_cargo_build(repo_dir: Path) -> tuple[bool, str]:
    """Run cargo build and return (success, error_output)."""
    proc = await asyncio.create_subprocess_exec(
        "cargo", "build", "--package", "tcg_expansions",
        cwd=str(repo_dir),
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    stdout, stderr = await proc.communicate()

    success = proc.returncode == 0
    output = stderr.decode("utf-8", errors="replace")

    return success, output if not success else ""


async def run_cargo_test(repo_dir: Path, instance_id: str) -> tuple[int, int, str]:
    """Run cargo test and return (passed, total, output)."""
    # Run tests filtered by instance_id
    proc = await asyncio.create_subprocess_exec(
        "cargo", "test", "--package", "tcg_expansions", "--", instance_id.replace("-", "_"),
        cwd=str(repo_dir),
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    stdout, stderr = await proc.communicate()

    output = stdout.decode("utf-8", errors="replace") + stderr.decode("utf-8", errors="replace")

    # Parse test results
    passed = 0
    failed = 0

    import re
    for line in output.split("\n"):
        if "test result:" in line:
            match_passed = re.search(r"(\d+) passed", line)
            match_failed = re.search(r"(\d+) failed", line)
            if match_passed:
                passed = int(match_passed.group(1))
            if match_failed:
                failed = int(match_failed.group(1))

    total = passed + failed
    return passed, total, output


def get_git_diff(repo_dir: Path) -> str:
    """Get the git diff of changes made by the agent."""
    result = subprocess.run(
        ["git", "diff"],
        cwd=str(repo_dir),
        capture_output=True,
        text=True,
    )
    return result.stdout


def calculate_gold_similarity(patch: str, instance_id: str) -> float:
    """Calculate similarity between generated patch and gold reference."""
    gold_path = GOLD_DIR / "patches" / f"{instance_id}.patch"
    if not gold_path.exists():
        return 0.0

    gold_patch = gold_path.read_text()

    # Simple line-based Jaccard similarity
    def extract_changes(p: str) -> set[str]:
        changes = set()
        for line in p.split("\n"):
            if line.startswith("+") and not line.startswith("+++"):
                changes.add(line[1:].strip())
            elif line.startswith("-") and not line.startswith("---"):
                changes.add(line[1:].strip())
        return changes

    changes1 = extract_changes(patch)
    changes2 = extract_changes(gold_patch)

    if not changes1 and not changes2:
        return 1.0
    if not changes1 or not changes2:
        return 0.0

    intersection = changes1 & changes2
    union = changes1 | changes2

    return len(intersection) / len(union)


def calculate_score(
    compile_pass: bool,
    tests_passed: int,
    tests_total: int,
    gold_similarity: float,
) -> float:
    """Calculate final score (0.0-1.0)."""
    if not compile_pass:
        return 0.0

    # Weights
    COMPILE_WEIGHT = 0.20
    TEST_WEIGHT = 0.50
    GOLD_WEIGHT = 0.30

    compile_score = COMPILE_WEIGHT
    test_score = TEST_WEIGHT * (tests_passed / tests_total if tests_total > 0 else 0.0)
    gold_score = GOLD_WEIGHT * gold_similarity

    return compile_score + test_score + gold_score


# ---------------------------------------------------------------------------
# FastAPI App
# ---------------------------------------------------------------------------


def create_container(
    *,
    required_api_key: str | None = None,
    max_concurrent_rollouts: int | None = None,
) -> FastAPI:
    """Create the EngineBench container."""

    instance_ids = load_instance_ids()
    required_api_key = required_api_key or os.getenv("ENVIRONMENT_API_KEY")
    max_concurrent = max_concurrent_rollouts or int(os.getenv("MAX_CONCURRENT_ROLLOUTS", "1"))

    app = FastAPI(
        title="EngineBench Container",
        description="Pokemon TCG card implementation benchmark",
        version="0.1.0",
    )

    state = EngineBenchState(
        instance_ids=instance_ids,
        openai_api_key=os.getenv("OPENAI_API_KEY"),
        max_concurrent_rollouts=max_concurrent,
    )
    app.state.engine_bench = state

    API_KEY_HEADER = "X-API-Key"

    @app.get("/")
    async def root():
        return {"status": "ok", "app": "engine-bench", "instances": len(state.instance_ids)}

    @app.get("/health")
    async def health(x_api_key: str | None = Header(default=None, alias=API_KEY_HEADER)):
        if required_api_key and x_api_key != required_api_key:
            raise HTTPException(status_code=401, detail="Unauthorized")
        return {
            "status": "healthy",
            "instances": len(state.instance_ids),
            "openai_configured": bool(state.openai_api_key),
        }

    @app.get("/info")
    async def info(x_api_key: str | None = Header(default=None, alias=API_KEY_HEADER)):
        if required_api_key and x_api_key != required_api_key:
            raise HTTPException(status_code=401, detail="Unauthorized")

        # Count instances by expansion
        df_count = len([i for i in state.instance_ids if i.startswith("df-")])
        hp_count = len([i for i in state.instance_ids if i.startswith("hp-")])
        return {
            "name": "EngineBench",
            "description": "Pokemon TCG card implementation benchmark",
            # Prompt-learning verifier contract: container must expose rubrics via /info.
            # Backend normalization expects: {"rubrics": {"outcome": <Rubric>, "events": <Rubric>}}
            # Where <Rubric> can be {"criteria": [...]} or a list of criteria dicts.
            "rubrics": {
                "outcome": {
                    "criteria": [
                        {
                            "id": "compiles",
                            "description": (
                                "Code compiles successfully for the target expansion crate without errors."
                            ),
                            "weight": 0.3,
                        },
                        {
                            "id": "tests_pass",
                            "description": (
                                "All task-specific tests pass (the card behavior matches the specification)."
                            ),
                            "weight": 0.7,
                        },
                    ]
                },
                "events": {
                    "criteria": [
                        {
                            "id": "edited_target_file",
                            "description": (
                                "The agent edits the specified target file and implements the required TODOs."
                            ),
                            "weight": 0.4,
                        },
                        {
                            "id": "uses_checks",
                            "description": (
                                "The agent runs compilation and/or tests to validate changes (cargo check/test)."
                            ),
                            "weight": 0.4,
                        },
                        {
                            "id": "scoped_changes",
                            "description": (
                                "Changes are reasonably scoped to the task (no unrelated refactors)."
                            ),
                            "weight": 0.2,
                        },
                    ]
                },
            },
            "modes": ["single_card", "full_deck"],
            "expansions": {
                "dragon_frontiers": {"code": "df", "count": df_count},
                "holon_phantoms": {"code": "hp", "count": hp_count},
            },
            "context_expansion": "Crystal Guardians",
            "total_instances": len(state.instance_ids),
            "instances": state.instance_ids,
        }

    @app.get("/task_info")
    async def task_info(
        x_api_key: str | None = Header(default=None, alias=API_KEY_HEADER),
        seed: int | None = None,
    ):
        if required_api_key and x_api_key != required_api_key:
            raise HTTPException(status_code=401, detail="Unauthorized")

        # Prompt-learning expects TaskInfo to include at least:
        # - environment: string env_name (used for verifier + trace metadata)
        # - task: dict describing the task (opaque to backend; must exist)
        base_info: dict[str, Any] = {
            "environment": "engine_bench",
            "task": {
                "id": "engine_bench",
                "name": "EngineBench",
            },
            # Backwards-compatible fields used by older demos.
            "id": "engine_bench",
            "name": "EngineBench",
            "instance_count": len(state.instance_ids),
        }

        if seed is not None:
            instance_id = state.pick_instance_id(seed)
            base_info["example"] = {"seed": seed, "instance_id": instance_id}
            base_info["task_metadata"] = {
                "seed": seed,
                "instance_id": instance_id,
            }

        return base_info

    @app.post("/upload_context")
    async def upload_context(
        payload: dict[str, Any] = Body(default_factory=dict),
        x_api_key: str | None = Header(default=None, alias=API_KEY_HEADER),
    ) -> dict[str, Any]:
        """Accept actionable upfront context uploads from GEPA/MIPRO runtimes.

        EngineBench evaluates against its checked-out repo and does not require an
        explicit ingestion step, but returning 200 here allows fail-closed context
        upload flows to proceed.
        """
        if required_api_key and x_api_key != required_api_key:
            raise HTTPException(status_code=401, detail="Unauthorized")

        return {
            "status": "accepted",
            "ingested": False,
            "mode": "no_op",
            "reason": "engine_bench task app reads context from local workspace",
            "payload_keys": sorted(payload.keys()),
            "mount_path": payload.get("mount_path"),
            "source_uri": payload.get("source_uri"),
        }

    @app.post("/validate-candidate", response_model=ValidateCandidateResponse)
    async def validate_candidate(
        request: ValidateCandidateRequest,
        x_api_key: str | None = Header(default=None, alias=API_KEY_HEADER),
    ) -> ValidateCandidateResponse:
        """Validate optimize-anything program_code artifacts before rollout."""
        if required_api_key and x_api_key != required_api_key:
            raise HTTPException(status_code=401, detail="Unauthorized")

        if _normalize_mode_token(request.optimization_mode) != "optimize_anything":
            return ValidateCandidateResponse(
                status="valid",
                warnings=[
                    CandidateValidationIssue(
                        code="UNEXPECTED_OPTIMIZATION_MODE",
                        message=(
                            "engine_bench validate-candidate only enforces "
                            "optimize_anything constraints"
                        ),
                        path="optimization_mode",
                    )
                ],
                validation_digest=None,
                validator_version="engine_bench_algo_v1",
            )

        artifact_kind = str(request.artifact_kind or "").strip().lower()
        candidate_code = _extract_candidate_code_from_artifact_payload(request.artifact_payload)
        errors: list[CandidateValidationIssue] = []
        warnings: list[CandidateValidationIssue] = []

        if artifact_kind and artifact_kind != "program_code":
            errors.append(
                CandidateValidationIssue(
                    code="UNSUPPORTED_ARTIFACT_KIND",
                    message=f"artifact_kind must be program_code for algo_bench (got {artifact_kind})",
                    path="artifact_kind",
                    constraint="program_code",
                )
            )

        if not candidate_code:
            errors.append(
                CandidateValidationIssue(
                    code="MISSING_CANDIDATE_CODE",
                    message="artifact payload does not include non-empty candidate_code",
                    path="artifact_payload",
                )
            )
        else:
            code_errors, code_warnings = _validate_algo_bench_candidate_code(candidate_code)
            errors.extend(code_errors)
            warnings.extend(code_warnings)

        status = "invalid" if errors else "valid"
        normalized_preview = (candidate_code or "")[:1000] or None
        validation_digest = (
            hashlib.sha256(candidate_code.encode("utf-8")).hexdigest() if candidate_code else None
        )
        return ValidateCandidateResponse(
            status=status,
            errors=errors,
            warnings=warnings,
            normalized_preview=normalized_preview,
            validation_digest=validation_digest,
            validator_version="engine_bench_algo_v1",
        )

    @app.post("/rollout", response_model=RolloutResponse)
    async def rollout(
        request: RolloutRequest,
        x_api_key: str | None = Header(default=None, alias=API_KEY_HEADER),
    ) -> RolloutResponse:
        """Execute one EngineBench rollout.

        This endpoint:
        1. Sets up a sandbox with the overzealous repo
        2. Runs a coding agent to implement the card(s)
        3. Evaluates with cargo build/test
        4. Returns score based on results
        """
        if required_api_key and x_api_key != required_api_key:
            raise HTTPException(status_code=401, detail="Unauthorized")

        seed = request.env.seed or 0
        env_config = request.env.config or {}
        policy_config = request.policy.config or {}

        instance_id = env_config.get("instance_id") or state.pick_instance_id(seed)
        model = _policy_value(policy_config, "model") or state.default_model
        timeout = int(_policy_value(policy_config, "timeout") or state.default_timeout)
        loop_limit = int(_policy_value(policy_config, "loop_limit") or state.default_loop_limit)
        trace_correlation_id = (
            request.trace_correlation_id
            or _policy_value(policy_config, "trace_correlation_id")
            or _extract_trace_correlation_id_from_url(_policy_value(policy_config, "inference_url"))
            or _extract_trace_correlation_id_from_url(_policy_value(policy_config, "base_url"))
            or f"trace_{secrets.token_hex(12)}"
        )
        # If prompt-learning provides an interceptor URL, use it as the OpenAI base URL.
        # This enables upstream trace hydration and consistent correlation IDs.
        inference_url = _policy_value(policy_config, "inference_url") or _policy_value(
            policy_config, "base_url"
        )
        api_key = _policy_value(policy_config, "api_key") or state.openai_api_key
        optimization_mode = _normalize_mode_token(_policy_value(policy_config, "optimization_mode"))

        # Optimize-anything path for algo_bench program_code candidates. In this mode,
        # rollout reward comes from benchmark win rate of candidate_code vs v1-v4 and
        # does not require agent API keys.
        if optimization_mode == "optimize_anything":
            candidate_code = _extract_candidate_code(policy_config)
            if not candidate_code:
                return _error_response(
                    request.run_id,
                    seed,
                    instance_id,
                    "Missing candidate_code in optimize_anything policy config",
                    trace_correlation_id=trace_correlation_id,
                    inference_url=inference_url if isinstance(inference_url, str) else None,
                )

            matches = env_config.get("algo_bench_matches") or _policy_value(
                policy_config, "algo_bench_matches"
            )
            try:
                matches_i = int(matches) if matches is not None else 4
            except (TypeError, ValueError):
                matches_i = 4
            max_matches_raw = os.getenv("ALGO_BENCH_MAX_MATCHES", "2000")
            try:
                max_matches_i = int(max_matches_raw)
            except (TypeError, ValueError):
                max_matches_i = 2000
            max_matches_i = max(1, max_matches_i)
            matches_i = max(1, min(matches_i, max_matches_i))
            timeout_raw = (
                env_config.get("algo_bench_timeout_seconds")
                or _policy_value(policy_config, "algo_bench_timeout_seconds")
            )
            try:
                timeout_i = int(timeout_raw) if timeout_raw is not None else 180
            except (TypeError, ValueError):
                timeout_i = 180
            timeout_i = max(5, min(timeout_i, 1800))
            configured_ai_name = (
                str(_policy_value(policy_config, "ai_name")).strip()
                if isinstance(_policy_value(policy_config, "ai_name"), str)
                else ""
            )
            detected_ai_name = _detect_ai_struct_name(candidate_code) or ""
            ai_name = configured_ai_name or detected_ai_name or "CandidateAi"
            # Keep seed mapping dense so downstream pair-index modulo logic
            # can cover all deck-pair combinations instead of sparse residues.
            seed_base = max(0, int(seed))
            build_timeout_raw = (
                env_config.get("algo_bench_build_timeout_seconds")
                or _policy_value(policy_config, "algo_bench_build_timeout_seconds")
            )
            try:
                build_timeout_i = (
                    int(build_timeout_raw)
                    if build_timeout_raw is not None
                    else int(state.algo_bench_build_timeout_seconds)
                )
            except (TypeError, ValueError):
                build_timeout_i = int(state.algo_bench_build_timeout_seconds)
            build_timeout_i = max(60, min(build_timeout_i, 3600))
            cache_key_material = (
                f"{_algo_bench_source_fingerprint()}\n{ai_name}\n{matches_i}\n{seed_base}\n{timeout_i}\n{candidate_code}"
            )
            cache_key = hashlib.sha256(cache_key_material.encode("utf-8")).hexdigest()

            async with state.algo_bench_cache_lock:
                cached_eval = state.algo_bench_cache.get(cache_key)

            if cached_eval is not None:
                eval_result = dict(cached_eval)
                duration = 0.0
                cache_hit = True
                build_cache_hit = bool(eval_result.get("build_cache_hit"))
                build_key_short = eval_result.get("build_key_short")
                build_duration = float(eval_result.get("build_duration_seconds") or 0.0)
            else:
                start_time = time.time()
                async with state.rollout_semaphore:
                    build_result = await ensure_algo_bench_binary(
                        state,
                        candidate_code=candidate_code,
                        ai_name=ai_name,
                        build_timeout_seconds=build_timeout_i,
                    )
                    build_cache_hit = bool(build_result.get("build_cache_hit"))
                    build_key_short = str(build_result.get("build_key") or "")[:12]
                    build_duration = float(build_result.get("duration_seconds") or 0.0)

                    if build_result.get("success"):
                        eval_result = await run_algo_bench_candidate_eval(
                            str(build_result["binary_path"]),
                            workspace_dir=str(build_result["workspace_dir"]),
                            matches=matches_i,
                            seed_base=seed_base,
                            timeout_seconds=timeout_i,
                        )
                    else:
                        eval_result = {
                            "returncode": int(build_result.get("returncode") or 1),
                            "reward": 0.0,
                            "overall_win_rate": None,
                            "wins": None,
                            "matches": None,
                            "timed_out": bool(build_result.get("timed_out")),
                            "timeout_seconds": int(build_timeout_i),
                            "stdout": build_result.get("stdout") or "",
                            "stderr": build_result.get("stderr") or "",
                            "stage": "build",
                        }
                    eval_result["build_cache_hit"] = build_cache_hit
                    eval_result["build_key_short"] = build_key_short
                    eval_result["build_duration_seconds"] = build_duration
                duration = time.time() - start_time
                cache_hit = False
                async with state.algo_bench_cache_lock:
                    state.algo_bench_cache[cache_key] = dict(eval_result)

            details = {
                "optimization_mode": "optimize_anything",
                "artifact_kind": policy_config.get("artifact_kind"),
                "algo_bench_matches": matches_i,
                "algo_bench_seed_base": seed_base,
                "algo_bench_timeout_seconds": timeout_i,
                "algo_bench_build_timeout_seconds": build_timeout_i,
                "cache_hit": cache_hit,
                "cache_key": cache_key[:12],
                "build_cache_hit": build_cache_hit,
                "build_key": build_key_short,
                "build_duration_seconds": build_duration,
                "benchmark_returncode": eval_result.get("returncode"),
                "timed_out": bool(eval_result.get("timed_out")),
                "benchmark_stage": eval_result.get("stage") or "run",
                "overall_win_rate": eval_result.get("overall_win_rate"),
                "wins": eval_result.get("wins"),
                "matches": eval_result.get("matches"),
                "benchmark_metrics": eval_result.get("benchmark_metrics"),
                "seed": seed,
                "instance_id": instance_id,
                "duration_seconds": duration,
                "stdout": (eval_result.get("stdout") or "")[:5000],
                "stderr": (eval_result.get("stderr") or "")[:5000],
            }

            trace_payload = {
                "schema_version": "3.0",
                "event_history": [
                    {
                        "type": "task",
                        "observation": f"Algo-bench optimize_anything rollout seed={seed}",
                        "metadata": {
                            "environment": "algo_bench",
                            "seed": seed,
                            "cache_hit": cache_hit,
                            "trace_correlation_id": trace_correlation_id,
                            "inference_url": inference_url,
                        },
                    },
                    {
                        "type": "evaluation",
                        "observation": (
                            f"reward={eval_result.get('reward', 0.0):.4f} "
                            f"overall_win_rate={eval_result.get('overall_win_rate')} "
                            f"wins={eval_result.get('wins')}/{eval_result.get('matches')}"
                        ),
                        "metadata": details,
                    },
                ],
                "metadata": {
                    "optimization_mode": "optimize_anything",
                    "artifact_kind": policy_config.get("artifact_kind"),
                    "seed": seed,
                    "trace_correlation_id": trace_correlation_id,
                    "inference_url": inference_url,
                },
            }

            return RolloutResponse(
                trace_correlation_id=trace_correlation_id,
                reward_info=RolloutMetrics(
                    outcome_reward=float(eval_result.get("reward", 0.0)),
                    event_rewards=[float(eval_result.get("reward", 0.0))],
                    details=details,
                ),
                inference_url=inference_url if isinstance(inference_url, str) else None,
                trace=trace_payload,
            )

        if not api_key:
            return _error_response(
                request.run_id,
                seed,
                instance_id,
                "Missing OPENAI_API_KEY",
                trace_correlation_id=trace_correlation_id,
                inference_url=inference_url if isinstance(inference_url, str) else None,
            )

        start_time = time.time()

        async with state.rollout_semaphore:
            with tempfile.TemporaryDirectory() as tmpdir:
                work_dir = Path(tmpdir)

                try:
                    # 1. Load instance
                    instance = load_instance(instance_id)

                    # 2. Set up sandbox
                    sandbox_dir = await setup_sandbox(instance_id, work_dir)

                    # 3. Build prompt and run agent in container
                    prompt = _build_prompt(instance, loop_limit)
                    base_url = _policy_value(policy_config, "base_url") or _policy_value(
                        policy_config, "inference_url"
                    )
                    # Echo back the effective inference URL used by the agent for trace hydration.
                    effective_inference_url = (
                        base_url
                        if isinstance(base_url, str) and base_url.strip()
                        else "https://api.openai.com/v1"
                    )

                    agent_result = await run_agent(
                        prompt,
                        sandbox_dir,
                        model,
                        timeout,
                        api_key,
                        base_url,
                    )

                    # 4. Evaluate
                    compile_pass, compile_error = await run_cargo_build(sandbox_dir)

                    if compile_pass:
                        tests_passed, tests_total, test_output = await run_cargo_test(
                            sandbox_dir, instance_id
                        )
                    else:
                        tests_passed, tests_total, test_output = 0, 0, ""

                    # 5. Get patch and calculate similarity
                    patch = get_git_diff(sandbox_dir)
                    gold_similarity = calculate_gold_similarity(patch, instance_id)

                    # 6. Calculate score
                    score = calculate_score(
                        compile_pass, tests_passed, tests_total, gold_similarity
                    )

                    duration = time.time() - start_time

                    # Minimal v3 trace payload required by the verifier pipeline.
                    # This is not a full LLM trace; the backend can hydrate full traces via correlation IDs
                    # when an interceptor inference_url is used.
                    agent_stdout = (agent_result.get("stdout", "") or "")[:5000]
                    agent_stderr = (agent_result.get("stderr", "") or "")[:5000]
                    # Provide minimal IO examples for GEPA proposers/extractors.
                    # EngineBench rollouts are agentic (no llm_request/llm_response events), so we
                    # populate markov_blanket_message_history with observation/action messages.
                    observation_text = (prompt or "")[:10000]
                    patch_excerpt = (patch or "")[:8000]
                    action_text = (
                        f"compile_pass={compile_pass}\n"
                        f"tests={tests_passed}/{tests_total}\n"
                        f"gold_similarity={gold_similarity:.3f}\n"
                        f"score={score:.3f}\n"
                        f"\n"
                        f"patch:\n{patch_excerpt}\n"
                    )[:12000]
                    trace_payload = {
                        "schema_version": "3.0",
                        "event_history": [
                            {
                                "type": "task",
                                "observation": f"EngineBench rollout seed={seed} instance_id={instance_id}",
                                "metadata": {
                                    "environment": "engine_bench",
                                    "instance_id": instance_id,
                                    "seed": seed,
                                    "trace_correlation_id": trace_correlation_id,
                                    "inference_url": effective_inference_url,
                                },
                            },
                            {
                                "type": "agent_run",
                                "observation": (
                                    "OpenCode agent executed inside a sandboxed repo; "
                                    "stdout/stderr are attached for debugging."
                                ),
                                "metadata": {
                                    "agent": "opencode",
                                    "backend": CONTAINER_BACKEND,
                                    "model": model,
                                    "timeout_s": timeout,
                                    "loop_limit": loop_limit,
                                    "success": bool(agent_result.get("success")),
                                },
                                "stdout": agent_stdout,
                                "stderr": agent_stderr,
                            },
                            {
                                "type": "evaluation",
                                "observation": (
                                    f"compile_pass={compile_pass} "
                                    f"tests={tests_passed}/{tests_total} "
                                    f"gold_similarity={gold_similarity:.3f} "
                                    f"score={score:.3f}"
                                ),
                                "metadata": {
                                    "compile_pass": compile_pass,
                                    "tests_passed": tests_passed,
                                    "tests_total": tests_total,
                                    "gold_similarity": gold_similarity,
                                    "score": score,
                                    "duration_seconds": duration,
                                },
                            },
                        ],
                        "markov_blanket_message_history": [
                            {
                                "message_type": "observation",
                                "content": {"text": observation_text},
                                "metadata": {
                                    "from_system_role": "environment",
                                    "to_system_role": "agent",
                                    "environment": "engine_bench",
                                    "seed": seed,
                                    "instance_id": instance_id,
                                },
                            },
                            {
                                "message_type": "action",
                                "content": {"text": action_text},
                                "metadata": {
                                    "from_system_role": "agent",
                                    "to_system_role": "environment",
                                    "environment": "engine_bench",
                                    "seed": seed,
                                    "instance_id": instance_id,
                                },
                            },
                        ],
                        "metadata": {
                            "instance_id": instance_id,
                            "trace_correlation_id": trace_correlation_id,
                            "inference_url": effective_inference_url,
                            "duration_seconds": duration,
                        },
                    }

                    return RolloutResponse(
                        trace_correlation_id=trace_correlation_id,
                        reward_info=RolloutMetrics(
                            outcome_reward=score,
                            event_rewards=[score],
                            details={
                                "compile_pass": compile_pass,
                                "tests_passed": tests_passed,
                                "tests_total": tests_total,
                                "gold_similarity": gold_similarity,
                                "seed": seed,
                                "instance_id": instance_id,
                                "patch": (patch or "")[:10000] or None,
                                "compile_error": (compile_error or "")[:2000] or None,
                                "test_output": (test_output or "")[:2000] or None,
                            },
                        ),
                        inference_url=effective_inference_url,
                        trace=trace_payload,
                    )

                except Exception as exc:
                    return _error_response(
                        request.run_id,
                        seed,
                        instance_id,
                        str(exc),
                        trace_correlation_id=trace_correlation_id,
                        inference_url=inference_url if isinstance(inference_url, str) else None,
                    )

    return app


def _error_response(
    run_id: str,
    seed: int,
    instance_id: str | None,
    error: str,
    *,
    trace_correlation_id: str | None = None,
    inference_url: str | None = None,
) -> RolloutResponse:
    """Create an error response."""
    tcid = trace_correlation_id or secrets.token_hex(6)
    trace_payload = {
        "schema_version": "3.0",
        "event_history": [
            {
                "type": "error",
                "observation": f"EngineBench rollout failed: {error}",
                "metadata": {
                    "error": error,
                    "seed": seed,
                    "instance_id": instance_id,
                    "trace_correlation_id": tcid,
                    "inference_url": inference_url,
                },
            }
        ],
        "markov_blanket_message_history": [
            {
                "message_type": "observation",
                "content": {"text": f"EngineBench rollout error seed={seed} instance_id={instance_id}"},
                "metadata": {
                    "from_system_role": "environment",
                    "to_system_role": "agent",
                    "environment": "engine_bench",
                    "seed": seed,
                    "instance_id": instance_id,
                },
            },
            {
                "message_type": "action",
                "content": {"text": (error or "")[:4000]},
                "metadata": {
                    "from_system_role": "agent",
                    "to_system_role": "environment",
                    "environment": "engine_bench",
                    "seed": seed,
                    "instance_id": instance_id,
                },
            },
        ],
        "metadata": {
            "error": error,
            "seed": seed,
            "instance_id": instance_id,
            "trace_correlation_id": tcid,
            "inference_url": inference_url,
        },
    }

    return RolloutResponse(
        trace_correlation_id=tcid,
        reward_info=RolloutMetrics(
            outcome_reward=0.0,
            details={"error": error, "seed": seed, "instance_id": instance_id},
        ),
        status_detail=error,
        inference_url=inference_url,
        trace=trace_payload,
    )


# Create the app
app = create_container()


if __name__ == "__main__":
    import argparse
    import uvicorn

    parser = argparse.ArgumentParser(description="EngineBench Container")
    parser.add_argument("--port", type=int, default=8017, help="Port to run on")
    parser.add_argument("--host", type=str, default="0.0.0.0", help="Host to bind")
    args = parser.parse_args()

    uvicorn.run(app, host=args.host, port=args.port)
