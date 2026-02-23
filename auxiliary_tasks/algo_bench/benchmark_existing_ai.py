#!/usr/bin/env python3
"""Benchmark an existing AI from the repo against v1-v4.

Uses reference algos from reference_algos (v1-v3) plus v4 from tcg_ai.

Usage:
    python benchmark_existing_ai.py --ai-name RandomAiV4 --matches 50
"""

import argparse
import re
import subprocess
import tempfile
from pathlib import Path
import sys

OVERZEALOUS_DIR = Path("/Users/joshpurtell/Documents/GitHub/overzealous")
REFERENCE_ALGOS_DIR = Path(__file__).parent / "reference_algos"
ALGO_BENCH_DATA_DIR = Path(__file__).parent / "data"


def reference_label(struct_name: str) -> str:
    if struct_name == "RandomAi":
        return "RandomAi (v1)"
    if struct_name == "RandomAiV2":
        return "RandomAiV2 (v2)"
    if struct_name == "RandomAiV3":
        return "RandomAiV3 (v3)"
    return struct_name


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


def generate_benchmark_binary(ai_name: str, reference_algos: list[dict]) -> str:
    """Generate a benchmark binary that uses the existing AI."""
    reference_modules = []
    reference_variants = []
    reference_build_arms = []
    reference_opponents = []
    reference_name_map = []

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
        reference_build_arms.append(
            f"AiType::{variant} => Box::new({module_name}::{struct_name}::new(seed)),"
        )
        reference_opponents.append((variant, label))
        reference_name_map.append((struct_name, variant))

    reference_modules_code = "\n\n".join(reference_modules)
    reference_variants_code = "\n    ".join(reference_variants)
    reference_build_arms_code = "\n        ".join(reference_build_arms)
    reference_name_map_code = "\n            ".join(
        [f"\"{name}\" => AiType::{variant}," for name, variant in reference_name_map]
    )
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

#[derive(PartialEq, Clone, Copy, Debug)]
enum AiType {{
    {reference_variants_code}
    RandomAiV4,
}}

fn ai_type_from_name(name: &str) -> AiType {{
    match name {{
{reference_name_map_code}
        "RandomAiV4" => AiType::RandomAiV4,
        _ => panic!("Unknown AI: {{}}", name),
    }}
}}

fn build_ai(ai_type: AiType, seed: u64) -> Box<dyn AiController> {{
    match ai_type {{
        {reference_build_arms_code}
        AiType::RandomAiV4 => Box::new(RandomAiV4::new(seed)),
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

fn run_blended_matchup(
    deck1: &[CardInstance],
    deck2: &[CardInstance],
    p1_ai_type: AiType,
    p2_ai_type: AiType,
    num_matches: usize,
    seed_base: u64,
    card_meta: &CardMetaMap,
) -> MatchStats {{
    let mut stats = MatchStats::default();

    // Run matches with p1_ai_type as P1
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
            if winner == PlayerId::P1 {{
                stats.p1_wins += 1;
            }}
        }}
    }}

    // Run matches with swapped positions
    for match_num in 0..num_matches {{
        let seed = seed_base + 50_000 + match_num as u64;
        let game = GameState::new_with_card_meta(
            deck1.to_vec(),
            deck2.to_vec(),
            seed,
            RulesetConfig::default(),
            card_meta.clone(),
        );

        let mut ai1_box = build_ai(p2_ai_type, seed);
        let mut ai2_box = build_ai(p1_ai_type, seed.wrapping_add(9001));

        if let Some((winner, _)) = run_match_loop_with_stats(
            game,
            Some(ai1_box.as_mut()),
            Some(ai2_box.as_mut()),
            5_000,
        ) {{
            stats.total += 1;
            if winner == PlayerId::P2 {{
                stats.p1_wins += 1;
            }}
        }}
    }}

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

    let test_ai = ai_type_from_name("{ai_name}");

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
            test_ai,
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
    parser = argparse.ArgumentParser(description="Benchmark an existing AI against v1-v4")
    parser.add_argument("--ai-name", type=str, default="RandomAiV4", help="Name of the AI (RandomAiV4, etc.)")
    parser.add_argument("--matches", type=int, default=50, help="Number of matches per opponent")
    default_server = str(ALGO_BENCH_DATA_DIR / "server.sqlite") if (ALGO_BENCH_DATA_DIR / "server.sqlite").exists() else "data/server.sqlite"
    default_cards = str(ALGO_BENCH_DATA_DIR / "cards.sqlite") if (ALGO_BENCH_DATA_DIR / "cards.sqlite").exists() else "data/cards.sqlite"
    parser.add_argument("--server-db", type=str, default=default_server, help="Path to server DB")
    parser.add_argument("--cards-db", type=str, default=default_cards, help="Path to cards DB")

    args = parser.parse_args()

    reference_algos = discover_reference_algos()
    if not reference_algos:
        print("Warning: no reference algos found in reference_algos directory.")

    with tempfile.TemporaryDirectory() as tmpdir:
        tmpdir = Path(tmpdir)
        benchmark_file = tmpdir / "benchmark.rs"
        cargo_toml = tmpdir / "Cargo.toml"

        # Generate benchmark binary
        benchmark_content = generate_benchmark_binary(args.ai_name, reference_algos)
        benchmark_file.write_text(benchmark_content)

        # Generate Cargo.toml
        overzealous_path = str(OVERZEALOUS_DIR).replace('\\', '/')
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
        print(f"Compiling benchmark for {args.ai_name}...")
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
        server_db = str(Path(args.server_db).resolve())
        cards_db = str(Path(args.cards_db).resolve())
        run_result = subprocess.run(
            [
                "cargo", "run", "--release", "--bin", "benchmark", "--",
                server_db,
                cards_db,
                str(args.matches),
            ],
            cwd=tmpdir,
        )

        if run_result.returncode != 0:
            print("Benchmark failed")
            sys.exit(1)


if __name__ == "__main__":
    main()
