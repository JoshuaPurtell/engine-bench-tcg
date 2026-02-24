use tcg_ai::{AiController, RandomAiV4};
use tcg_core::{Action, CardInstance, CardMetaMap, GameState, PlayerId, StepResult};
use tcg_rules_ex::RulesetConfig;

mod ref_random_ai {
    include!(r#"/Users/joshpurtell/Documents/Github/engine-bench-tcg/auxiliary_tasks/algo_bench/reference_algos/random_ai.rs"#);
}

mod ref_random_ai_v2 {
    include!(r#"/Users/joshpurtell/Documents/Github/engine-bench-tcg/auxiliary_tasks/algo_bench/reference_algos/random_ai_v2.rs"#);
}

mod ref_random_ai_v3 {
    include!(r#"/Users/joshpurtell/Documents/Github/engine-bench-tcg/auxiliary_tasks/algo_bench/reference_algos/random_ai_v3.rs"#);
}

mod ai_module {
    use rand_chacha::ChaCha8Rng;
    use rand::SeedableRng;
    use rand::seq::SliceRandom;
    use rand::seq::IteratorRandom;
    use rand::Rng;
    use tcg_core::{Action, GameView, Prompt, Attack, AttackCost, CardInstanceId, Type};
    use tcg_ai::traits::AiController;

