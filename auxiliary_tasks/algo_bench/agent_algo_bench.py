#!/usr/bin/env python3
"""Agent benchmark for algo_bench.

This drops an agent into a sandbox repo with the overzealous crates,
asks it to write a .txt file with a better algo, then benchmarks it
against v1-v4 (reference_algos + v4 in tcg_ai) using benchmark_ai.py.
"""

import argparse
import concurrent.futures
import json
import os
import shutil
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Any, Optional


BASE_DIR = Path(__file__).parent
ROOT_DIR = BASE_DIR.parent.parent
OVERZEALOUS_DIR = Path("/Users/joshpurtell/Documents/GitHub/overzealous")
ALGO_BENCH_DATA_DIR = BASE_DIR / "data"
BENCHMARK_SCRIPT = BASE_DIR / "benchmark_ai.py"
SUBMISSION_FILE = "algo_submission.txt"


ALGORITHM_TEMPLATE = '''
use rand::seq::SliceRandom;
use rand_chacha::ChaCha8Rng;
use rand::SeedableRng;

use tcg_core::{{Action, Attack, CardInstanceId, GameView, Prompt}};
use tcg_ai::traits::AiController;

/// {ai_name} - Pokemon TCG AI
/// 
/// Key game concepts available via GameView:
/// - view.my_active: Your active Pokemon (Option<PokemonInPlay>)
/// - view.my_bench: Your bench Pokemon (Vec<PokemonInPlay>)
/// - view.opponent_active: Opponent's active Pokemon
/// - view.opponent_bench: Opponent's bench
/// - view.action_hints: Available actions (playable_basic_ids, playable_energy_ids, 
///   attach_targets, usable_attacks, can_declare_attack, can_retreat, etc.)
/// - view.my_hand: Cards in your hand
/// - view.my_prizes_count / view.opponent_prizes_count: Prize counts remaining
/// - view.current_player / view.player_id: Turn tracking
///
/// PokemonView (my_active, my_bench items, opponent_active, opponent_bench items) has:
/// - card: CardInstance (id, name, card_type, etc.)
/// - hp: Current max HP (u16)
/// - damage_counters: Current damage (10 damage = 1 counter, u16)
/// - attached_energy: Vec<CardInstance> of energy cards
/// - types: Vec<Type> - Pokemon types
/// - special_conditions: Vec of status conditions
/// - weakness/resistance: Type matchup modifiers
pub struct {ai_name} {{
    rng: ChaCha8Rng,
}}

impl {ai_name} {{
    pub fn new(seed: u64) -> Self {{
        Self {{
            rng: ChaCha8Rng::seed_from_u64(seed),
        }}
    }}
    
    /// Pick best attack by damage, preferring lower energy cost as tiebreaker
    fn best_attack(attacks: &[Attack]) -> Option<Attack> {{
        attacks.iter().cloned().max_by(|a, b| {{
            let dmg = a.damage.cmp(&b.damage);
            if dmg != std::cmp::Ordering::Equal {{
                return dmg;
            }}
            a.cost.total_energy.cmp(&b.cost.total_energy).reverse()
        }})
    }}
}}

impl AiController for {ai_name} {{
    fn propose_prompt_response(&mut self, view: &GameView, prompt: &Prompt) -> Vec<Action> {{
        let mut actions: Vec<Action> = Vec::new();

        match prompt {{
            Prompt::ChooseStartingActive {{ options }} => {{
                // TODO: Pick the best starter (consider HP, retreat cost, attack potential)
                if let Some(&card_id) = options.first() {{
                    actions.push(Action::ChooseActive {{ card_id }});
                }}
            }}
            Prompt::ChooseBenchBasics {{ options, min, max }} => {{
                // TODO: Strategically choose which basics to bench
                let count = (*min).max(1).min(*max).min(options.len());
                let picked: Vec<CardInstanceId> = options.iter().take(count).copied().collect();
                actions.push(Action::ChooseBench {{ card_ids: picked }});
            }}
            Prompt::ChooseAttack {{ attacks }} => {{
                // Pick highest damage attack
                if let Some(best) = Self::best_attack(attacks) {{
                    actions.push(Action::DeclareAttack {{ attack: best }});
                }}
                // Fallback: try all attacks
                for attack in attacks {{
                    actions.push(Action::DeclareAttack {{ attack: attack.clone() }});
                }}
            }}
            Prompt::ChooseNewActive {{ player, options }} => {{
                if *player != view.player_id {{
                    return vec![Action::EndTurn];
                }}
                // TODO: Pick best replacement (consider HP, energy, matchup)
                let candidates: Vec<CardInstanceId> = if options.is_empty() {{
                    view.my_bench.iter().map(|p| p.card.id).collect()
                }} else {{
                    options.clone()
                }};
                if let Some(&card_id) = candidates.first() {{
                    actions.push(Action::ChooseNewActive {{ card_id }});
                }}
            }}
            _ => {{
                // Handle other prompts with EndTurn fallback
            }}
        }}

        actions
    }}

    fn propose_free_actions(&mut self, view: &GameView) -> Vec<Action> {{
        if view.current_player != view.player_id {{
            return Vec::new();
        }}
        if view.pending_prompt.is_some() {{
            return Vec::new();
        }}

        let mut actions: Vec<Action> = Vec::new();
        let hints = &view.action_hints;

        // TODO: Improve energy attachment strategy
        // - Prioritize Pokemon that can attack this turn
        // - Consider evolution targets
        if let Some(&energy_id) = hints.playable_energy_ids.first() {{
            if let Some(&target_id) = hints.attach_targets.first() {{
                actions.push(Action::AttachEnergy {{ energy_id, target_id }});
            }}
        }}

        // Attack with best available attack
        if let Some(best) = Self::best_attack(&hints.usable_attacks) {{
            actions.push(Action::DeclareAttack {{ attack: best }});
        }}

        // TODO: Improve bench strategy
        // - Set up evolution lines
        // - Maintain backup attackers
        if let Some(&card_id) = hints.playable_basic_ids.first() {{
            actions.push(Action::PlayBasic {{ card_id }});
        }}

        // TODO: Consider retreat when advantageous
        // if hints.can_retreat {{ ... }}

        actions.push(Action::EndTurn);
        actions
    }}
}}
'''

