#!/usr/bin/env python3
"""Benchmark a new AI implementation against v1-v4.

Usage:
    python benchmark_ai.py --ai-code-file path/to/ai.rs --name MyAI
    python benchmark_ai.py --ai-code "pub struct MyAI { ... }" --name MyAI

The AI code should implement a struct with:
- `pub fn new(seed: u64) -> Self`
- Implementation of `AiController` trait

Reference opponents are loaded from `reference_algos` (v1-v3) plus v4 from `tcg_ai`.

Example AI code:
```rust
use rand_chacha::ChaCha8Rng;
use rand::SeedableRng;
use tcg_core::{Action, GameView, Prompt};
use tcg_ai::traits::AiController;

pub struct MyAI {
    rng: ChaCha8Rng,
}

impl MyAI {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
        }
    }
}

impl AiController for MyAI {
    fn propose_prompt_response(&mut self, view: &GameView, prompt: &Prompt) -> Vec<Action> {
        vec![Action::EndTurn]
    }
    
    fn propose_free_actions(&mut self, view: &GameView) -> Vec<Action> {
        vec![Action::EndTurn]
    }
}
```
"""

import argparse
import re
import subprocess
import tempfile
from pathlib import Path
import sys

# These import prefixes are provided by the benchmark harness and should be filtered
# Any use statement starting with these prefixes will be removed
PROVIDED_IMPORT_PREFIXES = [
    "use rand_chacha::",
    "use rand::",
    "use tcg_core::",
    "use tcg_ai::traits::",
    "use tcg_ai::AiController",
    "use crate::traits::AiController",
]


def filter_duplicate_imports(code: str) -> str:
    """Remove use statements for types already provided by the benchmark harness."""
    lines = code.split("\n")
    filtered_lines = []
    skip_grouped_use = False
    tcg_core_provided = {
        "Action",
        "GameView",
        "Prompt",
        "Attack",
        "AttackCost",
        "CardInstanceId",
        "Type",
    }
    i = 0
    while i < len(lines):
        line = lines[i]
        stripped = line.strip()

        # Handle multi-line grouped imports for tcg_core and keep only non-provided items
        if stripped.startswith("use tcg_core::") and "{" in stripped:
            group_lines = [line]
            j = i
            while "};" not in lines[j]:
                j += 1
                if j >= len(lines):
                    break
                group_lines.append(lines[j])
            group_text = "\n".join(group_lines)
            if "{" in group_text and "}" in group_text:
                items_text = group_text.split("{", 1)[1].rsplit("}", 1)[0]
                items = [item.strip() for item in items_text.replace("\n", " ").split(",")]
                items = [item for item in items if item]
                remaining = [item for item in items if item not in tcg_core_provided]
                if remaining:
                    filtered_lines.append(f"use tcg_core::{{{', '.join(remaining)}}};")
            i = j + 1
            continue

        # Check if this line is a use statement that should be filtered
        if stripped.startswith("use "):
            should_filter = any(
                stripped.startswith(prefix) for prefix in PROVIDED_IMPORT_PREFIXES
            )
            if should_filter:
                if "{" in stripped and "};" not in stripped:
                    # This is a multi-line grouped use; skip until closing "};"
                    skip_grouped_use = True
                i += 1
                continue
        if skip_grouped_use:
            # Skip lines until the grouped use statement ends
            if "};" in stripped:
                skip_grouped_use = False
            i += 1
            continue
        filtered_lines.append(line)
        i += 1
    return "\n".join(filtered_lines)

BASE_DIR = Path(__file__).parent.parent.parent.parent
OVERZEALOUS_DIR = Path("/Users/joshpurtell/Documents/GitHub/overzealous")
REFERENCE_ALGOS_DIR = Path(__file__).parent / "reference_algos"
ALGO_BENCH_DATA_DIR = Path(__file__).parent / "data"


def generate_ai_module(ai_code: str, ai_name: str) -> str:
    """Generate a complete Rust module file for the AI."""
    # Remove common imports from ai_code since they'll be in the module scope
    # These are provided by the include! site in the benchmark binary
    cleaned_code = filter_duplicate_imports(ai_code)
    
    return f"""// Auto-generated AI module for {ai_name}
{cleaned_code}
"""