    include!(r#"/Users/joshpurtell/Documents/Github/engine-bench-tcg/.algo_bench_build_cache/2cf87b74de9ff853c1d1bd5d52fa9d882749072332c87dbd0534c31027333697/templateai_ai.rs"#);
}

use ai_module::TemplateAi;

#[derive(PartialEq, Clone, Copy, Debug)]
enum AiType {
    RefRandomAi,
    RefRandomAiV2,
    RefRandomAiV3,
    RandomAiV4,
    TestAI,
}

fn ai_name(ai_type: AiType) -> &'static str {
    match ai_type {
        AiType::RefRandomAi => "RandomAi (v1)",
        AiType::RefRandomAiV2 => "RandomAiV2 (v2)",
        AiType::RefRandomAiV3 => "RandomAiV3 (v3)",
        AiType::RandomAiV4 => "RandomAiV4 (v4)",
        AiType::TestAI => "TemplateAi",
    }
}

fn build_ai(ai_type: AiType, seed: u64) -> Box<dyn AiController> {
    match ai_type {
        AiType::RefRandomAi => Box::new(ref_random_ai::RandomAi::new(seed)),
        AiType::RefRandomAiV2 => Box::new(ref_random_ai_v2::RandomAiV2::new(seed)),
        AiType::RefRandomAiV3 => Box::new(ref_random_ai_v3::RandomAiV3::new(seed)),
        AiType::RandomAiV4 => Box::new(RandomAiV4::new(seed)),
        AiType::TestAI => Box::new(TemplateAi::new(seed)),
    }
}

fn run_match_loop_with_stats(
    mut game: GameState,
    mut p1_ai: Option<&mut dyn AiController>,
    mut p2_ai: Option<&mut dyn AiController>,
    max_steps: usize,
) -> Option<(PlayerId, GameState, u16, u16)> {
    let mut steps_left = max_steps;
    let mut actions_budget = 5_000usize;
    let mut p1_evolutions: u16 = 0;
    let mut p2_evolutions: u16 = 0;

    while steps_left > 0 && actions_budget > 0 {
        match game.step() {
            StepResult::Event { .. } => {}
            StepResult::GameOver { winner } => {
                return Some((winner, game, p1_evolutions, p2_evolutions));
            }
            StepResult::Prompt { prompt, for_player } => {
                let view = game.view_for_player(for_player);
                let mut candidates: Vec<Action> = match for_player {
                    PlayerId::P1 => {
                        if let Some(ai) = p1_ai.as_mut() {
                            ai.propose_prompt_response(&view, &prompt)
                        } else {
                            Vec::new()
                        }
                    }
                    PlayerId::P2 => {
                        if let Some(ai) = p2_ai.as_mut() {
                            ai.propose_prompt_response(&view, &prompt)
                        } else {
                            Vec::new()
                        }
                    }
                };
                candidates.push(Action::EndTurn);
                let applied = accepted_action(&mut game, for_player, candidates);
                if let Some(action) = applied {
                    if matches!(action, Action::EvolveFromHand { .. }) {
                        if for_player == PlayerId::P1 {
                            p1_evolutions = p1_evolutions.saturating_add(1);
                        } else {
                            p2_evolutions = p2_evolutions.saturating_add(1);
                        }
                    }
                } else {
                    return None;
                }
                actions_budget = actions_budget.saturating_sub(1);
            }
            StepResult::Continue => {
                let phase = game.turn.phase;
                if matches!(phase, tcg_rules_ex::Phase::Main | tcg_rules_ex::Phase::Attack) {
                    let current = game.turn.player;
                    let view = game.view_for_player(current);
                    let mut candidates: Vec<Action> = match current {
                        PlayerId::P1 => {
                            if let Some(ai) = p1_ai.as_mut() {
                                ai.propose_free_actions(&view)
                            } else {
                                Vec::new()
                            }
                        }
                        PlayerId::P2 => {
                            if let Some(ai) = p2_ai.as_mut() {
                                ai.propose_free_actions(&view)
                            } else {
                                Vec::new()
                            }
                        }
                    };
                    candidates.push(Action::EndTurn);
                    if let Some(action) = accepted_action(&mut game, current, candidates) {
                        if matches!(action, Action::EvolveFromHand { .. }) {
                            if current == PlayerId::P1 {
                                p1_evolutions = p1_evolutions.saturating_add(1);
                            } else {
                                p2_evolutions = p2_evolutions.saturating_add(1);
                            }
                        }
                    }
                    actions_budget = actions_budget.saturating_sub(1);
                }
            }
        }
        steps_left -= 1;
    }

    None
}

#[derive(Default, Clone)]
struct MatchStats {
    p1_wins: usize,
    total: usize,
    outcomes: Vec<MatchOutcome>,
}

#[derive(Clone)]
struct MatchOutcome {
    tracked_won: bool,
    turns: u32,
    tracked_prizes_taken: u8,
    opponent_prizes_taken: u8,
    tracked_evolutions: u16,
    opponent_evolutions: u16,
}

fn accepted_action(
    game: &mut GameState,
    player: PlayerId,
    candidates: Vec<Action>,
) -> Option<Action> {
    for action in candidates {
        if game.apply_action(player, action.clone()).is_ok() {
            return Some(action);
        }
    }
    None
}

fn summarize_u32(values: &[u32]) -> serde_json::Value {
    if values.is_empty() {
        return serde_json::json!({"count": 0});
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let count = sorted.len();
    let sum: u64 = sorted.iter().map(|v| *v as u64).sum();
    let mean = (sum as f64) / (count as f64);
    let p50 = sorted[(count - 1) / 2];
    let p90 = sorted[((count - 1) * 9) / 10];
    let mut counts = std::collections::BTreeMap::<u32, usize>::new();
    for value in &sorted {
        *counts.entry(*value).or_insert(0) += 1;
    }
    serde_json::json!({
        "count": count,
        "min": sorted[0],
        "max": sorted[count - 1],
        "mean": mean,
        "p50": p50,
        "p90": p90,
        "counts": counts,
    })
}

fn summarize_u16(values: &[u16]) -> serde_json::Value {
    summarize_u32(&values.iter().map(|v| *v as u32).collect::<Vec<_>>())
}

fn summarize_u8(values: &[u8]) -> serde_json::Value {
    summarize_u32(&values.iter().map(|v| *v as u32).collect::<Vec<_>>())
}

fn summarize_match_outcomes(outcomes: &[MatchOutcome]) -> serde_json::Value {
    let turns: Vec<u32> = outcomes.iter().map(|o| o.turns).collect();
    let tracked_prizes_taken: Vec<u8> = outcomes.iter().map(|o| o.tracked_prizes_taken).collect();
    let opponent_prizes_taken: Vec<u8> = outcomes.iter().map(|o| o.opponent_prizes_taken).collect();
    let tracked_evolutions: Vec<u16> = outcomes.iter().map(|o| o.tracked_evolutions).collect();
    let opponent_evolutions: Vec<u16> = outcomes.iter().map(|o| o.opponent_evolutions).collect();
    let tracked_wins = outcomes.iter().filter(|o| o.tracked_won).count();
    let total = outcomes.len();
    let tracked_win_rate = if total > 0 {
        (tracked_wins as f64) / (total as f64)
    } else {
        0.0
    };
    serde_json::json!({
        "tracked_win_rate": tracked_win_rate,
        "tracked_wins": tracked_wins,
        "total_games": total,
        "turns": summarize_u32(&turns),
        "tracked_prizes_taken": summarize_u8(&tracked_prizes_taken),
        "opponent_prizes_taken": summarize_u8(&opponent_prizes_taken),
        "tracked_evolutions": summarize_u16(&tracked_evolutions),
        "opponent_evolutions": summarize_u16(&opponent_evolutions),
    })
}

fn run_match_series(
    deck1: &[CardInstance],
    deck2: &[CardInstance],
    p1_ai_type: AiType,
    p2_ai_type: AiType,
    num_matches: usize,
    seed_base: u64,
    count_player: PlayerId,
    card_meta: &CardMetaMap,
) -> MatchStats {
    let mut stats = MatchStats::default();

    for match_num in 0..num_matches {
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

        if let Some((winner, game, p1_evolutions, p2_evolutions)) = run_match_loop_with_stats(
            game,
            Some(ai1_box.as_mut()),
            Some(ai2_box.as_mut()),
            5_000,
        ) {
            let p1_view = game.view_for_player(PlayerId::P1);
            let p1_prizes_remaining = p1_view.my_prizes_count as u8;
            let p2_prizes_remaining = p1_view.opponent_prizes_count as u8;
            let p1_prizes_taken = 6u8.saturating_sub(p1_prizes_remaining);
            let p2_prizes_taken = 6u8.saturating_sub(p2_prizes_remaining);
            let turns = game.turn.number;
            let (tracked_won, tracked_prizes_taken, opponent_prizes_taken, tracked_evolutions, opponent_evolutions) =
                if count_player == PlayerId::P1 {
                    (
                        winner == PlayerId::P1,
                        p1_prizes_taken,
                        p2_prizes_taken,
                        p1_evolutions,
                        p2_evolutions,
                    )
                } else {
                    (
                        winner == PlayerId::P2,
                        p2_prizes_taken,
                        p1_prizes_taken,
                        p2_evolutions,
                        p1_evolutions,
                    )
                };

            stats.total += 1;
            if tracked_won {
                stats.p1_wins += 1;
            }
            stats.outcomes.push(MatchOutcome {
                tracked_won,
                turns,
                tracked_prizes_taken,
                opponent_prizes_taken,
                tracked_evolutions,
                opponent_evolutions,
            });
        }
    }

    stats
}

fn run_blended_matchup(
    deck1: &[CardInstance],
    deck2: &[CardInstance],
    p1_ai_type: AiType,
    p2_ai_type: AiType,
    num_matches: usize,
    seed_base: u64,
    card_meta: &CardMetaMap,
) -> MatchStats {
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
    stats.outcomes.extend(swapped.outcomes);
    stats
}

fn load_card_meta(cards_db_path: &str) -> CardMetaMap {
    use rusqlite::Connection;
    let conn = Connection::open(cards_db_path).ok();
    if let Some(conn) = conn {
        tcg_db::load_card_meta_map(&conn).unwrap_or_default()
    } else {
        CardMetaMap::new()
    }
}

#[derive(Clone, serde::Deserialize)]
struct DeckEntry {
    card_def_id: String,
    count: usize,
}

#[derive(Clone)]
struct DeckSpec {
    name: String,
    entries: Vec<DeckEntry>,
}

fn build_deck_for_player(entries: &[DeckEntry], player: PlayerId) -> Vec<CardInstance> {
    let mut deck = Vec::new();
    for entry in entries {
        for _ in 0..entry.count {
            deck.push(CardInstance::new(
                tcg_core::CardDefId::new(entry.card_def_id.clone()),
                player,
            ));
        }
    }
    deck
}

fn load_public_deck_specs(server_db_path: &str) -> Vec<DeckSpec> {
    use rusqlite::Connection;
    let conn = match Connection::open(server_db_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut stmt = match conn.prepare(
        "SELECT name, cards_json FROM decks WHERE is_public = 1 ORDER BY LOWER(name), deck_id"
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let rows = match stmt.query_map([], |row| {
        let name: String = row.get(0)?;
        let cards_json: String = row.get(1)?;
        Ok((name, cards_json))
    }) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let mut specs = Vec::new();
    for row in rows {
        if let Ok((name, cards_json)) = row {
            if let Ok(entries) = serde_json::from_str::<Vec<DeckEntry>>(&cards_json) {
                if !entries.is_empty() {
                    specs.push(DeckSpec { name, entries });
                }
            }
        }
    }
    specs
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let server_db_path = args.get(1).map(|s| s.as_str()).unwrap_or("data/server.sqlite");
    let cards_db_path = args.get(2).map(|s| s.as_str()).unwrap_or("data/cards.sqlite");
    let num_matches = args
        .get(3)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(50);
    let seed_base = args
        .get(4)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    println!("Benchmarking TemplateAi vs v1-v4");
    println!("==================================");
    println!("Server DB: {}", server_db_path);
    println!("Cards DB: {}", cards_db_path);
    println!("Matches per pairing: {}", num_matches);
    println!("Seed base: {}", seed_base);
    println!();

    let card_meta = load_card_meta(cards_db_path);
    println!("Loaded {} card definitions", card_meta.len());

    // Load all public decks and deterministically select a non-mirrored deck pair
    // from seed_base. This makes each seed correspond to one deck-combination seed.
    let deck_specs = load_public_deck_specs(server_db_path);
    if deck_specs.len() < 2 {
        eprintln!("Need at least 2 public decks in server DB; found {}", deck_specs.len());
        return;
    }
    println!("Loaded {} public decks", deck_specs.len());
    for spec in &deck_specs {
        let count: usize = spec.entries.iter().map(|entry| entry.count).sum();
        println!("  - {} ({} cards)", spec.name, count);
    }

    let mut deck_pairs: Vec<(usize, usize)> = Vec::new();
    for i in 0..deck_specs.len() {
        for j in (i + 1)..deck_specs.len() {
            deck_pairs.push((i, j));
        }
    }
    if deck_pairs.is_empty() {
        eprintln!("No unique non-mirror deck pairs available");
        return;
    }
    let pair_idx = (seed_base as usize) % deck_pairs.len();
    let (deck1_idx, deck2_idx) = deck_pairs[pair_idx];
    let deck1_spec = &deck_specs[deck1_idx];
    let deck2_spec = &deck_specs[deck2_idx];
    let deck1 = build_deck_for_player(&deck1_spec.entries, PlayerId::P1);
    let deck2 = build_deck_for_player(&deck2_spec.entries, PlayerId::P2);
    println!(
        "Seed-selected deck pair [{}/{}]: {} vs {}",
        pair_idx + 1,
        deck_pairs.len(),
        deck1_spec.name,
        deck2_spec.name
    );

    let opponents = [
        (AiType::RefRandomAi, "RandomAi (v1)"),
        (AiType::RefRandomAiV2, "RandomAiV2 (v2)"),
        (AiType::RefRandomAiV3, "RandomAiV3 (v3)"),
        (AiType::RandomAiV4, "RandomAiV4 (v4)"),
    ];

    let mut overall_stats = MatchStats::default();
    let mut opponent_results = Vec::new();

    for (opponent_idx, (opponent_type, opponent_name)) in opponents.iter().enumerate() {
        println!("\nOpponent: {}", opponent_name);
        let opponent_seed_base = seed_base + (opponent_idx as u64) * 1_000_000;
        let stats = run_blended_matchup(
            &deck1,
            &deck2,
            AiType::TestAI,
            *opponent_type,
            num_matches,
            opponent_seed_base,
            &card_meta,
        );

        let win_rate = if stats.total > 0 {
            (stats.p1_wins as f64 / stats.total as f64) * 100.0
        } else {
            0.0
        };

        println!("  TemplateAi: {} wins / {} matches ({:.1}%)", 
            stats.p1_wins, stats.total, win_rate);
        println!("  {}: {} wins / {} matches ({:.1}%)", 
            opponent_name, stats.total - stats.p1_wins, stats.total, 100.0 - win_rate);

        opponent_results.push((opponent_name.to_string(), stats.clone()));
        overall_stats.p1_wins += stats.p1_wins;
        overall_stats.total += stats.total;
        overall_stats.outcomes.extend(stats.outcomes.clone());
    }

    let overall_rate = if overall_stats.total > 0 {
        (overall_stats.p1_wins as f64 / overall_stats.total as f64) * 100.0
    } else {
        0.0
    };

    println!("\nSummary:");
    println!("{:18} {:>8} {:>9} {:>8}", "Opponent", "Wins", "Matches", "Win%");
    for (name, stats) in &opponent_results {
        let rate = if stats.total > 0 {
            (stats.p1_wins as f64 / stats.total as f64) * 100.0
        } else {
            0.0
        };
        println!("{:18} {:>8} {:>9} {:>7.1}%", name, stats.p1_wins, stats.total, rate);
    }

    println!("\nOverall: {} wins / {} matches ({:.1}%)", 
        overall_stats.p1_wins, overall_stats.total, overall_rate);

    let per_opponent_metrics: Vec<serde_json::Value> = opponent_results
        .iter()
        .map(|(name, stats)| {
            serde_json::json!({
                "opponent": name,
                "wins": stats.p1_wins,
                "matches": stats.total,
                "win_rate": if stats.total > 0 { (stats.p1_wins as f64) / (stats.total as f64) } else { 0.0 },
                "distributions": summarize_match_outcomes(&stats.outcomes),
            })
        })
        .collect();
    let benchmark_metrics = serde_json::json!({
        "version": "algo_bench_metrics_v1",
        "matches_per_opponent_per_side": num_matches,
        "opponent_count": opponent_results.len(),
        "total_games": overall_stats.total,
        "overall_win_rate": if overall_stats.total > 0 {
            (overall_stats.p1_wins as f64) / (overall_stats.total as f64)
        } else {
            0.0
        },
        "overall_distributions": summarize_match_outcomes(&overall_stats.outcomes),
        "per_opponent": per_opponent_metrics,
    });
    println!("BENCHMARK_METRICS_JSON: {}", benchmark_metrics);
}