PROMPT_TEMPLATE = """Create a stronger Pokemon TCG AI.

Write Rust code ONLY to: {submission_path}

Rules for a clean submission:
- No markdown, no explanations, no extra braces
- End file with the final `}}` only (no trailing `}};`)
- Use only valid Rust code
- Use `PokemonView` (not `PokemonInPlay`)

Starter template:
{algorithm_template}

Requirements:
- Struct name: {ai_name}
- Has `pub fn new(seed: u64) -> Self`
- Implements `AiController`
- If you cannot write the file, output ONLY the Rust code to stdout
"""

PROMPT_TEMPLATE_STRICT = """Output only valid Rust code for {ai_name}.

Write it to: {submission_path}

Sanitized submission rules:
- No markdown or prose
- No trailing `}};` (end with `}}` only)
- No extra wrapper code
- Use `PokemonView` (not `PokemonInPlay`)
- If you cannot write the file, output ONLY the Rust code to stdout

Template:
{algorithm_template}
"""


def run_harness(
    prompt: str,
    sandbox_dir: Path,
    model: str,
    timeout: int,
    env: dict[str, str],
    harness: str,
) -> dict[str, Any]:
    """Run agent via the selected harness."""
    if harness == "codex":
        # Use `codex exec` for non-interactive execution with JSON output
        # Configure codex to use OpenAI API directly with the provided API key
        # Strip provider prefix if present (e.g., "openai/gpt-5-codex" -> "gpt-5-codex")
        codex_model = model.split("/")[-1] if "/" in model else model
        cmd = [
            "codex",
            "exec",
            "--json",
            "--model",
            codex_model,
            "-c",
            "model_provider=openai",
            "-c",
            'model_providers.openai.base_url="https://api.openai.com/v1"',
            "-c",
            'model_providers.openai.env_key="OPENAI_API_KEY"',
            "-c",
            "model_providers.openai.requires_openai_auth=false",
            "-c",
            'model_reasoning_effort="medium"',
            "--sandbox",
            "workspace-write",
            "--skip-git-repo-check",
            prompt,
        ]
        try:
            proc = subprocess.Popen(
                cmd,
                cwd=str(sandbox_dir),
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                env=env,
            )
            stdout, stderr = proc.communicate(timeout=timeout)
            return {
                "success": proc.returncode == 0,
                "stdout": stdout,
                "stderr": stderr,
                "command": " ".join(cmd),
            }
        except subprocess.TimeoutExpired:
            proc.kill()
            stdout, stderr = proc.communicate()
            return {
                "success": False,
                "stdout": stdout or "",
                "stderr": f"Timeout after {timeout} seconds",
                "command": " ".join(cmd),
            }
        except Exception as exc:
            return {
                "success": False,
                "stdout": "",
                "stderr": str(exc),
                "command": " ".join(cmd),
            }
    elif harness == "cursor":
        # Cursor agent CLI with --print for headless mode
        # Model names include reasoning effort: gpt-5.2-codex (medium), gpt-5.2-codex-high, etc.
        # Map common model names to cursor equivalents
        cursor_model = model.split("/")[-1] if "/" in model else model
        # Map gpt-5-codex -> gpt-5.2-codex for cursor
        if cursor_model == "gpt-5-codex":
            cursor_model = "gpt-5.2-codex"  # medium reasoning by default
        cmd = [
            "cursor",
            "agent",
            "--print",
            "--output-format",
            "json",
            "--model",
            cursor_model,
            "--force",
            "--workspace",
            str(sandbox_dir),
            prompt,
        ]
        try:
            proc = subprocess.Popen(
                cmd,
                cwd=str(sandbox_dir),
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                env=env,
            )
            stdout, stderr = proc.communicate(timeout=timeout)
            return {
                "success": proc.returncode == 0,
                "stdout": stdout,
                "stderr": stderr,
                "command": " ".join(cmd),
            }
        except subprocess.TimeoutExpired:
            proc.kill()
            stdout, stderr = proc.communicate()
            return {
                "success": False,
                "stdout": stdout or "",
                "stderr": f"Timeout after {timeout} seconds",
                "command": " ".join(cmd),
            }
        except Exception as exc:
            return {
                "success": False,
                "stdout": "",
                "stderr": str(exc),
                "command": " ".join(cmd),
            }
    else:
        # Default: opencode harness
        cmd = [
            "opencode",
            "run",
            "--model",
            model,
            "--agent",
            "build",
            "--format",
            "json",
            prompt,
        ]
        try:
            proc = subprocess.Popen(
                cmd,
                cwd=str(sandbox_dir),
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                env=env,
            )
            stdout, stderr = proc.communicate(timeout=timeout)
            return {
                "success": proc.returncode == 0,
                "stdout": stdout,
                "stderr": stderr,
                "command": " ".join(cmd),
            }
        except subprocess.TimeoutExpired:
            proc.kill()
            stdout, stderr = proc.communicate()
            return {
                "success": False,
                "stdout": stdout or "",
                "stderr": f"Timeout after {timeout} seconds",
                "command": " ".join(cmd),
            }
        except Exception as exc:
            return {
                "success": False,
                "stdout": "",
                "stderr": str(exc),
                "command": " ".join(cmd),
            }



