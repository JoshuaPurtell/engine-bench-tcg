#!/usr/bin/env python3
"""Evaluate models on HP (Holon Phantoms) cards.

Usage:
    python scripts/run_hp_eval.py --model openai/gpt-4.1-nano --instance hp-104-pikachu-star
    python scripts/run_hp_eval.py --model openai/gpt-4.1-nano --instances hp-104-pikachu-star,hp-017-vileplume
    python scripts/run_hp_eval.py --model openai/gpt-4.1-nano --count 10 --workers 4

This script:
1. Sets up a sandbox with the overzealous repo
2. Copies the stub file for the card
3. Runs the coding agent to implement the card
4. Injects eval tests and runs cargo test
5. Reports pass/fail based on actual test results
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime
from pathlib import Path

BASE_DIR = Path(__file__).parent.parent
DATA_DIR = BASE_DIR / "data"
GOLD_DIR = BASE_DIR / "gold"
OVERZEALOUS_REPO = Path.home() / "Documents" / "GitHub" / "overzealous"


def get_hp_instances(count: int = None) -> list[str]:
    """Get HP instance IDs."""
    instances_dir = DATA_DIR / "instances" / "single"
    instances = []
    for f in sorted(instances_dir.glob("hp-*.json")):
        instance_id = f.stem
        # Check if we have eval tests
        eval_file = GOLD_DIR / "tests" / f"{instance_id.replace('-', '_')}_eval.rs"
        if eval_file.exists():
            instances.append(instance_id)
    if count:
        return instances[:count]
    return instances


def load_instance(instance_id: str) -> dict:
    """Load instance specification."""
    instance_path = DATA_DIR / "instances" / "single" / f"{instance_id}.json"
    return json.loads(instance_path.read_text())


def build_prompt(instance: dict) -> str:
    """Build the prompt for the coding agent."""
    cards = instance.get("cards", [])
    card_file = instance.get("card_file", "")
    card_spec = json.dumps(cards[0], indent=2) if cards else "{}"
    
    tests = instance.get("tests", [])
    test_descriptions = "\n".join([f"- {t['name']}: {t['description']}" for t in tests])

    instance_id = instance.get("id", "").replace("-", "_")

    return f'''You are implementing a Pokemon TCG card for the Holon Phantoms expansion in Rust.

## Task
EDIT the file `{card_file}` to implement the card below. You MUST:
1. Actually WRITE code to the file - replace the TODO stubs with working implementations
2. Make sure it compiles without errors
3. Make sure all tests pass

## Card to Implement
### {cards[0].get("name", "Unknown") if cards else "Unknown"}
```json
{card_spec}
```

## File to Edit
`{card_file}` - This file contains stub functions with TODO comments. You must REPLACE the TODO implementations with actual working code.

## Tests to Pass
{test_descriptions}

## Reference
Look at existing card implementations in `tcg_expansions/src/df/cards/` (Dragon Frontiers) for patterns on how to implement attack modifiers and effects.

## CRITICAL Instructions
1. READ the stub file at `{card_file}`
2. READ similar card implementations in tcg_expansions/src/df/cards/ for patterns
3. EDIT `{card_file}` to replace the TODO stubs with working implementations
4. Run `cargo check --package tcg_expansions` to verify it compiles
5. Run `cargo test --package tcg_expansions -- {instance_id}` to verify tests pass

You MUST edit the file and write actual code. Do not just describe what to do - actually do it!
'''


def setup_sandbox(instance_id: str, work_dir: Path) -> Path:
    """Set up sandbox with overzealous repo."""
    sandbox_dir = work_dir / "overzealous"

    # Copy repo (excluding .git and target)
    shutil.copytree(
        OVERZEALOUS_REPO,
        sandbox_dir,
        symlinks=True,
        ignore=shutil.ignore_patterns(".git", "target", "*.pyc", "__pycache__"),
    )

    # Get card file path from instance
    instance = load_instance(instance_id)
    card_file_rel = instance.get("card_file", "")

    # Create HP directory structure
    hp_dir = sandbox_dir / "tcg_expansions" / "src" / "hp"
    hp_cards_dir = hp_dir / "cards"
    hp_cards_dir.mkdir(parents=True, exist_ok=True)

    # Get module name from instance_id (hp-001-armaldo -> hp_001_armaldo)
    module_name = instance_id.replace("-", "_")

    # Create hp/mod.rs
    hp_mod_rs = hp_dir / "mod.rs"
    hp_mod_rs.write_text(f"pub mod cards;\n")

    # Create hp/cards/mod.rs with the card module
    cards_mod_rs = hp_cards_dir / "mod.rs"
    cards_mod_rs.write_text(f"pub mod {module_name};\n")

    # Add hp module to lib.rs
    lib_rs = sandbox_dir / "tcg_expansions" / "src" / "lib.rs"
    lib_content = lib_rs.read_text()
    if "pub mod hp;" not in lib_content:
        lib_rs.write_text(lib_content + "\npub mod hp;\n")

    # Copy stub to sandbox
    stub_file = GOLD_DIR / "stubs" / f"{module_name}.rs"
    if stub_file.exists():
        card_path = sandbox_dir / card_file_rel
        card_path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(stub_file, card_path)

    return sandbox_dir


def run_opencode(prompt: str, sandbox_dir: Path, model: str, timeout: int = 180) -> dict:
    """Run OpenCode agent."""
    cmd = [
        "opencode", "run",
        "--model", model,
        "--agent", "build",
        "--format", "json",
        prompt,
    ]
    try:
        proc = subprocess.Popen(
            cmd,
            cwd=str(sandbox_dir),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        stdout, stderr = proc.communicate(timeout=timeout)
        return {"success": proc.returncode == 0, "stdout": stdout, "stderr": stderr}
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.communicate()
        return {"success": False, "stdout": "", "stderr": f"Timeout after {timeout}s"}
    except Exception as e:
        return {"success": False, "stdout": "", "stderr": str(e)}


def inject_eval_tests(sandbox_dir: Path, instance_id: str, card_file_rel: str) -> bool:
    """Inject evaluation tests."""
    eval_test_file = GOLD_DIR / "tests" / f"{instance_id.replace('-', '_')}_eval.rs"
    if not eval_test_file.exists():
        return False

    card_path = sandbox_dir / card_file_rel
    if not card_path.exists():
        return False

    eval_tests = eval_test_file.read_text()
    current_content = card_path.read_text()
    card_path.write_text(current_content + "\n\n" + eval_tests)
    return True


def run_cargo_test(sandbox_dir: Path, instance_id: str, card_file_rel: str = "", timeout: int = 60) -> dict:
    """Run cargo test and parse results."""
    # Build module path from card file (tcg_expansions/src/hp/cards/hp_001_armaldo.rs -> hp::cards::hp_001_armaldo)
    if card_file_rel:
        # Strip prefix and .rs suffix to get module path
        module_path = card_file_rel.replace("tcg_expansions/src/", "").replace(".rs", "").replace("/", "::")
    else:
        # Strict: assume HP card structure
        module_name = instance_id.replace("-", "_")
        module_path = f"hp::cards::{module_name}"

    # Run cargo test for the specific module's eval_tests
    test_filter = f"{module_path}::eval_tests"

    try:
        proc = subprocess.run(
            ["cargo", "test", "--package", "tcg_expansions", "--", test_filter, "--test-threads=1"],
            cwd=str(sandbox_dir),
            capture_output=True,
            text=True,
            timeout=timeout,
        )

        output = proc.stdout + proc.stderr

        # Parse test results from cargo output
        # Format: "test result: ok. X passed; Y failed; Z ignored"
        # Or: "test result: FAILED. X passed; Y failed; Z ignored"
        result = {
            "compiled": "error[E" not in output and "cannot find" not in output.lower(),
            "tests_passed": 0,
            "tests_failed": 0,
            "tests_total": 0,
            "all_passed": False,
            "output": output[-2000:] if len(output) > 2000 else output,  # Truncate
        }

        # Look for test result line
        match = re.search(r"test result: (\w+)\. (\d+) passed; (\d+) failed", output)
        if match:
            status = match.group(1)
            result["tests_passed"] = int(match.group(2))
            result["tests_failed"] = int(match.group(3))
            result["tests_total"] = result["tests_passed"] + result["tests_failed"]
            result["all_passed"] = status.lower() == "ok" and result["tests_failed"] == 0
        else:
            # Check for compilation errors
            if "error" in output.lower():
                result["compiled"] = False

        return result

    except subprocess.TimeoutExpired:
        return {
            "compiled": False,
            "tests_passed": 0,
            "tests_failed": 0,
            "tests_total": 0,
            "all_passed": False,
            "output": f"Cargo test timeout after {timeout}s",
        }
    except Exception as e:
        return {
            "compiled": False,
            "tests_passed": 0,
            "tests_failed": 0,
            "tests_total": 0,
            "all_passed": False,
            "output": str(e),
        }


def check_implementation(sandbox_dir: Path, instance_id: str, card_file_rel: str) -> dict:
    """Check if the implementation looks valid."""
    card_path = sandbox_dir / card_file_rel
    if not card_path.exists():
        return {"valid": False, "reason": "File not found"}
    
    content = card_path.read_text()
    stub_file = GOLD_DIR / "stubs" / f"{instance_id.replace('-', '_')}.rs"
    stub_content = stub_file.read_text() if stub_file.exists() else ""
    
    result = {
        "valid": True,
        "changed": content != stub_content,
        "todos_remaining": content.count("TODO"),
        "stub_todos": stub_content.count("TODO"),
        "has_implementations": "fn " in content and ("game" in content or "return" in content),
    }
    
    # Check brace balance
    if content.count("{") != content.count("}"):
        result["valid"] = False
        result["reason"] = "Unbalanced braces"
    
    return result


def evaluate_instance(instance_id: str, model: str, timeout: int) -> dict:
    """Evaluate a single instance."""
    result = {
        "instance_id": instance_id,
        "model": model,
        "success": False,
        "changed": False,
        "compiled": False,
        "tests_passed": 0,
        "tests_failed": 0,
        "tests_total": 0,
        "duration": 0.0,
        "error": None,
    }

    tmpdir = tempfile.mkdtemp()
    try:
        work_dir = Path(tmpdir)
        instance = load_instance(instance_id)
        card_file_rel = instance.get("card_file", "")

        # Setup
        sandbox_dir = setup_sandbox(instance_id, work_dir)
        prompt = build_prompt(instance)

        # Run agent
        start_time = time.time()
        agent_result = run_opencode(prompt, sandbox_dir, model, timeout)
        duration = time.time() - start_time
        result["duration"] = duration

        # Check if file was changed
        check = check_implementation(sandbox_dir, instance_id, card_file_rel)
        result["changed"] = check.get("changed", False)

        # Save output before injecting tests
        results_dir = BASE_DIR / "results"
        results_dir.mkdir(exist_ok=True)

        card_path = sandbox_dir / card_file_rel
        if card_path.exists():
            output_file = results_dir / f"{instance_id}_{model.replace('/', '_')}_{int(time.time())}.rs"
            shutil.copy(card_path, output_file)
            result["output_file"] = str(output_file)

        # Inject eval tests and run cargo test
        if result["changed"] and check.get("valid", False):
            injected = inject_eval_tests(sandbox_dir, instance_id, card_file_rel)
            if injected:
                test_result = run_cargo_test(sandbox_dir, instance_id, card_file_rel, timeout=90)
                result["compiled"] = test_result["compiled"]
                result["tests_passed"] = test_result["tests_passed"]
                result["tests_failed"] = test_result["tests_failed"]
                result["tests_total"] = test_result["tests_total"]
                result["success"] = test_result["all_passed"]

                # Save test output for debugging
                if not test_result["all_passed"]:
                    result["test_output"] = test_result.get("output", "")[:500]

    except Exception as e:
        result["error"] = str(e)
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)

    return result


def main():
    parser = argparse.ArgumentParser(description="Evaluate models on HP cards")
    parser.add_argument("--model", type=str, default="openai/gpt-4.1-nano", help="Model to use")
    parser.add_argument("--instance", type=str, help="Single instance to evaluate")
    parser.add_argument("--instances", type=str, help="Comma-separated instances")
    parser.add_argument("--count", type=int, default=10, help="Number of instances (if not specified)")
    parser.add_argument("--workers", type=int, default=4, help="Parallel workers")
    parser.add_argument("--timeout", type=int, default=180, help="Timeout per instance (seconds)")
    args = parser.parse_args()
    
    # Determine instances to run
    if args.instance:
        instances = [args.instance]
    elif args.instances:
        instances = args.instances.split(",")
    else:
        instances = get_hp_instances(args.count)
    
    print(f"\n{'='*60}")
    print(f"HP Card Evaluation")
    print(f"{'='*60}")
    print(f"Model: {args.model}")
    print(f"Instances: {len(instances)}")
    print(f"Workers: {args.workers}")
    print(f"Timeout: {args.timeout}s")
    print(f"{'='*60}\n")
    
    for inst in instances:
        print(f"  - {inst}")
    print()
    
    results = []
    start_time = time.time()
    
    with ThreadPoolExecutor(max_workers=args.workers) as executor:
        futures = {
            executor.submit(evaluate_instance, inst, args.model, args.timeout): inst
            for inst in instances
        }

        for future in as_completed(futures):
            instance_id = futures[future]
            try:
                result = future.result()
                results.append(result)

                status = "PASS" if result["success"] else "FAIL"
                tests_str = f"{result['tests_passed']}/{result['tests_total']}" if result["tests_total"] > 0 else "-"
                duration = result["duration"]
                dur_str = f"{duration:.0f}s" if duration < 60 else f"{duration/60:.1f}m"

                if not result["changed"]:
                    status_detail = "unchanged"
                elif not result["compiled"]:
                    status_detail = "no compile"
                else:
                    status_detail = f"tests {tests_str}"

                print(f"  [{len(results):2d}/{len(instances)}] {instance_id}: {status} ({status_detail}, {dur_str})")

            except Exception as e:
                print(f"  [{len(results):2d}/{len(instances)}] {instance_id}: ERROR - {e}")
                results.append({"instance_id": instance_id, "success": False, "error": str(e)})

    total_time = time.time() - start_time

    # Summary
    print(f"\n{'='*60}")
    print("SUMMARY")
    print(f"{'='*60}")
    print(f"Total time: {total_time/60:.1f} minutes")

    successful = sum(1 for r in results if r.get("success"))
    changed = sum(1 for r in results if r.get("changed"))
    compiled = sum(1 for r in results if r.get("compiled"))
    total_tests_passed = sum(r.get("tests_passed", 0) for r in results)
    total_tests = sum(r.get("tests_total", 0) for r in results)

    print(f"\nResults:")
    print(f"  All tests passed: {successful}/{len(results)} ({100*successful/len(results):.0f}%)")
    print(f"  Made changes: {changed}/{len(results)}")
    print(f"  Compiled: {compiled}/{len(results)}")
    print(f"  Total tests: {total_tests_passed}/{total_tests} passed")

    print(f"\n{'Instance':<25} {'Status':>6} {'Changed':>8} {'Compiled':>9} {'Tests':>10} {'Duration':>10}")
    print("-" * 70)
    for r in sorted(results, key=lambda x: x["instance_id"]):
        inst = r["instance_id"]
        status = "PASS" if r.get("success") else "FAIL"
        changed = "yes" if r.get("changed") else "no"
        compiled = "yes" if r.get("compiled") else "no"
        tests_str = f"{r.get('tests_passed', 0)}/{r.get('tests_total', 0)}" if r.get("tests_total", 0) > 0 else "-"
        duration = r.get("duration", 0)
        dur_str = f"{duration:.0f}s" if duration < 60 else f"{duration/60:.1f}m"
        print(f"{inst:<25} {status:>6} {changed:>8} {compiled:>9} {tests_str:>10} {dur_str:>10}")

    print(f"{'='*60}\n")

    # Save detailed results to JSON
    results_json = BASE_DIR / "results" / f"eval_{args.model.replace('/', '_')}_{int(time.time())}.json"
    with open(results_json, "w") as f:
        json.dump({
            "model": args.model,
            "timestamp": datetime.now().isoformat(),
            "total_time": total_time,
            "summary": {
                "successful": successful,
                "changed": changed,
                "compiled": compiled,
                "total_tests_passed": total_tests_passed,
                "total_tests": total_tests,
            },
            "results": results,
        }, f, indent=2)
    print(f"Results saved to: {results_json}")


if __name__ == "__main__":
    main()