def discover_reference_algos() -> list[dict]:
    """Discover reference algos from reference_algos directory."""
    algos = []
    if not REFERENCE_ALGOS_DIR.exists():
        return algos

    for path in sorted(REFERENCE_ALGOS_DIR.glob("*.rs")):
        content = path.read_text()
        match = re.search(r"pub struct (\w+)", content)
        if not match:
            continue
        struct_name = match.group(1)
        module_name = f"ref_{path.stem}"
        algos.append(
            {
                "module": module_name,
                "struct": struct_name,
                "path": path,
                "label": reference_label(struct_name),
            }
        )
    return algos


def reference_label(struct_name: str) -> str:
    if struct_name == "RandomAi":
        return "RandomAi (v1)"
    if struct_name == "RandomAiV2":
        return "RandomAiV2 (v2)"
    if struct_name == "RandomAiV3":
        return "RandomAiV3 (v3)"
    return struct_name


def generate_benchmark_binary(ai_name: str, ai_module_path: Path, reference_algos: list[dict]) -> str:
    """Generate a benchmark binary that uses the AI."""
    # Escape paths for Rust string literal
    ai_module_path_str = str(ai_module_path).replace("\\", "\\\\")
    reference_modules = []
    reference_variants = []
    reference_ai_name_arms = []
    reference_build_arms = []
    reference_opponents = []

    for algo in reference_algos:
        module_name = algo["module"]
        struct_name = algo["struct"]
        label = algo["label"]
        path_str = str(algo["path"]).replace("\\", "\\\\")
        variant = f"Ref{struct_name}"

        reference_modules.append(
            f"""mod {module_name} {{
    include!(r#\"{path_str}\"#);
}}"""
        )
        reference_variants.append(f"{variant},")
        reference_ai_name_arms.append(f'AiType::{variant} => "{label}",')
        reference_build_arms.append(
            f"AiType::{variant} => Box::new({module_name}::{struct_name}::new(seed)),"
        )
        reference_opponents.append((variant, label))

    reference_modules_code = "\n\n".join(reference_modules)
    reference_variants_code = "\n    ".join(reference_variants)
    reference_ai_name_arms_code = "\n        ".join(reference_ai_name_arms)
    reference_build_arms_code = "\n        ".join(reference_build_arms)
    if reference_opponents:
        reference_opponents_code = (
            ",\n        ".join(
                [f'(AiType::{variant}, "{label}")' for variant, label in reference_opponents]
            )
            + ",\n        "
        )
    else:
        reference_opponents_code = ""

    return f"""use tcg_ai::{{AiController, RandomAiV4}};
use tcg_core::{{Action, CardInstance, CardMetaMap, GameState, PlayerId, StepResult}};
use tcg_rules_ex::RulesetConfig;

{reference_modules_code}

mod ai_module {{
    use rand_chacha::ChaCha8Rng;
    use rand::SeedableRng;
    use rand::seq::SliceRandom;
    use rand::seq::IteratorRandom;
    use rand::Rng;
    use tcg_core::{{Action, GameView, Prompt, Attack, AttackCost, CardInstanceId, Type}};
    use tcg_ai::traits::AiController;

    include!(r#"{ai_module_path_str}"#);
}}

use ai_module::{ai_name};

#[derive(PartialEq, Clone, Copy, Debug)]
enum AiType {{
    {reference_variants_code}
    RandomAiV4,
    TestAI,
}}

fn ai_name(ai_type: AiType) -> &'static str {{
    match ai_type {{
        {reference_ai_name_arms_code}
        AiType::RandomAiV4 => "RandomAiV4 (v4)",
        AiType::TestAI => "{ai_name}",
    }}
}}

fn build_ai(ai_type: AiType, seed: u64) -> Box<dyn AiController> {{
    match ai_type {{
        {reference_build_arms_code}
        AiType::RandomAiV4 => Box::new(RandomAiV4::new(seed)),
        AiType::TestAI => Box::new({ai_name}::new(seed)),
    }}
}}

fn apply_first_accepted(game: &mut GameState, player: PlayerId, candidates: Vec<Action>) -> bool {{
    for action in candidates {{
        if game.apply_action(player, action).is_ok() {{
            return true;
        }}
    }}
    false
}}

fn run_match_loop_with_stats(
    mut game: GameState,
    mut p1_ai: Option<&mut dyn AiController>,
    mut p2_ai: Option<&mut dyn AiController>,
    max_steps: usize,
) -> Option<(PlayerId, GameState)> {{
    let mut steps_left = max_steps;
    let mut actions_budget = 5_000usize;

    while steps_left > 0 && actions_budget > 0 {{
        match game.step() {{
            StepResult::Event {{ .. }} => {{}}
            StepResult::GameOver {{ winner }} => return Some((winner, game)),
            StepResult::Prompt {{ prompt, for_player }} => {{
                let view = game.view_for_player(for_player);
                let mut candidates: Vec<Action> = match for_player {{
                    PlayerId::P1 => {{
                        if let Some(ai) = p1_ai.as_mut() {{
                            ai.propose_prompt_response(&view, &prompt)
                        }} else {{
                            Vec::new()
                        }}
                    }}
                    PlayerId::P2 => {{
                        if let Some(ai) = p2_ai.as_mut() {{
                            ai.propose_prompt_response(&view, &prompt)
                        }} else {{
                            Vec::new()
                        }}
                    }}
                }};
                candidates.push(Action::EndTurn);
                let applied = apply_first_accepted(&mut game, for_player, candidates);
                if !applied {{
                    return None;
                }}
                actions_budget = actions_budget.saturating_sub(1);
            }}
            StepResult::Continue => {{
                let phase = game.turn.phase;
                if matches!(phase, tcg_rules_ex::Phase::Main | tcg_rules_ex::Phase::Attack) {{
                    let current = game.turn.player;
                    let view = game.view_for_player(current);
                    let mut candidates: Vec<Action> = match current {{
                        PlayerId::P1 => {{
                            if let Some(ai) = p1_ai.as_mut() {{
                                ai.propose_free_actions(&view)
                            }} else {{
                                Vec::new()
                            }}
                        }}
                        PlayerId::P2 => {{
                            if let Some(ai) = p2_ai.as_mut() {{
                                ai.propose_free_actions(&view)
                            }} else {{
                                Vec::new()
                            }}
                        }}
                    }};
                    candidates.push(Action::EndTurn);
                    let _ = apply_first_accepted(&mut game, current, candidates);
                    actions_budget = actions_budget.saturating_sub(1);
                }}
            }}
        }}
        steps_left -= 1;
    }}

    None
}}

#[derive(Default, Clone)]
struct MatchStats {{
    p1_wins: usize,
    total: usize,
}}

fn run_match_series(
    deck1: &[CardInstance],
    deck2: &[CardInstance],
    p1_ai_type: AiType,
    p2_ai_type: AiType,
    num_matches: usize,
    seed_base: u64,
    count_player: PlayerId,
    card_meta: &CardMetaMap,
) -> MatchStats {{
    let mut stats = MatchStats::default();

    for match_num in 0..num_matches {{
        let seed = seed_base + match_num as u64;
        let game = GameState::new_with_card_meta(
            deck1.to_vec(),
            deck2.to_vec(),
            seed,
            RulesetConfig::default(),
            card_meta.clone(),
        );

        let mut ai1_box = build_ai(p1_ai_type, seed);
        let mut ai2_box = build_ai(p2_ai_type, seed.wrapping_add(9001));

        if let Some((winner, _)) = run_match_loop_with_stats(
            game,
            Some(ai1_box.as_mut()),
            Some(ai2_box.as_mut()),
            5_000,
        ) {{
            stats.total += 1;
            if winner == count_player {{
                stats.p1_wins += 1;
            }}
        }}
    }}

    stats
}}

fn run_blended_matchup(
    deck1: &[CardInstance],
    deck2: &[CardInstance],
    p1_ai_type: AiType,
    p2_ai_type: AiType,
    num_matches: usize,
    seed_base: u64,
    card_meta: &CardMetaMap,
) -> MatchStats {{
    let mut stats = run_match_series(
        deck1,
        deck2,
        p1_ai_type,
        p2_ai_type,
        num_matches,
        seed_base,
        PlayerId::P1,
        card_meta,
    );
    let swapped = run_match_series(
        deck1,
        deck2,
        p2_ai_type,
        p1_ai_type,
        num_matches,
        seed_base + 50_000,
        PlayerId::P2,
        card_meta,
    );
    stats.p1_wins += swapped.p1_wins;
    stats.total += swapped.total;
    stats
}}

fn load_card_meta(cards_db_path: &str) -> CardMetaMap {{
    use rusqlite::Connection;
    let conn = Connection::open(cards_db_path).ok();
    if let Some(conn) = conn {{
        tcg_db::load_card_meta_map(&conn).unwrap_or_default()
    }} else {{
        CardMetaMap::new()
    }}
}}

fn load_deck_by_name(server_db_path: &str, deck_name: &str, player: PlayerId) -> Option<Vec<CardInstance>> {{
    use rusqlite::Connection;
    let conn = Connection::open(server_db_path).ok()?;
    
    let cards_json: String = conn.query_row(
        "SELECT cards_json FROM decks WHERE LOWER(name) LIKE LOWER(?1) AND is_public = 1 LIMIT 1",
        [&format!("%{{}}%", deck_name)],
        |row| row.get(0),
    ).ok()?;

    #[derive(serde::Deserialize)]
    struct DeckEntry {{
        card_def_id: String,
        count: usize,
    }}

    let entries: Vec<DeckEntry> = serde_json::from_str(&cards_json).ok()?;
    let mut deck = Vec::new();
    for entry in entries {{
        for _ in 0..entry.count {{
            deck.push(CardInstance::new(
                tcg_core::CardDefId::new(entry.card_def_id.clone()),
                player,
            ));
        }}
    }}

    if deck.is_empty() {{
        None
    }} else {{
        Some(deck)
    }}
}}

fn main() {{
    let args: Vec<String> = std::env::args().collect();
    let server_db_path = args.get(1).map(|s| s.as_str()).unwrap_or("data/server.sqlite");
    let cards_db_path = args.get(2).map(|s| s.as_str()).unwrap_or("data/cards.sqlite");
    let num_matches = args
        .get(3)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(50);

    println!("Benchmarking {ai_name} vs v1-v4");
    println!("==================================");
    println!("Server DB: {{}}", server_db_path);
    println!("Cards DB: {{}}", cards_db_path);
    println!("Matches per pairing: {{}}", num_matches);
    println!();

    let card_meta = load_card_meta(cards_db_path);
    println!("Loaded {{}} card definitions", card_meta.len());

    // Load a test deck
    let deck_name = "Overzealous";
    let deck = match load_deck_by_name(server_db_path, deck_name, PlayerId::P1) {{
        Some(d) => d,
        None => {{
            eprintln!("Failed to load deck: {{}}", deck_name);
            return;
        }}
    }};
    println!("Loaded deck: {{}} ({{}} cards)", deck_name, deck.len());

    let opponents = [
        {reference_opponents_code}(AiType::RandomAiV4, "RandomAiV4 (v4)"),
    ];

    let mut overall_stats = MatchStats::default();
    let mut opponent_results = Vec::new();

    for (opponent_type, opponent_name) in &opponents {{
        println!("\\nOpponent: {{}}", opponent_name);
        let stats = run_blended_matchup(
            &deck,
            &deck,
            AiType::TestAI,
            *opponent_type,
            num_matches,
            0,
            &card_meta,
        );

        let win_rate = if stats.total > 0 {{
            (stats.p1_wins as f64 / stats.total as f64) * 100.0
        }} else {{
            0.0
        }};

        println!("  {ai_name}: {{}} wins / {{}} matches ({{:.1}}%)", 
            stats.p1_wins, stats.total, win_rate);
        println!("  {{}}: {{}} wins / {{}} matches ({{:.1}}%)", 
            opponent_name, stats.total - stats.p1_wins, stats.total, 100.0 - win_rate);

        opponent_results.push((opponent_name.to_string(), stats.clone()));
        overall_stats.p1_wins += stats.p1_wins;
        overall_stats.total += stats.total;
    }}

    let overall_rate = if overall_stats.total > 0 {{
        (overall_stats.p1_wins as f64 / overall_stats.total as f64) * 100.0
    }} else {{
        0.0
    }};

    println!("\\nSummary:");
    println!("{{:18}} {{:>8}} {{:>9}} {{:>8}}", "Opponent", "Wins", "Matches", "Win%");
    for (name, stats) in &opponent_results {{
        let rate = if stats.total > 0 {{
            (stats.p1_wins as f64 / stats.total as f64) * 100.0
        }} else {{
            0.0
        }};
        println!("{{:18}} {{:>8}} {{:>9}} {{:>7.1}}%", name, stats.p1_wins, stats.total, rate);
    }}

    println!("\\nOverall: {{}} wins / {{}} matches ({{:.1}}%)", 
        overall_stats.p1_wins, overall_stats.total, overall_rate);
}}
"""