def copy_overzealous_repo(dest_dir: Path) -> None:
    """Copy overzealous repo into sandbox."""
    ignore = shutil.ignore_patterns(".git", "target", "node_modules", ".venv", "dist")
    shutil.copytree(OVERZEALOUS_DIR, dest_dir, dirs_exist_ok=True, ignore=ignore)


def parse_overall_rate(output: str) -> Optional[float]:
    """Parse Overall win rate from benchmark output."""
    for line in output.splitlines():
        if line.strip().startswith("Overall:"):
            # Overall: X wins / Y matches (ZZ.Z%)
            if "(" in line and "%" in line:
                try:
                    pct = line.split("(")[-1].split("%")[0].strip()
                    return float(pct)
                except ValueError:
                    return None
    return None


def extract_code_from_agent_output(stdout: str, assume_json: bool = True) -> Optional[str]:
    """Extract AI code from agent stdout (JSON events or plain text)."""
    if assume_json:
        code_parts = []
        for line in stdout.splitlines():
            if not line.strip():
                continue
            try:
                event = json.loads(line)
                if event.get("type") == "text":
                    part = event.get("part", {})
                    text = part.get("text", "")
                    # Check if this looks like AI code
                    if "AiController" in text or "pub struct" in text:
                        code_parts.append(text)
            except json.JSONDecodeError:
                continue
        if code_parts:
            # Return the longest code block (most likely the complete implementation)
            return max(code_parts, key=len)
    return extract_code_from_plain_output(stdout)


def extract_code_from_plain_output(stdout: str) -> Optional[str]:
    """Extract Rust code from plain-text stdout."""
    if "```" in stdout:
        blocks = []
        in_block = False
        current: list[str] = []
        for line in stdout.splitlines():
            if line.strip().startswith("```"):
                if in_block:
                    blocks.append("\n".join(current).strip())
                    current = []
                    in_block = False
                else:
                    in_block = True
                continue
            if in_block:
                current.append(line)
        if blocks:
            blocks = [b for b in blocks if "AiController" in b or "pub struct" in b]
            return max(blocks, key=len) if blocks else None

    if "pub struct" in stdout and "AiController" in stdout:
        lines = stdout.splitlines()
        start = next((i for i, line in enumerate(lines) if "pub struct" in line), None)
        if start is not None:
            candidate = "\n".join(lines[start:]).strip()
            return candidate if candidate else None
    return None


def load_env_file(path: Path) -> dict[str, str]:
    """Load simple KEY=VALUE pairs from a .env file."""
    if not path.exists():
        return {}
    env_vars: dict[str, str] = {}
    for raw_line in path.read_text().splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            # Allow raw API key lines (e.g., "sk-...") as a convenience
            if line.startswith("sk-"):
                env_vars["OPENAI_API_KEY"] = line
            continue
        key, value = line.split("=", 1)
        env_vars[key.strip()] = value.strip().strip('"').strip("'")
    return env_vars


def extract_agent_usage(stdout: str) -> dict[str, Any]:
    """Extract cost/tokens info from opencode JSON output if available."""
    usage: dict[str, Any] = {}
    for line in stdout.splitlines():
        if not line.strip():
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if event.get("type") == "step_finish":
            part = event.get("part", {})
            if "cost" in part:
                usage["cost"] = part.get("cost")
            if "tokens" in part:
                usage["tokens"] = part.get("tokens")
    return usage


def extract_agent_model_info(stdout: str) -> dict[str, Any]:
    """Extract model info (e.g., reasoning effort) from opencode JSON output."""
    info: dict[str, Any] = {}
    for line in stdout.splitlines():
        if not line.strip():
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if "model" in event and "model" not in info:
            info["model"] = event.get("model")
        if "provider" in event and "provider" not in info:
            info["provider"] = event.get("provider")
        if "reasoning_effort" in event and "reasoning_effort" not in info:
            info["reasoning_effort"] = event.get("reasoning_effort")
        part = event.get("part", {})
        if isinstance(part, dict):
            if "model" in part and "model" not in info:
                info["model"] = part.get("model")
            if "provider" in part and "provider" not in info:
                info["provider"] = part.get("provider")
            if "reasoning_effort" in part and "reasoning_effort" not in info:
                info["reasoning_effort"] = part.get("reasoning_effort")
    return info


def sanitize_submission(code: str) -> str:
    """Best-effort cleanup for common agent output mistakes."""
    lines = code.splitlines()
    # Drop trailing unmatched closing delimiter often emitted as a single '};'
    i = len(lines) - 1
    while i >= 0 and not lines[i].strip():
        i -= 1
    if i >= 0 and lines[i].strip() == "};":
        lines = lines[:i] + lines[i + 1 :]
    return "\n".join(lines)