def main():
    parser = argparse.ArgumentParser(description="Benchmark a new AI against v1-v4")
    parser.add_argument("--ai-code-file", type=Path, help="Path to Rust file with AI implementation")
    parser.add_argument("--ai-code", type=str, help="Rust code string with AI implementation")
    parser.add_argument("--name", type=str, required=True, help="Name of the AI (used for struct name)")
    parser.add_argument("--matches", type=int, default=50, help="Number of matches per opponent")
    default_server = str(ALGO_BENCH_DATA_DIR / "server.sqlite") if (ALGO_BENCH_DATA_DIR / "server.sqlite").exists() else "data/server.sqlite"
    default_cards = str(ALGO_BENCH_DATA_DIR / "cards.sqlite") if (ALGO_BENCH_DATA_DIR / "cards.sqlite").exists() else "data/cards.sqlite"
    parser.add_argument("--server-db", type=str, default=default_server, help="Path to server DB")
    parser.add_argument("--cards-db", type=str, default=default_cards, help="Path to cards DB")
    parser.add_argument("--overzealous-dir", type=Path, default=OVERZEALOUS_DIR, help="Path to overzealous repo")

    args = parser.parse_args()

    if not args.ai_code_file and not args.ai_code:
        parser.error("Must provide either --ai-code-file or --ai-code")

    # Read AI code
    if args.ai_code_file:
        ai_code = args.ai_code_file.read_text()
    else:
        ai_code = args.ai_code

    reference_algos = discover_reference_algos()
    if not reference_algos:
        print("Warning: no reference algos found in reference_algos directory.")

    # Generate temporary files
    with tempfile.TemporaryDirectory() as tmpdir:
        tmpdir = Path(tmpdir)
        ai_module_file = tmpdir / f"{args.name.lower()}_ai.rs"
        benchmark_file = tmpdir / "benchmark.rs"
        cargo_toml = tmpdir / "Cargo.toml"

        # Generate AI module
        ai_module_content = generate_ai_module(ai_code, args.name)
        ai_module_file.write_text(ai_module_content)

        # Generate benchmark binary
        benchmark_content = generate_benchmark_binary(
            args.name, ai_module_file, reference_algos
        )
        benchmark_file.write_text(benchmark_content)

        # Generate Cargo.toml
        overzealous_path = str(args.overzealous_dir).replace('\\', '/')
        cargo_content = f"""[package]
name = "ai_benchmark"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "benchmark"
path = "benchmark.rs"

[dependencies]
tcg_core = {{ path = "{overzealous_path}/tcg_core" }}
tcg_db = {{ path = "{overzealous_path}/tcg_db" }}
tcg_rules_ex = {{ path = "{overzealous_path}/tcg_rules_ex" }}
tcg_ai = {{ path = "{overzealous_path}/tcg_ai" }}
rusqlite = {{ version = "0.32", features = ["bundled"] }}
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
rand = "0.8"
rand_chacha = "0.3"
"""
        cargo_toml.write_text(cargo_content)

        # Compile and run
        print(f"Compiling benchmark for {args.name}...")
        compile_result = subprocess.run(
            ["cargo", "build", "--release", "--bin", "benchmark"],
            cwd=tmpdir,
            capture_output=True,
            text=True,
        )

        if compile_result.returncode != 0:
            print("Compilation failed:")
            print(compile_result.stderr)
            sys.exit(1)

        print("Running benchmark...")
        run_result = subprocess.run(
            [
                "cargo", "run", "--release", "--bin", "benchmark", "--",
                args.server_db,
                args.cards_db,
                str(args.matches),
            ],
            cwd=tmpdir,
        )

        if run_result.returncode != 0:
            print("Benchmark failed")
            sys.exit(1)


if __name__ == "__main__":
    main()