def run_instance(run_index: int, total_runs: int, args: argparse.Namespace, merged_env: dict[str, str]) -> dict[str, Any]:
    run_results: dict[str, Any] = {
        "run": run_index + 1,
        "success": False,
        "agent": {"model": args.model, "timeout": args.timeout},
        "benchmark": {
            "matches": args.matches,
            "server_db": args.server_db,
            "cards_db": args.cards_db,
        },
            "harness": {
                "name": args.harness,
                "command": None,
                "format": "json" if args.harness in ("opencode", "cursor") else "text",
            },
    }
    with tempfile.TemporaryDirectory() as tmpdir:
        sandbox_dir = Path(tmpdir) / "sandbox"
        copy_overzealous_repo(sandbox_dir)

        submission_path = sandbox_dir / SUBMISSION_FILE
        algorithm_template = ALGORITHM_TEMPLATE.format(ai_name=args.ai_name)
        prompt = PROMPT_TEMPLATE.format(
            submission_path=SUBMISSION_FILE,
            ai_name=args.ai_name,
            algorithm_template=algorithm_template,
        )
        run_results["prompt"] = prompt

        if args.parallel <= 1:
            print(f"Running agent ({run_index + 1}/{total_runs})...")
        start = time.time()
        agent_result = run_harness(
            prompt,
            sandbox_dir,
            args.model,
            args.timeout,
            merged_env,
            args.harness,
        )
        duration = time.time() - start
        run_results["agent"]["duration_seconds"] = duration
        run_results["agent"]["stdout"] = agent_result["stdout"]
        run_results["agent"]["stderr"] = agent_result["stderr"]
        run_results["agent"]["success"] = agent_result["success"]
        run_results["agent"]["usage"] = extract_agent_usage(agent_result["stdout"])
        run_results["agent"]["model_info"] = extract_agent_model_info(agent_result["stdout"])
        run_results["harness"]["command"] = agent_result.get("command")

        ai_code = None
        if submission_path.exists():
            ai_code = sanitize_submission(submission_path.read_text())
            run_results["code_source"] = "file"
        else:
            # Try to extract code from agent's output
            ai_code = extract_code_from_agent_output(
                agent_result["stdout"],
                assume_json=args.harness == "opencode",
            )
            if ai_code:
                ai_code = sanitize_submission(ai_code)
                run_results["code_source"] = "extracted_from_output"
                # Save it for reference
                submission_path.write_text(ai_code)
            else:
                # Retry once with a stricter prompt that forces code output
                strict_prompt = PROMPT_TEMPLATE_STRICT.format(
                    submission_path=SUBMISSION_FILE,
                    ai_name=args.ai_name,
                    algorithm_template=algorithm_template,
                )
                run_results["agent_retry"] = {"prompt": strict_prompt}
                retry_result = run_harness(
                    strict_prompt,
                    sandbox_dir,
                    args.model,
                    args.timeout,
                    merged_env,
                    args.harness,
                )
                run_results["agent_retry"]["stdout"] = retry_result["stdout"]
                run_results["agent_retry"]["stderr"] = retry_result["stderr"]
                run_results["agent_retry"]["success"] = retry_result["success"]
                run_results["agent_retry"]["usage"] = extract_agent_usage(retry_result["stdout"])

                if submission_path.exists():
                    ai_code = sanitize_submission(submission_path.read_text())
                    run_results["code_source"] = "file"
                else:
                    ai_code = extract_code_from_agent_output(
                        retry_result["stdout"],
                        assume_json=args.harness == "opencode",
                    )
                    if ai_code:
                        ai_code = sanitize_submission(ai_code)
                        run_results["code_source"] = "extracted_from_output"
                        submission_path.write_text(ai_code)
                    else:
                        run_results["error"] = (
                            "No code found after retry: submission file not created and no code in agent output"
                        )

        if ai_code:
            # Persist sanitized code if it came from a file
            submission_path.write_text(ai_code)
            run_results["submission_chars"] = len(ai_code)
            run_results["submission"] = {
                "path": str(submission_path),
                "code": ai_code,
            }
            run_results["sandbox"] = {"path": str(sandbox_dir)}

            if args.parallel <= 1:
                print(f"Running benchmark ({run_index + 1}/{total_runs})...")
            bench_cmd = [
                "python3",
                str(BENCHMARK_SCRIPT),
                "--ai-code-file",
                str(submission_path),
                "--name",
                args.ai_name,
                "--matches",
                str(args.matches),
                "--server-db",
                args.server_db,
                "--cards-db",
                args.cards_db,
            ]
            bench_proc = subprocess.run(
                bench_cmd,
                cwd=str(BASE_DIR),
                capture_output=True,
                text=True,
            )
            run_results["benchmark"]["stdout"] = bench_proc.stdout
            run_results["benchmark"]["stderr"] = bench_proc.stderr
            run_results["benchmark"]["returncode"] = bench_proc.returncode
            run_results["benchmark"]["overall_win_rate"] = parse_overall_rate(bench_proc.stdout)

            if bench_proc.returncode != 0:
                run_results["error"] = "SubmissionFailedCompilation"
            run_results["success"] = bench_proc.returncode == 0
        else:
            run_results["sandbox"] = {"path": str(sandbox_dir)}

    return run_results


def main() -> None:
    parser = argparse.ArgumentParser(description="Agent benchmark for algo_bench")
    parser.add_argument("--model", type=str, default="gpt-4.1-mini", help="Model for agent")
    parser.add_argument(
        "--harness",
        type=str,
        default="opencode",
        choices=["opencode", "codex", "cursor"],
        help="Agent harness to use",
    )
    parser.add_argument("--timeout", type=int, default=1800, help="Agent timeout in seconds")
    parser.add_argument("--matches", type=int, default=50, help="Matches per opponent")
    parser.add_argument("--ai-name", type=str, default="CandidateAi", help="Struct name for the AI")
    parser.add_argument(
        "--instances",
        type=int,
        default=1,
        help="Number of agent runs to execute",
    )
    parser.add_argument(
        "--parallel",
        type=int,
        default=1,
        help="Number of instances to run in parallel",
    )
    default_server = str(ALGO_BENCH_DATA_DIR / "server.sqlite") if (ALGO_BENCH_DATA_DIR / "server.sqlite").exists() else str(OVERZEALOUS_DIR / "data/server.sqlite")
    default_cards = str(ALGO_BENCH_DATA_DIR / "cards.sqlite") if (ALGO_BENCH_DATA_DIR / "cards.sqlite").exists() else str(OVERZEALOUS_DIR / "data/cards.sqlite")
    parser.add_argument("--server-db", type=str, default=default_server)
    parser.add_argument("--cards-db", type=str, default=default_cards)
    parser.add_argument("--env-file", type=Path, help="Path to .env file for API keys")
    parser.add_argument(
        "--output",
        type=str,
        default="results.json",
        help="Write JSON results to file (default: results.json)",
    )
    args = parser.parse_args()

    if args.env_file:
        env_candidates = [args.env_file]
        env_file = args.env_file
        env_source = "explicit"
    else:
        env_candidates = [
            ROOT_DIR / ".env",
            BASE_DIR / ".env",
            Path.cwd() / ".env",
            OVERZEALOUS_DIR / ".env",
            Path.home() / ".env",
        ]
        env_file = next((path for path in env_candidates if path.exists()), env_candidates[0])
        env_source = "auto"
    env_vars = load_env_file(env_file)
    merged_env = os.environ.copy()
    merged_env.update(env_vars)
    removed_env_keys: list[str] = []
    if args.harness == "opencode" and args.model.startswith("openai/"):
        # Ensure opencode doesn't prefer its own hosted provider credentials
        for key in ("OPENCODE_API_KEY", "OPENCODE_TOKEN", "OPENCODE_AUTH_TOKEN"):
            if key in merged_env:
                removed_env_keys.append(key)
                merged_env.pop(key, None)

    results: dict[str, Any] = {
        "agent": {"model": args.model, "timeout": args.timeout},
        "benchmark": {
            "matches": args.matches,
            "server_db": args.server_db,
            "cards_db": args.cards_db,
        },
        "harness": {
            "name": args.harness,
            "format": "json" if args.harness in ("opencode", "cursor") else "text",
        },
        "env": {
            "path": str(env_file),
            "loaded": bool(env_vars),
            "keys": sorted(env_vars.keys()),
            "candidates": [str(path) for path in env_candidates],
            "source": env_source,
            "removed_keys": removed_env_keys,
        },
        "runs": [],
        "summary": {},
    }

    if args.parallel < 1:
        args.parallel = 1
    runs: list[dict[str, Any]] = []
    if args.parallel == 1:
        for run_index in range(args.instances):
            runs.append(run_instance(run_index, args.instances, args, merged_env))
    else:
        print(f"Running {args.instances} instances with {args.parallel} workers...")
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.parallel) as executor:
            future_map = {
                executor.submit(run_instance, run_index, args.instances, args, merged_env): run_index
                for run_index in range(args.instances)
            }
            for future in concurrent.futures.as_completed(future_map):
                runs.append(future.result())
        runs.sort(key=lambda item: item.get("run", 0))

    results["runs"] = runs

    success_runs = [run for run in runs if run.get("success")]

    def average(values: list[float]) -> Optional[float]:
        if not values:
            return None
        return sum(values) / len(values)

    avg_win = average(
        [
            run["benchmark"]["overall_win_rate"]
            for run in success_runs
            if run.get("benchmark", {}).get("overall_win_rate") is not None
        ]
    )
    avg_duration = average(
        [
            run["agent"]["duration_seconds"]
            for run in success_runs
            if run.get("agent", {}).get("duration_seconds") is not None
        ]
    )
    avg_cost = average(
        [
            run.get("agent", {}).get("usage", {}).get("cost")
            for run in success_runs
            if run.get("agent", {}).get("usage", {}).get("cost") is not None
        ]
    )

    results["summary"] = {
        "instances": args.instances,
        "parallel_workers": args.parallel,
        "successful_runs": len(success_runs),
        "average_overall_win_rate": avg_win,
        "average_duration_seconds": avg_duration,
        "average_cost": avg_cost,
    }
    results["success"] = len(success_runs) == len(runs) and len(runs) > 0

    if args.output:
        Path(args.output).write_text(json.dumps(results, indent=2))

    print("\nResults:")
    header = f"{'Run':>3} {'Success':>7} {'Win%':>6} {'Duration(s)':>12} {'Cost':>10} Error"
    print(header)
    print("-" * len(header))
    for run in runs:
        win = run.get("benchmark", {}).get("overall_win_rate")
        duration = run.get("agent", {}).get("duration_seconds")
        cost = run.get("agent", {}).get("usage", {}).get("cost")
        error = run.get("error", "")
        win_text = f"{win:.1f}" if isinstance(win, (int, float)) else "-"
        duration_text = f"{duration:.2f}" if isinstance(duration, (int, float)) else "-"
        cost_text = f"{cost:.6f}" if isinstance(cost, (int, float)) else "-"
        success_text = "yes" if run.get("success") else "no"
        print(
            f"{run.get('run', '-'):>3} {success_text:>7} {win_text:>6} {duration_text:>12} "
            f"{cost_text:>10} {error}"
        )

    if avg_win is not None:
        print(f"\nAverage win rate: {avg_win:.2f}")
    else:
        print("\nAverage win rate: -")
    if avg_duration is not None:
        print(f"Average agent duration (s): {avg_duration:.2f}")
    if avg_cost is not None:
        print(f"Average agent cost: {avg_cost:.6f}")

    print("Harness:", results.get("harness", {}).get("name"))
    print("Model:", results.get("agent", {}).get("model"))
    model_info = next(
        (
            run.get("agent", {}).get("model_info")
            for run in runs
            if isinstance(run.get("agent", {}).get("model_info"), dict)
        ),
        {},
    )
    if isinstance(model_info, dict):
        if "reasoning_effort" in model_info:
            print("Reasoning effort:", model_info.get("reasoning_effort"))
        if "provider" in model_info:
            print("Model provider:", model_info.get("provider"))
        if "model" in model_info and model_info.get("model") != results.get("agent", {}).get("model"):
            print("Resolved model:", model_info.get("model"))


if __name__ == "__main__":
    main()
