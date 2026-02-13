//! Python bindings for Pokemon TCG game engine.
//!
//! This crate provides PyO3 bindings to run Pokemon TCG games
//! between an LLM-based agent and the AI v4 opponent.

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use serde::Serialize;
use serde_json::{json, Map, Value};

use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;

use tcg_ai::{AiController, RandomAiV4};
use tcg_ai::react::{render_game_view, render_game_view_compact, parse_response};

use tcg_core::{
    Action, Attack, AttackCost, CardDefId, CardInstance, CardInstanceId, CardMeta, CardMetaMap,
    GameState, PlayerId, PokemonView, Stage, StepResult, GameView, PendingPrompt, Prompt, Type,
};
use tcg_rules_ex::RulesetConfig;

// ============================================================================
// Card Registry - Defines metadata and attacks for all supported cards
// ============================================================================

fn create_attack(name: &str, damage: u16, attack_type: Type, total_energy: u8, energy_types: Vec<Type>) -> Attack {
    Attack {
        name: name.to_string(),
        damage,
        attack_type,
        cost: AttackCost {
            total_energy,
            types: energy_types,
        },
        effect_ast: None,
    }
}

fn get_card_meta(def_id: &str) -> Option<CardMeta> {
    // Normalize the def_id for matching
    let lower = def_id.to_lowercase();

    // Energy cards
    // Accept multiple ID schemes:
    // - energy-psychic (used by tcg_py demos)
    // - hp-109-psychic-energy (used by Holon Phantoms basic energy instances)
    if lower.starts_with("hp-") && lower.ends_with("-energy") {
        let energy_type = if lower.contains("psychic-energy") {
            Type::Psychic
        } else if lower.contains("fire-energy") {
            Type::Fire
        } else if lower.contains("grass-energy") {
            Type::Grass
        } else if lower.contains("water-energy") {
            Type::Water
        } else if lower.contains("lightning-energy") {
            Type::Lightning
        } else if lower.contains("fighting-energy") {
            Type::Fighting
        } else if lower.contains("darkness-energy") {
            Type::Darkness
        } else if lower.contains("metal-energy") {
            Type::Metal
        } else {
            Type::Colorless
        };

        return Some(CardMeta {
            name: def_id.to_string(),
            is_basic: true,
            is_tool: false,
            is_stadium: false,
            is_pokemon: false,
            is_energy: true,
            hp: 0,
            energy_kind: Some("Basic".to_string()),
            provides: vec![energy_type],
            trainer_kind: None,
            is_ex: false,
            is_star: false,
            is_delta: false,
            stage: Stage::Basic,
            types: vec![],
            weakness: None,
            resistance: None,
            retreat_cost: None,
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![],
            card_type: "Energy".to_string(),
            delta_species: false,
        });
    }

    // Energy cards
    if lower.starts_with("energy-") {
        let energy_type = match lower.as_str() {
            "energy-psychic" => Type::Psychic,
            "energy-fire" => Type::Fire,
            "energy-grass" => Type::Grass,
            "energy-water" => Type::Water,
            "energy-lightning" => Type::Lightning,
            "energy-fighting" => Type::Fighting,
            "energy-darkness" => Type::Darkness,
            "energy-metal" => Type::Metal,
            _ => Type::Colorless,
        };
        return Some(CardMeta {
            name: def_id.to_string(),
            is_basic: true,
            is_tool: false,
            is_stadium: false,
            is_pokemon: false,
            is_energy: true,
            hp: 0,
            energy_kind: Some("Basic".to_string()),
            provides: vec![energy_type],
            trainer_kind: None,
            is_ex: false,
            is_star: false,
            is_delta: false,
            stage: Stage::Basic,
            types: vec![],
            weakness: None,
            resistance: None,
            retreat_cost: None,
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![],
            card_type: "Energy".to_string(),
            delta_species: false,
        });
    }

    // Pokemon and Trainer cards
    match lower.as_str() {
        // DF-061 Ralts δ - Basic Psychic - HP 50
        "df-061-ralts" => Some(CardMeta {
            name: "Ralts δ".to_string(),
            is_basic: true,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp: 50,
            energy_kind: None,
            provides: vec![],
            trainer_kind: None,
            is_ex: false,
            is_star: false,
            is_delta: true,
            stage: Stage::Basic,
            types: vec![Type::Psychic],
            weakness: None,
            resistance: None,
            retreat_cost: Some(1),
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![
                create_attack("Hypnosis", 0, Type::Psychic, 1, vec![Type::Psychic]),
                create_attack("Psychic Boom", 10, Type::Psychic, 2, vec![Type::Psychic, Type::Colorless]),
            ],
            card_type: "Pokemon".to_string(),
            delta_species: true,
        }),

        // DF-033 Kirlia δ - Stage 1 Psychic - HP 70
        "df-033-kirlia" => Some(CardMeta {
            name: "Kirlia δ".to_string(),
            is_basic: false,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp: 70,
            energy_kind: None,
            provides: vec![],
            trainer_kind: None,
            is_ex: false,
            is_star: false,
            is_delta: true,
            stage: Stage::Stage1,
            types: vec![Type::Psychic],
            weakness: None,
            resistance: None,
            retreat_cost: Some(1),
            trainer_effect: None,
            evolves_from: Some("Ralts".to_string()),
            attacks: vec![
                create_attack("Smack", 20, Type::Colorless, 1, vec![Type::Colorless]),
                create_attack("Flickering Flames", 40, Type::Psychic, 2, vec![Type::Psychic, Type::Colorless]),
            ],
            card_type: "Pokemon".to_string(),
            delta_species: true,
        }),

        // DF-093 Gardevoir ex δ - Stage 2 Fire - HP 150
        "df-093-gardevoir-ex" => Some(CardMeta {
            name: "Gardevoir ex δ".to_string(),
            is_basic: false,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp: 150,
            energy_kind: None,
            provides: vec![],
            trainer_kind: None,
            is_ex: true,
            is_star: false,
            is_delta: true,
            stage: Stage::Stage2,
            types: vec![Type::Fire],
            weakness: None,
            resistance: None,
            retreat_cost: Some(2),
            trainer_effect: None,
            evolves_from: Some("Kirlia".to_string()),
            attacks: vec![
                create_attack("Flame Ball", 80, Type::Fire, 3, vec![Type::Fire, Type::Colorless, Type::Colorless]),
            ],
            card_type: "Pokemon".to_string(),
            delta_species: true,
        }),

        // DF-017 Jynx δ - Basic Psychic - HP 60
        "df-017-jynx" => Some(CardMeta {
            name: "Jynx δ".to_string(),
            is_basic: true,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp: 60,
            energy_kind: None,
            provides: vec![],
            trainer_kind: None,
            is_ex: false,
            is_star: false,
            is_delta: true,
            stage: Stage::Basic,
            types: vec![Type::Psychic],
            weakness: None,
            resistance: None,
            retreat_cost: Some(1),
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![
                create_attack("Psybeam", 20, Type::Psychic, 2, vec![Type::Psychic, Type::Colorless]),
            ],
            card_type: "Pokemon".to_string(),
            delta_species: true,
        }),

        // DF-070 Vulpix δ - Basic Fire - HP 50
        "df-070-vulpix" => Some(CardMeta {
            name: "Vulpix δ".to_string(),
            is_basic: true,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp: 50,
            energy_kind: None,
            provides: vec![],
            trainer_kind: None,
            is_ex: false,
            is_star: false,
            is_delta: true,
            stage: Stage::Basic,
            types: vec![Type::Fire],
            weakness: None,
            resistance: None,
            retreat_cost: Some(1),
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![
                create_attack("Will-o'-the-wisp", 20, Type::Fire, 1, vec![Type::Fire]),
            ],
            card_type: "Pokemon".to_string(),
            delta_species: true,
        }),

        // DF-008 Ninetales δ - Stage 1 Fire - HP 80
        "df-008-ninetales" => Some(CardMeta {
            name: "Ninetales δ".to_string(),
            is_basic: false,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp: 80,
            energy_kind: None,
            provides: vec![],
            trainer_kind: None,
            is_ex: false,
            is_star: false,
            is_delta: true,
            stage: Stage::Stage1,
            types: vec![Type::Fire],
            weakness: None,
            resistance: None,
            retreat_cost: Some(1),
            trainer_effect: None,
            evolves_from: Some("Vulpix".to_string()),
            attacks: vec![
                create_attack("Fire Blast", 60, Type::Fire, 3, vec![Type::Fire, Type::Fire, Type::Colorless]),
            ],
            card_type: "Pokemon".to_string(),
            delta_species: true,
        }),

        // DF-010 Snorlax δ - Basic Colorless - HP 90
        "df-010-snorlax" => Some(CardMeta {
            name: "Snorlax δ".to_string(),
            is_basic: true,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp: 90,
            energy_kind: None,
            provides: vec![],
            trainer_kind: None,
            is_ex: false,
            is_star: false,
            is_delta: true,
            stage: Stage::Basic,
            types: vec![Type::Colorless],
            weakness: None,
            resistance: None,
            retreat_cost: Some(3),
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![
                create_attack("Claw Swipe", 30, Type::Colorless, 2, vec![Type::Colorless, Type::Colorless]),
            ],
            card_type: "Pokemon".to_string(),
            delta_species: true,
        }),

        // DF-068 Trapinch δ - Basic Psychic - HP 50
        "df-068-trapinch" => Some(CardMeta {
            name: "Trapinch δ".to_string(),
            is_basic: true,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp: 50,
            energy_kind: None,
            provides: vec![],
            trainer_kind: None,
            is_ex: false,
            is_star: false,
            is_delta: true,
            stage: Stage::Basic,
            types: vec![Type::Psychic],
            weakness: None,
            resistance: None,
            retreat_cost: Some(1),
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![
                create_attack("Bite", 10, Type::Colorless, 1, vec![Type::Colorless]),
            ],
            card_type: "Pokemon".to_string(),
            delta_species: true,
        }),

        // DF-024 Vibrava δ - Stage 1 Psychic - HP 70
        "df-024-vibrava" => Some(CardMeta {
            name: "Vibrava δ".to_string(),
            is_basic: false,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp: 70,
            energy_kind: None,
            provides: vec![],
            trainer_kind: None,
            is_ex: false,
            is_star: false,
            is_delta: true,
            stage: Stage::Stage1,
            types: vec![Type::Psychic],
            weakness: None,
            resistance: None,
            retreat_cost: Some(1),
            trainer_effect: None,
            evolves_from: Some("Trapinch".to_string()),
            attacks: vec![
                create_attack("Quick Attack", 20, Type::Colorless, 1, vec![Type::Colorless]),
                create_attack("Dragon Beat", 40, Type::Psychic, 2, vec![Type::Psychic, Type::Colorless]),
            ],
            card_type: "Pokemon".to_string(),
            delta_species: true,
        }),

        // DF-092 Flygon ex δ - Stage 2 Psychic - HP 150
        "df-092-flygon-ex" => Some(CardMeta {
            name: "Flygon ex δ".to_string(),
            is_basic: false,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp: 150,
            energy_kind: None,
            provides: vec![],
            trainer_kind: None,
            is_ex: true,
            is_star: false,
            is_delta: true,
            stage: Stage::Stage2,
            types: vec![Type::Psychic],
            weakness: None,
            resistance: None,
            retreat_cost: Some(2),
            trainer_effect: None,
            evolves_from: Some("Vibrava".to_string()),
            attacks: vec![
                create_attack("Psychic Pulse", 80, Type::Psychic, 3, vec![Type::Psychic, Type::Psychic, Type::Colorless]),
            ],
            card_type: "Pokemon".to_string(),
            delta_species: true,
        }),

        // DF-009 Pinsir δ - Basic Grass - HP 70
        "df-009-pinsir" => Some(CardMeta {
            name: "Pinsir δ".to_string(),
            is_basic: true,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp: 70,
            energy_kind: None,
            provides: vec![],
            trainer_kind: None,
            is_ex: false,
            is_star: false,
            is_delta: true,
            stage: Stage::Basic,
            types: vec![Type::Grass],
            weakness: None,
            resistance: None,
            retreat_cost: Some(2),
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![
                create_attack("Grip and Squeeze", 50, Type::Grass, 3, vec![Type::Grass, Type::Grass, Type::Colorless]),
            ],
            card_type: "Pokemon".to_string(),
            delta_species: true,
        }),

        // DF-003 Heracross δ - Basic Grass - HP 80
        "df-003-heracross" => Some(CardMeta {
            name: "Heracross δ".to_string(),
            is_basic: true,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp: 80,
            energy_kind: None,
            provides: vec![],
            trainer_kind: None,
            is_ex: false,
            is_star: false,
            is_delta: true,
            stage: Stage::Basic,
            types: vec![Type::Grass],
            weakness: None,
            resistance: None,
            retreat_cost: Some(2),
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![
                create_attack("Megahorn", 60, Type::Grass, 3, vec![Type::Grass, Type::Grass, Type::Colorless]),
            ],
            card_type: "Pokemon".to_string(),
            delta_species: true,
        }),

        // DF-082 TV Reporter - Trainer/Supporter
        "df-082-tv-reporter" => Some(CardMeta {
            name: "TV Reporter".to_string(),
            is_basic: false,
            is_tool: false,
            is_stadium: false,
            is_pokemon: false,
            is_energy: false,
            hp: 0,
            energy_kind: None,
            provides: vec![],
            trainer_kind: Some("Supporter".to_string()),
            is_ex: false,
            is_star: false,
            is_delta: false,
            stage: Stage::Basic,
            types: vec![],
            weakness: None,
            resistance: None,
            retreat_cost: None,
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![],
            card_type: "Trainer".to_string(),
            delta_species: false,
        }),

        // DF-079 Prof. Elm's Training Method - Trainer
        "df-079-prof-elms-training" => Some(CardMeta {
            name: "Prof. Elm's Training Method".to_string(),
            is_basic: false,
            is_tool: false,
            is_stadium: false,
            is_pokemon: false,
            is_energy: false,
            hp: 0,
            energy_kind: None,
            provides: vec![],
            trainer_kind: Some("Trainer".to_string()),
            is_ex: false,
            is_star: false,
            is_delta: false,
            stage: Stage::Basic,
            types: vec![],
            weakness: None,
            resistance: None,
            retreat_cost: None,
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![],
            card_type: "Trainer".to_string(),
            delta_species: false,
        }),

        // DF-072 Buffer Piece - Pokemon Tool
        "df-072-buffer-piece" => Some(CardMeta {
            name: "Buffer Piece".to_string(),
            is_basic: false,
            is_tool: true,
            is_stadium: false,
            is_pokemon: false,
            is_energy: false,
            hp: 0,
            energy_kind: None,
            provides: vec![],
            trainer_kind: Some("Pokemon Tool".to_string()),
            is_ex: false,
            is_star: false,
            is_delta: false,
            stage: Stage::Basic,
            types: vec![],
            weakness: None,
            resistance: None,
            retreat_cost: None,
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![],
            card_type: "Trainer".to_string(),
            delta_species: false,
        }),

        _ => None,
    }
}

fn extract_card_id(text: &str) -> Option<CardInstanceId> {
    // Extract "card_id": <number> from JSON-like text.
    let key = "\"card_id\"";
    let start = text.find(key)?;
    let after = &text[start + key.len()..];
    let colon = after.find(':')?;
    let mut rest = after[colon + 1..].trim();
    if rest.starts_with('"') {
        rest = &rest[1..];
    }
    let mut digits = String::new();
    for ch in rest.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else {
            break;
        }
    }
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u64>().ok().map(CardInstanceId::new)
}

fn build_card_meta_map(deck1: &[String], deck2: &[String]) -> CardMetaMap {
    let mut meta_map = CardMetaMap::new();

    // Process both decks
    for def_id in deck1.iter().chain(deck2.iter()) {
        let card_def_id = CardDefId::new(def_id.clone());
        if !meta_map.contains_key(&card_def_id) {
            if let Some(meta) = get_card_meta(def_id) {
                meta_map.insert(card_def_id, meta);
            } else {
                // Create a generic fallback for unknown cards
                meta_map.insert(card_def_id, CardMeta {
                    name: def_id.clone(),
                    is_basic: true,
                    is_tool: false,
                    is_stadium: false,
                    is_pokemon: true,
                    is_energy: false,
                    hp: 50,
                    energy_kind: None,
                    provides: vec![],
                    trainer_kind: None,
                    is_ex: false,
                    is_star: false,
                    is_delta: false,
                    stage: Stage::Basic,
                    types: vec![Type::Colorless],
                    weakness: None,
                    resistance: None,
                    retreat_cost: Some(1),
                    trainer_effect: None,
                    evolves_from: None,
                    attacks: vec![
                        create_attack("Tackle", 20, Type::Colorless, 1, vec![Type::Colorless]),
                    ],
                    card_type: "Pokemon".to_string(),
                    delta_species: false,
                });
            }
        }
    }

    meta_map
}

/// Result of a completed game.
#[pyclass]
#[derive(Clone)]
pub struct PyGameResult {
    #[pyo3(get)]
    pub winner: Option<String>,
    #[pyo3(get)]
    pub win_condition: Option<String>,
    #[pyo3(get)]
    pub turns: u32,
    #[pyo3(get)]
    pub steps: u32,
    #[pyo3(get)]
    pub p1_prizes_remaining: usize,
    #[pyo3(get)]
    pub p2_prizes_remaining: usize,
    #[pyo3(get)]
    pub end_reason: String,
    #[pyo3(get)]
    pub history: Vec<PyTurnSummary>,
    #[pyo3(get)]
    pub event_log: Vec<String>,
    #[pyo3(get)]
    pub final_state_p1: String,
    #[pyo3(get)]
    pub final_state_p2: String,
    #[pyo3(get)]
    pub final_state_compact_p1: String,
    #[pyo3(get)]
    pub final_state_compact_p2: String,
    #[pyo3(get)]
    pub summary_json: String,
}

#[pymethods]
impl PyGameResult {
    fn __repr__(&self) -> String {
        format!(
            "GameResult(winner={:?}, win_condition={:?}, turns={}, prizes=({}, {}), reason={})",
            self.winner, self.win_condition, self.turns, self.p1_prizes_remaining, self.p2_prizes_remaining, self.end_reason
        )
    }
}

#[pyclass]
#[derive(Clone)]
pub struct PyAiVsAiBatchResult {
    #[pyo3(get)]
    pub total_games: usize,
    #[pyo3(get)]
    pub p1_wins: usize,
    #[pyo3(get)]
    pub p2_wins: usize,
    #[pyo3(get)]
    pub draws: usize,
    #[pyo3(get)]
    pub mean_turns: f64,
    #[pyo3(get)]
    pub mean_steps: f64,
    /// Vec of (win_condition, count) to keep this simple to consume from Python.
    #[pyo3(get)]
    pub win_condition_counts: Vec<(String, usize)>,
    /// Optional sampled full replays (heavy payload: event_log + final states).
    #[pyo3(get)]
    pub sample_games: Vec<PyGameResult>,
}

/// Summary of a turn.
#[pyclass]
#[derive(Clone)]
pub struct PyTurnSummary {
    #[pyo3(get)]
    pub turn: u32,
    #[pyo3(get)]
    pub player: String,
    #[pyo3(get)]
    pub actions: Vec<String>,
    #[pyo3(get)]
    pub state_compact: String,
}

/// Game observation for the LLM agent.
#[pyclass]
#[derive(Clone)]
pub struct PyObservation {
    #[pyo3(get)]
    pub game_state: String,
    #[pyo3(get)]
    pub compact: String,
    #[pyo3(get)]
    pub prompt_text: String,
    #[pyo3(get)]
    pub prompt_json: String,
    #[pyo3(get)]
    pub phase: String,
    #[pyo3(get)]
    pub current_player: String,
    #[pyo3(get)]
    pub has_prompt: bool,
    #[pyo3(get)]
    pub prompt_type: Option<String>,
    #[pyo3(get)]
    pub available_actions: Vec<String>,
    #[pyo3(get)]
    pub my_prizes: usize,
    #[pyo3(get)]
    pub opp_prizes: usize,
    #[pyo3(get)]
    pub my_bench_count: usize,
    #[pyo3(get)]
    pub game_steps: u32,
    #[pyo3(get)]
    pub decision_step: u32,
}

#[derive(Serialize)]
struct CardSnapshot {
    id: u64,
    def_id: String,
    name: String,
}

#[derive(Serialize)]
struct PokemonSnapshot {
    id: u64,
    def_id: String,
    name: String,
    hp: [u16; 2],
    energy: Vec<String>,
    damage_counters: u16,
    special_conditions: Vec<String>,
}

#[derive(Serialize)]
struct SideSnapshot {
    active: Option<PokemonSnapshot>,
    bench: Vec<PokemonSnapshot>,
    hand: Vec<CardSnapshot>,
    deck_count: usize,
    prizes_remaining: usize,
}

#[derive(Serialize)]
struct OpponentSnapshot {
    active: Option<PokemonSnapshot>,
    bench: Vec<PokemonSnapshot>,
    hand_count: usize,
    deck_count: usize,
    prizes_remaining: usize,
}

#[derive(Serialize)]
struct PendingPromptSnapshot {
    required: bool,
    action_type: String,
    choices: Vec<u64>,
    details: Value,
}

#[derive(Serialize)]
struct ActionSnapshot {
    #[serde(rename = "type")]
    action_type: String,
    options: Value,
}

#[derive(Serialize)]
struct AttackSnapshot {
    name: String,
    damage: u16,
    cost: Vec<String>,
    total_energy: u8,
    effects: Vec<String>,
}

#[derive(Serialize)]
struct RulesSnapshot {
    max_bench: usize,
    energy_attach_per_turn: u8,
}

#[derive(Serialize)]
struct TurnContextSnapshot {
    turn_index: u32,
    phase: String,
    active_switch_required: bool,
}

#[derive(Serialize)]
struct SnapshotV1 {
    phase: String,
    current_player: String,
    pending_prompt: PendingPromptSnapshot,
    your_side: SideSnapshot,
    opponent_side: OpponentSnapshot,
    available_actions: Vec<ActionSnapshot>,
    attacks: Vec<AttackSnapshot>,
    rules: RulesSnapshot,
    turn_context: TurnContextSnapshot,
    game_steps: u32,
    decision_step: u32,
}

fn type_name(type_: Type) -> String {
    format!("{:?}", type_)
}

fn card_name(game: &GameState, card: &CardInstance) -> String {
    game.get_card_meta(&card.def_id)
        .map(|meta| meta.name.clone())
        .unwrap_or_else(|| card.def_id.as_str().to_string())
}

fn card_snapshot(game: &GameState, card: &CardInstance) -> CardSnapshot {
    CardSnapshot {
        id: card.id.value(),
        def_id: card.def_id.as_str().to_string(),
        name: card_name(game, card),
    }
}

fn pokemon_snapshot(game: &GameState, slot: &PokemonView) -> PokemonSnapshot {
    let energy = slot
        .attached_energy
        .iter()
        .map(|card| card.def_id.as_str().to_string())
        .collect();
    let special_conditions = slot
        .special_conditions
        .iter()
        .map(|cond| format!("{:?}", cond))
        .collect();

    PokemonSnapshot {
        id: slot.card.id.value(),
        def_id: slot.card.def_id.as_str().to_string(),
        name: card_name(game, &slot.card),
        hp: [slot.hp.saturating_sub(slot.damage_counters * 10), slot.hp],
        energy,
        damage_counters: slot.damage_counters,
        special_conditions,
    }
}

fn prompt_action_type(prompt: &Prompt) -> String {
    match prompt {
        Prompt::ChooseStartingActive { .. } => "ChooseActive".to_string(),
        Prompt::ChooseNewActive { .. } => "ChooseActive".to_string(),
        Prompt::ChooseBenchBasics { .. } => "ChooseBench".to_string(),
        Prompt::ChooseAttack { .. } => "DeclareAttack".to_string(),
        Prompt::ChooseDefenderAttack { .. } => "DeclareAttack".to_string(),
        Prompt::ChoosePokemonAttack { .. } => "DeclareAttack".to_string(),
        Prompt::ChooseAttachedEnergy { .. } => "ChooseAttachedEnergy".to_string(),
        Prompt::ChooseCardsFromDeck { .. } => "ChooseCardsFromDeck".to_string(),
        Prompt::ChooseCardsFromDiscard { .. } => "ChooseCardsFromDiscard".to_string(),
        Prompt::ChooseCardsFromHand { .. } => "ChooseCardsFromHand".to_string(),
        Prompt::ChooseCardsInPlay { .. } => "ChooseCardsInPlay".to_string(),
        Prompt::ChoosePokemonInPlay { .. } => "ChoosePokemonInPlay".to_string(),
        Prompt::ReorderDeckTop { .. } => "ReorderDeckTop".to_string(),
        Prompt::ChooseSpecialCondition { .. } => "ChooseSpecialCondition".to_string(),
        Prompt::ChoosePrizeCards { .. } => "ChoosePrizeCards".to_string(),
        _ => "UnknownPrompt".to_string(),
    }
}

fn pending_prompt_snapshot(prompt: Option<&Prompt>) -> PendingPromptSnapshot {
    if let Some(prompt) = prompt {
        let (choices, details) = match prompt {
            Prompt::ChooseStartingActive { options } => (
                options.iter().map(|id| id.value()).collect(),
                json!({ "options": options.iter().map(|id| id.value()).collect::<Vec<_>>() }),
            ),
            Prompt::ChooseNewActive { options, .. } => (
                options.iter().map(|id| id.value()).collect(),
                json!({ "options": options.iter().map(|id| id.value()).collect::<Vec<_>>() }),
            ),
            Prompt::ChooseBenchBasics { options, min, max } => (
                options.iter().map(|id| id.value()).collect(),
                json!({
                    "options": options.iter().map(|id| id.value()).collect::<Vec<_>>(),
                    "min": min,
                    "max": max,
                }),
            ),
            Prompt::ChooseAttack { attacks, .. } => (
                Vec::new(),
                json!({ "attack_names": attacks.iter().map(|a| a.name.clone()).collect::<Vec<_>>() }),
            ),
            Prompt::ChooseDefenderAttack { attacks, .. } => (
                Vec::new(),
                json!({ "attack_names": attacks.clone() }),
            ),
            Prompt::ChoosePokemonAttack { attacks, .. } => (
                Vec::new(),
                json!({ "attack_names": attacks.clone() }),
            ),
            Prompt::ChooseCardsFromDeck { count, options, min, max, destination, shuffle, .. } => (
                options.iter().map(|id| id.value()).collect(),
                json!({
                    "count": count,
                    "options": options.iter().map(|id| id.value()).collect::<Vec<_>>(),
                    "min": min,
                    "max": max,
                    "destination": format!("{:?}", destination),
                    "shuffle": shuffle,
                }),
            ),
            Prompt::ChooseCardsFromDiscard { count, options, min, max, destination, .. } => (
                options.iter().map(|id| id.value()).collect(),
                json!({
                    "count": count,
                    "options": options.iter().map(|id| id.value()).collect::<Vec<_>>(),
                    "min": min,
                    "max": max,
                    "destination": format!("{:?}", destination),
                }),
            ),
            Prompt::ChoosePokemonInPlay { options, min, max, .. } => (
                options.iter().map(|id| id.value()).collect(),
                json!({
                    "options": options.iter().map(|id| id.value()).collect::<Vec<_>>(),
                    "min": min,
                    "max": max,
                }),
            ),
            Prompt::ReorderDeckTop { options, .. } => (
                options.iter().map(|id| id.value()).collect(),
                json!({ "options": options.iter().map(|id| id.value()).collect::<Vec<_>>() }),
            ),
            Prompt::ChooseAttachedEnergy { pokemon_id, count, min, .. } => (
                vec![pokemon_id.value()],
                json!({ "pokemon_id": pokemon_id.value(), "count": count, "min": min }),
            ),
            Prompt::ChooseCardsFromHand { count, options, min, max, return_to_deck, .. } => (
                options.iter().map(|id| id.value()).collect(),
                json!({
                    "count": count,
                    "options": options.iter().map(|id| id.value()).collect::<Vec<_>>(),
                    "min": min,
                    "max": max,
                    "return_to_deck": return_to_deck,
                }),
            ),
            Prompt::ChooseCardsInPlay { options, min, max, .. } => (
                options.iter().map(|id| id.value()).collect(),
                json!({
                    "options": options.iter().map(|id| id.value()).collect::<Vec<_>>(),
                    "min": min,
                    "max": max,
                }),
            ),
            Prompt::ChooseSpecialCondition { options, .. } => (
                Vec::new(),
                json!({ "options": options.iter().map(|c| format!("{:?}", c)).collect::<Vec<_>>() }),
            ),
            Prompt::ChoosePrizeCards { options, min, max, .. } => (
                options.iter().map(|id| id.value()).collect(),
                json!({
                    "options": options.iter().map(|id| id.value()).collect::<Vec<_>>(),
                    "min": min,
                    "max": max,
                }),
            ),
            _ => (Vec::new(), json!({})),
        };

        PendingPromptSnapshot {
            required: true,
            action_type: prompt_action_type(prompt),
            choices,
            details,
        }
    } else {
        PendingPromptSnapshot {
            required: false,
            action_type: String::new(),
            choices: Vec::new(),
            details: json!({}),
        }
    }
}

fn create_observation(
    game: &GameState,
    view: &GameView,
    energy_attached_this_turn: bool,
    game_steps: u32,
    decision_step: u32,
) -> PyObservation {
    let hints = &view.action_hints;
    let mut available_actions = Vec::new();

    // Only show actions valid during Main phase
    let is_main_phase = matches!(view.phase, tcg_rules_ex::Phase::Main);

    // Count bench Pokemon (max 5 allowed)
    let bench_count = view.my_bench.len();
    let bench_full = bench_count >= 5;

    // Only show PlayBasic if bench is not full
    if is_main_phase && !bench_full && !hints.playable_basic_ids.is_empty() {
        available_actions.push(format!("PlayBasic ({})", hints.playable_basic_ids.len()));
    }
    // Only show AttachEnergy if not already attached this turn
    if is_main_phase && !energy_attached_this_turn && !hints.playable_energy_ids.is_empty() {
        available_actions.push(format!("AttachEnergy ({})", hints.playable_energy_ids.len()));
    }
    if is_main_phase && !hints.playable_evolution_ids.is_empty() {
        available_actions.push(format!("Evolve ({})", hints.playable_evolution_ids.len()));
    }
    if is_main_phase && !hints.playable_trainer_ids.is_empty() {
        available_actions.push(format!("PlayTrainer ({})", hints.playable_trainer_ids.len()));
    }
    if hints.can_declare_attack {
        available_actions.push(format!("Attack ({})", hints.usable_attacks.len()));
    }
    if hints.can_end_turn {
        available_actions.push("EndTurn".to_string());
    }

    let current_player = if view.pending_prompt.is_some() {
        "P1".to_string()
    } else {
        format!("{:?}", view.current_player)
    };

    let prompt_snapshot = pending_prompt_snapshot(view.pending_prompt.as_ref());

    let mut normalized_actions: Vec<ActionSnapshot> = Vec::new();
    if view.pending_prompt.is_some() {
        normalized_actions.push(ActionSnapshot {
            action_type: prompt_snapshot.action_type.clone(),
            options: prompt_snapshot.details.clone(),
        });
    } else {
        if is_main_phase && !bench_full && !hints.playable_basic_ids.is_empty() {
            normalized_actions.push(ActionSnapshot {
                action_type: "PlayBasic".to_string(),
                options: json!({ "card_ids": hints.playable_basic_ids.iter().map(|id| id.value()).collect::<Vec<_>>() }),
            });
        }
        if is_main_phase && !energy_attached_this_turn && !hints.playable_energy_ids.is_empty() {
            normalized_actions.push(ActionSnapshot {
                action_type: "AttachEnergy".to_string(),
                options: json!({
                    "energy_ids": hints.playable_energy_ids.iter().map(|id| id.value()).collect::<Vec<_>>(),
                    "targets": hints.attach_targets.iter().map(|id| id.value()).collect::<Vec<_>>(),
                }),
            });
        }
        if is_main_phase && !hints.playable_evolution_ids.is_empty() {
            let mut evolve_targets: Map<String, Value> = Map::new();
            for (card_id, targets) in &hints.evolve_targets_by_card_id {
                evolve_targets.insert(
                    card_id.value().to_string(),
                    json!(targets.iter().map(|id| id.value()).collect::<Vec<_>>()),
                );
            }
            normalized_actions.push(ActionSnapshot {
                action_type: "Evolve".to_string(),
                options: json!({
                    "card_ids": hints.playable_evolution_ids.iter().map(|id| id.value()).collect::<Vec<_>>(),
                    "targets_by_card_id": evolve_targets,
                }),
            });
        }
        if is_main_phase && !hints.playable_trainer_ids.is_empty() {
            normalized_actions.push(ActionSnapshot {
                action_type: "PlayTrainer".to_string(),
            options: json!({ "card_ids": hints.playable_trainer_ids.iter().map(|id| id.value()).collect::<Vec<_>>() }),
            });
        }
        if hints.can_declare_attack {
            let attack_names: Vec<String> = if !hints.usable_attacks.is_empty() {
                hints.usable_attacks.iter().map(|a| a.name.clone()).collect()
            } else if let Some(active) = view.my_active.as_ref() {
                game.get_card_meta(&active.card.def_id)
                    .map(|meta| meta.attacks.iter().map(|a| a.name.clone()).collect())
                    .unwrap_or_else(Vec::new)
            } else {
                Vec::new()
            };
            normalized_actions.push(ActionSnapshot {
                action_type: "DeclareAttack".to_string(),
                options: json!({ "attack_names": attack_names }),
            });
        }
        if hints.can_end_turn {
            normalized_actions.push(ActionSnapshot {
                action_type: "EndTurn".to_string(),
                options: json!({}),
            });
        }
    }

    let attacks: Vec<AttackSnapshot> = if let Some(active) = view.my_active.as_ref() {
        game.get_card_meta(&active.card.def_id)
            .map(|meta| meta.attacks.clone())
            .unwrap_or_else(|| hints.usable_attacks.clone())
            .into_iter()
            .map(|attack| AttackSnapshot {
                name: attack.name,
                damage: attack.damage,
                cost: attack.cost.types.iter().map(|t| type_name(*t)).collect(),
                total_energy: attack.cost.total_energy,
                effects: attack
                    .effect_ast
                    .as_ref()
                    .map(|ast| vec![format!("{:?}", ast)])
                    .unwrap_or_default(),
            })
            .collect()
    } else {
        Vec::new()
    };

    let snapshot = SnapshotV1 {
        phase: format!("{:?}", view.phase),
        current_player: current_player.clone(),
        pending_prompt: prompt_snapshot,
        your_side: SideSnapshot {
            active: view.my_active.as_ref().map(|slot| pokemon_snapshot(game, slot)),
            bench: view.my_bench.iter().map(|slot| pokemon_snapshot(game, slot)).collect(),
            hand: view.my_hand.iter().map(|card| card_snapshot(game, card)).collect(),
            deck_count: view.my_deck_count,
            prizes_remaining: view.my_prizes_count,
        },
        opponent_side: OpponentSnapshot {
            active: view.opponent_active.as_ref().map(|slot| pokemon_snapshot(game, slot)),
            bench: view.opponent_bench.iter().map(|slot| pokemon_snapshot(game, slot)).collect(),
            hand_count: view.opponent_hand_count,
            deck_count: view.opponent_deck_count,
            prizes_remaining: view.opponent_prizes_count,
        },
        available_actions: normalized_actions,
        attacks,
        rules: RulesSnapshot {
            max_bench: 5,
            energy_attach_per_turn: game.rules.energy_attachment_limit_per_turn(),
        },
        turn_context: TurnContextSnapshot {
            turn_index: game.turn.number,
            phase: format!("{:?}", view.phase),
            active_switch_required: matches!(view.pending_prompt, Some(Prompt::ChooseNewActive { .. })),
        },
        game_steps,
        decision_step,
    };

    let prompt_text = render_game_view(view);
    let prompt_json = serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string());

    PyObservation {
        game_state: prompt_text.clone(),
        compact: render_game_view_compact(view),
        prompt_text,
        prompt_json,
        phase: format!("{:?}", view.phase),
        current_player,
        has_prompt: view.pending_prompt.is_some(),
        prompt_type: view
            .pending_prompt
            .as_ref()
            .map(|p| format!("{:?}", p).split('{').next().unwrap_or("Unknown").trim().to_string()),
        available_actions,
        my_prizes: view.my_prizes_count,
        opp_prizes: view.opponent_prizes_count,
        my_bench_count: bench_count,
        game_steps,
        decision_step,
    }
}

/// A Pokemon TCG game that can be played step by step.
#[pyclass]
pub struct PtcgGame {
    game: GameState,
    v4_ai: RandomAiV4,
    steps: u32,
    turns: u32,
    decision_steps: u32,
    max_steps: u32,
    history: Vec<PyTurnSummary>,
    current_turn_actions: Vec<String>,
    game_over: bool,
    winner: Option<PlayerId>,
    last_turn_player: PlayerId,
    energy_attached_this_turn: bool,
}

#[pymethods]
impl PtcgGame {
    /// Create a new game with the given decks and seeds.
    #[new]
    #[pyo3(signature = (p1_deck, p2_deck, game_seed=42, ai_seed=123, max_steps=1000))]
    fn new(
        p1_deck: Vec<String>,
        p2_deck: Vec<String>,
        game_seed: u64,
        ai_seed: u64,
        max_steps: u32,
    ) -> PyResult<Self> {
        // Build card metadata map with attacks
        let card_meta_map = build_card_meta_map(&p1_deck, &p2_deck);

        // Create decks from card IDs
        let deck1 = p1_deck.iter()
            .enumerate()
            .map(|(_i, def_id)| CardInstance::new(
                CardDefId::new(def_id.clone()),
                PlayerId::P1,
            ))
            .collect();

        let deck2 = p2_deck.iter()
            .enumerate()
            .map(|(_i, def_id)| CardInstance::new(
                CardDefId::new(def_id.clone()),
                PlayerId::P2,
            ))
            .collect();

        // Use new_with_card_meta to include attack definitions
        let game = GameState::new_with_card_meta(deck1, deck2, game_seed, RulesetConfig::default(), card_meta_map);
        let v4_ai = RandomAiV4::new(ai_seed);

        Ok(Self {
            game,
            v4_ai,
            steps: 0,
            turns: 0,
            decision_steps: 0,
            max_steps,
            history: Vec::new(),
            current_turn_actions: Vec::new(),
            game_over: false,
            winner: None,
            last_turn_player: PlayerId::P1,
            energy_attached_this_turn: false,
        })
    }

    /// Get the current observation for player 1 (the LLM agent).
    fn get_observation(&self) -> PyObservation {
        let view = self.game.view_for_player(PlayerId::P1);
        create_observation(
            &self.game,
            &view,
            self.energy_attached_this_turn,
            self.steps,
            self.decision_steps,
        )
    }

    /// Check if it's the LLM agent's turn (P1).
    fn is_agent_turn(&self) -> bool {
        self.game.turn.player == PlayerId::P1
    }

    /// Check if the game is over.
    fn is_game_over(&self) -> bool {
        self.game_over
    }

    /// Get the winner if game is over.
    fn get_winner(&self) -> Option<String> {
        self.winner.map(|w| format!("{:?}", w))
    }

    /// Step the game forward, handling AI turns automatically.
    /// Returns the new observation for P1.
    fn step(&mut self) -> PyResult<PyObservation> {
        if self.game_over {
            return Ok(self.get_observation());
        }
        let _ = step_internal(self)?;
        let obs = self.get_observation();
        if !self.game_over {
            self.decision_steps = self.decision_steps.saturating_add(1);
        }
        Ok(obs)
    }

    /// Submit an action from the LLM agent (P1).
    /// The action should be a JSON string like: {"action": "EndTurn"}
    fn submit_action(&mut self, action_json: &str) -> PyResult<bool> {
        if self.game_over {
            return Err(PyRuntimeError::new_err("Game is already over"));
        }

        let view = self.game.view_for_player(PlayerId::P1);
        // If a P1 prompt exists in game state but isn't surfaced in the view,
        // handle ChooseNewActive directly to avoid InvalidPrompt.
        if view.pending_prompt.is_none() {
            if let Some(pending) = &self.game.pending_prompt {
                if pending.for_player == PlayerId::P1 {
                    if let Prompt::ChooseNewActive { options, .. } = &pending.prompt {
                        if let Some(card_id) = extract_card_id(action_json) {
                            if options.contains(&card_id) {
                                self.game.apply_action(
                                    PlayerId::P1,
                                    Action::ChooseNewActive { card_id },
                                ).map_err(|e| PyRuntimeError::new_err(format!("Action failed: {:?}", e)))?;
                                self.game.pending_prompt = None;
                                return Ok(true);
                            }
                        }
                    }
                }
            }
        }

        // Parse the action
        let parsed = match parse_response(action_json, &view) {
            Ok(p) => p,
            Err(e) => {
                return Err(PyRuntimeError::new_err(format!(
                    "Failed to parse action: {:?}",
                    e
                )));
            }
        };
        // Check if this is an energy attachment
        let is_energy_attach = action_json.contains("AttachEnergy");

        // Apply the action
        match self.game.apply_action(PlayerId::P1, parsed.action) {
            Ok(_) => {
                self.current_turn_actions.push(action_json.to_string());
                if is_energy_attach {
                    self.energy_attached_this_turn = true;
                }
                self.game.pending_prompt = None;
                Ok(true)
            }
            Err(e) => {
                // If prompt mismatch, try a ChooseNewActive fallback.
                if let Some(pending) = &self.game.pending_prompt {
                    if let Prompt::ChooseNewActive { options, .. } = &pending.prompt {
                        if let Some(card_id) = extract_card_id(action_json) {
                            if options.contains(&card_id) {
                                self.game.apply_action(
                                    PlayerId::P1,
                                    Action::ChooseNewActive { card_id },
                                ).map_err(|err| PyRuntimeError::new_err(format!("Action failed: {:?}", err)))?;
                                self.game.pending_prompt = None;
                                return Ok(true);
                            }
                        }
                    }
                }
                Err(PyRuntimeError::new_err(format!("Action failed: {:?}", e)))
            }
        }
    }

    /// Run the game until it needs P1 input or ends.
    /// Returns observation when P1 action is needed, or final observation.
    fn run_until_agent_turn(&mut self) -> PyResult<PyObservation> {
        loop {
            if self.game_over {
                break;
            }

            // P1 prompt (e.g. setup) always takes priority.
            let p1_view = self.game.view_for_player(PlayerId::P1);
            if p1_view.pending_prompt.is_some() {
                break;
            }

            // Only return during P1's actionable phases (including setup prompts).
            if p1_view.current_player == PlayerId::P1
                && matches!(
                    p1_view.phase,
                    tcg_rules_ex::Phase::Setup
                        | tcg_rules_ex::Phase::Main
                        | tcg_rules_ex::Phase::Attack
                )
            {
                break;
            }

            // Step once and handle P2 AI internally.
            let result = step_internal(self)?;

            // If we hit a P1 prompt, return control immediately.
            if let StepResult::Prompt { for_player, .. } = result {
                if for_player == PlayerId::P1 {
                    break;
                }
            }
        }

        Ok(self.get_observation())
    }

    /// Get the final game result.
    fn get_result(&self) -> PyGameResult {
        let end_reason = if self.winner.is_some() {
            "GameOver"
        } else if self.steps >= self.max_steps {
            "MaxSteps"
        } else {
            "InProgress"
        };

        let winner = self.winner.map(|w| format!("{:?}", w));
        let win_condition = self
            .game
            .event_log
            .iter()
            .rev()
            .find_map(|evt| match evt {
                tcg_core::GameEvent::GameEnded { reason, .. } => Some(format!("{:?}", reason)),
                _ => None,
            });

        let summary_json = serde_json::to_string(&json!({
            "winner": winner.clone(),
            "end_reason": end_reason,
            "win_condition": win_condition.clone(),
            "p1_prizes_remaining": self.game.view_for_player(PlayerId::P1).my_prizes_count,
            "p2_prizes_remaining": self.game.view_for_player(PlayerId::P2).my_prizes_count,
            "p1_damage_dealt": 0,
            "p2_damage_dealt": 0
        }))
        .unwrap_or_else(|_| "{}".to_string());

        PyGameResult {
            winner,
            win_condition,
            turns: self.turns,
            steps: self.steps,
            p1_prizes_remaining: self.game.view_for_player(PlayerId::P1).my_prizes_count,
            p2_prizes_remaining: self.game.view_for_player(PlayerId::P2).my_prizes_count,
            end_reason: end_reason.to_string(),
            history: self.history.clone(),
            event_log: self.game.event_log.iter().map(|e| format!("{:?}", e)).collect(),
            final_state_p1: render_game_view(&self.game.view_for_player(PlayerId::P1)),
            final_state_p2: render_game_view(&self.game.view_for_player(PlayerId::P2)),
            final_state_compact_p1: render_game_view_compact(&self.game.view_for_player(PlayerId::P1)),
            final_state_compact_p2: render_game_view_compact(&self.game.view_for_player(PlayerId::P2)),
            summary_json,
        }
    }
}

/// Internal step helper that returns the core StepResult.
fn step_internal(game: &mut PtcgGame) -> PyResult<StepResult> {
    // Track turn changes - detect when player changes
    let current_player = game.game.turn.player;
    if current_player != game.last_turn_player {
        game.turns += 1;
        game.last_turn_player = current_player;
        game.energy_attached_this_turn = false;
    }

    // Run one game step
    let result = game.game.step();
    game.steps += 1;

    match &result {
        StepResult::GameOver { winner } => {
            game.game_over = true;
            game.winner = Some(*winner);
        }
        StepResult::Prompt { prompt, for_player } => {
            game.game.pending_prompt = Some(PendingPrompt {
                for_player: *for_player,
                prompt: prompt.clone(),
            });
            if *for_player == PlayerId::P2 {
                // AI handles prompt - apply ONE action and break
                // (matching the behavior in run_ai_vs_ai)
                let view = game.game.view_for_player(PlayerId::P2);
                let actions = game.v4_ai.propose_prompt_response(&view, prompt);
                for action in actions {
                    if game.game.apply_action(PlayerId::P2, action).is_ok() {
                        break; // Only apply one action per step
                    }
                }
                game.game.pending_prompt = None;
            }
            // If P1 prompt, caller should handle it.
        }
        StepResult::Continue => {
            // Check for AI free actions - P2 is the AI opponent
            if game.game.turn.player == PlayerId::P2 {
                let view = game.game.view_for_player(PlayerId::P2);
                if view.pending_prompt.is_none()
                    && matches!(
                        view.phase,
                        tcg_rules_ex::Phase::Main | tcg_rules_ex::Phase::Attack
                    )
                {
                    // Get AI's proposed actions and apply ONE at a time
                    // (matching the behavior in run_ai_vs_ai)
                    let actions = game.v4_ai.propose_free_actions(&view);
                    for action in actions {
                        if game.game.apply_action(PlayerId::P2, action).is_ok() {
                            break; // Only apply one action per step
                        }
                    }
                }
            }
        }
        StepResult::Event { .. } => {}
    }

    // Check max steps
    if game.steps >= game.max_steps {
        game.game_over = true;
    }

    Ok(result)
}

/// Run a complete game with a Python LLM callback.
///
/// Args:
///     p1_deck: List of card definition IDs for player 1's deck
///     p2_deck: List of card definition IDs for player 2's deck
///     llm_callback: Python callable that takes (observation: str) and returns action JSON
///     game_seed: Random seed for the game
///     ai_seed: Random seed for AI v4
///     max_steps: Maximum game steps before stopping
///
/// Returns:
///     GameResult with winner, turns, etc.
#[pyfunction]
#[pyo3(signature = (p1_deck, p2_deck, llm_callback, game_seed=42, ai_seed=123, max_steps=1000))]
fn run_game_vs_ai(
    py: Python<'_>,
    p1_deck: Vec<String>,
    p2_deck: Vec<String>,
    llm_callback: PyObject,
    game_seed: u64,
    ai_seed: u64,
    max_steps: u32,
) -> PyResult<PyGameResult> {
    let mut game = PtcgGame::new(p1_deck, p2_deck, game_seed, ai_seed, max_steps)?;

    while !game.is_game_over() {
        // Run until agent needs to act
        let obs = game.run_until_agent_turn()?;

        if game.is_game_over() {
            break;
        }

        // Call LLM callback with observation
        let action_json: String = llm_callback.call1(py, (obs.game_state.clone(),))?.extract(py)?;

        // Submit action
        match game.submit_action(&action_json) {
            Ok(_) => {}
            Err(e) => {
                // On error, try EndTurn as fallback
                let _ = game.submit_action(r#"{"action": "EndTurn"}"#);
            }
        }

        // Step to process the action
        game.step()?;
    }

    Ok(game.get_result())
}

/// Run a complete deterministic game between two AI v4 players.
///
/// This is used by deckbuilder to evaluate a candidate deck against opponents
/// without any LLM/interceptor involvement.
#[pyfunction]
#[pyo3(signature = (p1_deck, p2_deck, game_seed=42, p1_ai_seed=123, p2_ai_seed=456, max_steps=1000))]
fn run_ai_vs_ai(
    p1_deck: Vec<String>,
    p2_deck: Vec<String>,
    game_seed: u64,
    p1_ai_seed: u64,
    p2_ai_seed: u64,
    max_steps: u32,
) -> PyResult<PyGameResult> {
    // Build card metadata map (attacks/energy/trainer info) for both decks
    let card_meta_map = build_card_meta_map(&p1_deck, &p2_deck);

    // Create decks from card IDs
    let deck1: Vec<CardInstance> = p1_deck
        .iter()
        .map(|def_id| CardInstance::new(CardDefId::new(def_id.clone()), PlayerId::P1))
        .collect();

    let deck2: Vec<CardInstance> = p2_deck
        .iter()
        .map(|def_id| CardInstance::new(CardDefId::new(def_id.clone()), PlayerId::P2))
        .collect();

    let mut game = GameState::new_with_card_meta(deck1, deck2, game_seed, RulesetConfig::default(), card_meta_map);
    let mut p1_ai = RandomAiV4::new(p1_ai_seed);
    let mut p2_ai = RandomAiV4::new(p2_ai_seed);

    let mut steps: u32 = 0;
    let mut turns: u32 = 0;
    let mut winner: Option<PlayerId> = None;
    let mut last_turn = game.turn.number;

    while steps < max_steps {
        // Track turn changes
        if game.turn.number != last_turn {
            turns += 1;
            last_turn = game.turn.number;
        }

        let result = game.step();
        steps += 1;

        match result {
            StepResult::GameOver { winner: w } => {
                winner = Some(w);
                break;
            }
            StepResult::Prompt { prompt, for_player } => {
                let view = game.view_for_player(for_player);
                let actions = if for_player == PlayerId::P1 {
                    p1_ai.propose_prompt_response(&view, &prompt)
                } else {
                    p2_ai.propose_prompt_response(&view, &prompt)
                };

                let mut applied = false;
                for action in actions {
                    if game.apply_action(for_player, action).is_ok() {
                        applied = true;
                        break;
                    }
                }
                if !applied {
                    return Err(PyRuntimeError::new_err(
                        "AI prompt response failed (all candidate actions invalid)",
                    ));
                }
            }
            StepResult::Continue => {
                // Give the current player one free action in Main/Attack phases.
                let current = game.turn.player;
                let view = game.view_for_player(current);
                if matches!(view.phase, tcg_rules_ex::Phase::Main | tcg_rules_ex::Phase::Attack) {
                    let actions = if current == PlayerId::P1 {
                        p1_ai.propose_free_actions(&view)
                    } else {
                        p2_ai.propose_free_actions(&view)
                    };

                    let mut applied = false;
                    for action in actions {
                        if game.apply_action(current, action).is_ok() {
                            applied = true;
                            break;
                        }
                    }
                    if !applied {
                        // If the AI proposed only invalid actions, just continue stepping.
                    }
                }
            }
            StepResult::Event { .. } => {}
        }
    }

    let end_reason = if winner.is_some() {
        "GameOver"
    } else if steps >= max_steps {
        "MaxSteps"
    } else {
        "InProgress"
    };

    let winner_str = winner.map(|w| format!("{:?}", w));
    let win_condition = game
        .event_log
        .iter()
        .rev()
        .find_map(|evt| match evt {
            tcg_core::GameEvent::GameEnded { reason, .. } => Some(format!("{:?}", reason)),
            _ => None,
        });

    let summary_json = serde_json::to_string(&json!({
        "winner": winner_str.clone(),
        "end_reason": end_reason,
        "win_condition": win_condition.clone(),
        "p1_prizes_remaining": game.view_for_player(PlayerId::P1).my_prizes_count,
        "p2_prizes_remaining": game.view_for_player(PlayerId::P2).my_prizes_count,
        "p1_damage_dealt": 0,
        "p2_damage_dealt": 0
    }))
    .unwrap_or_else(|_| "{}".to_string());

    Ok(PyGameResult {
        winner: winner_str,
        win_condition,
        turns,
        steps,
        p1_prizes_remaining: game.view_for_player(PlayerId::P1).my_prizes_count,
        p2_prizes_remaining: game.view_for_player(PlayerId::P2).my_prizes_count,
        end_reason: end_reason.to_string(),
        history: Vec::new(),
        event_log: game.event_log.iter().map(|e| format!("{:?}", e)).collect(),
        final_state_p1: render_game_view(&game.view_for_player(PlayerId::P1)),
        final_state_p2: render_game_view(&game.view_for_player(PlayerId::P2)),
        final_state_compact_p1: render_game_view_compact(&game.view_for_player(PlayerId::P1)),
        final_state_compact_p2: render_game_view_compact(&game.view_for_player(PlayerId::P2)),
        summary_json,
    })
}

fn run_ai_vs_ai_minimal(
    p1_deck: &[String],
    p2_deck: &[String],
    game_seed: u64,
    p1_ai_seed: u64,
    p2_ai_seed: u64,
    max_steps: u32,
) -> (Option<PlayerId>, Option<String>, u32, u32) {
    let card_meta_map = build_card_meta_map(p1_deck, p2_deck);
    let deck1: Vec<CardInstance> = p1_deck
        .iter()
        .map(|def_id| CardInstance::new(CardDefId::new(def_id.clone()), PlayerId::P1))
        .collect();
    let deck2: Vec<CardInstance> = p2_deck
        .iter()
        .map(|def_id| CardInstance::new(CardDefId::new(def_id.clone()), PlayerId::P2))
        .collect();

    let mut game = GameState::new_with_card_meta(deck1, deck2, game_seed, RulesetConfig::default(), card_meta_map);
    let mut p1_ai = RandomAiV4::new(p1_ai_seed);
    let mut p2_ai = RandomAiV4::new(p2_ai_seed);

    let mut steps: u32 = 0;
    let mut turns: u32 = 0;
    let mut winner: Option<PlayerId> = None;
    let mut last_turn = game.turn.number;
    let mut win_condition: Option<String> = None;

    while steps < max_steps {
        if game.turn.number != last_turn {
            turns += 1;
            last_turn = game.turn.number;
        }

        let result = game.step();
        steps += 1;

        match result {
            StepResult::GameOver { winner: w } => {
                winner = Some(w);
                win_condition = game
                    .event_log
                    .iter()
                    .rev()
                    .find_map(|evt| match evt {
                        tcg_core::GameEvent::GameEnded { reason, .. } => Some(format!("{:?}", reason)),
                        _ => None,
                    });
                break;
            }
            StepResult::Prompt { prompt, for_player } => {
                let view = game.view_for_player(for_player);
                let actions = if for_player == PlayerId::P1 {
                    p1_ai.propose_prompt_response(&view, &prompt)
                } else {
                    p2_ai.propose_prompt_response(&view, &prompt)
                };

                let mut applied = false;
                for action in actions {
                    if game.apply_action(for_player, action).is_ok() {
                        applied = true;
                        break;
                    }
                }
                if !applied {
                    break;
                }
            }
            StepResult::Continue => {
                let current = game.turn.player;
                let view = game.view_for_player(current);
                if matches!(view.phase, tcg_rules_ex::Phase::Main | tcg_rules_ex::Phase::Attack) {
                    let actions = if current == PlayerId::P1 {
                        p1_ai.propose_free_actions(&view)
                    } else {
                        p2_ai.propose_free_actions(&view)
                    };

                    for action in actions {
                        if game.apply_action(current, action).is_ok() {
                            break;
                        }
                    }
                }
            }
            StepResult::Event { .. } => {}
        }
    }

    (winner, win_condition, turns, steps)
}

/// Run many deterministic AI v4 vs AI v4 games in parallel (Rust threads).
///
/// Deterministic: results are a pure function of the provided seeds.
#[pyfunction]
#[pyo3(signature = (
    p1_deck,
    p2_deck,
    game_seeds,
    p1_ai_seeds,
    p2_ai_seeds,
    max_steps=2000,
    max_workers=0,
    sample_stride=0,
    max_samples=0
))]
fn run_ai_vs_ai_batch(
    py: Python<'_>,
    p1_deck: Vec<String>,
    p2_deck: Vec<String>,
    game_seeds: Vec<u64>,
    p1_ai_seeds: Vec<u64>,
    p2_ai_seeds: Vec<u64>,
    max_steps: u32,
    max_workers: usize,
    sample_stride: usize,
    max_samples: usize,
) -> PyResult<PyAiVsAiBatchResult> {
    let n = game_seeds.len();
    if n == 0 {
        return Err(PyRuntimeError::new_err("game_seeds must be non-empty"));
    }
    if p1_ai_seeds.len() != n || p2_ai_seeds.len() != n {
        return Err(PyRuntimeError::new_err(
            "seed arrays must have equal length: game_seeds, p1_ai_seeds, p2_ai_seeds",
        ));
    }

    let worker_count = if max_workers == 0 {
        thread::available_parallelism().map(|n| n.get()).unwrap_or(8)
    } else {
        max_workers
    }
    .max(1);

    // Deterministic sampling indices (optional).
    let mut sample_indices: Vec<usize> = Vec::new();
    if sample_stride > 0 && max_samples > 0 {
        let mut i = 0usize;
        while i < n && sample_indices.len() < max_samples {
            if i % sample_stride == 0 {
                sample_indices.push(i);
            }
            i += 1;
        }
    }

    let (stats, sample_games) = py.allow_threads(|| {
        let (tx, rx) = mpsc::channel::<(usize, Option<PlayerId>, Option<String>, u32, u32)>();

        let p1_deck_ref = &p1_deck;
        let p2_deck_ref = &p2_deck;
        let game_seeds_ref = &game_seeds;
        let p1_ai_seeds_ref = &p1_ai_seeds;
        let p2_ai_seeds_ref = &p2_ai_seeds;

        thread::scope(|scope| {
            for worker_id in 0..worker_count {
                let tx = tx.clone();
                scope.spawn(move || {
                    for idx in (worker_id..n).step_by(worker_count) {
                        let (winner, win_cond, turns, steps_taken) = run_ai_vs_ai_minimal(
                            p1_deck_ref,
                            p2_deck_ref,
                            game_seeds_ref[idx],
                            p1_ai_seeds_ref[idx],
                            p2_ai_seeds_ref[idx],
                            max_steps,
                        );
                        let _ = tx.send((idx, winner, win_cond, turns, steps_taken));
                    }
                });
            }
        });

        drop(tx);

        let mut p1_wins = 0usize;
        let mut p2_wins = 0usize;
        let mut draws = 0usize;
        let mut sum_turns: u64 = 0;
        let mut sum_steps: u64 = 0;
        let mut win_condition_counts: HashMap<String, usize> = HashMap::new();

        for (_idx, winner, win_cond, turns, steps_taken) in rx.iter() {
            match winner {
                Some(PlayerId::P1) => p1_wins += 1,
                Some(PlayerId::P2) => p2_wins += 1,
                None => draws += 1,
            }
            sum_turns += turns as u64;
            sum_steps += steps_taken as u64;
            if let Some(cond) = win_cond {
                *win_condition_counts.entry(cond).or_insert(0) += 1;
            }
        }

        let mut sample_games: Vec<PyGameResult> = Vec::new();
        for &idx in &sample_indices {
            if let Ok(full) = run_ai_vs_ai(
                p1_deck.clone(),
                p2_deck.clone(),
                game_seeds[idx],
                p1_ai_seeds[idx],
                p2_ai_seeds[idx],
                max_steps,
            ) {
                sample_games.push(full);
            }
        }

        (
            (p1_wins, p2_wins, draws, sum_turns, sum_steps, win_condition_counts),
            sample_games,
        )
    });

    let (p1_wins, p2_wins, draws, sum_turns, sum_steps, win_condition_counts) = stats;
    let mut win_condition_counts_vec: Vec<(String, usize)> = win_condition_counts.into_iter().collect();
    win_condition_counts_vec.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(PyAiVsAiBatchResult {
        total_games: n,
        p1_wins,
        p2_wins,
        draws,
        mean_turns: (sum_turns as f64) / (n as f64),
        mean_steps: (sum_steps as f64) / (n as f64),
        win_condition_counts: win_condition_counts_vec,
        sample_games,
    })
}


/// Python module for Pokemon TCG game engine.
#[pymodule]
fn tcg_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PtcgGame>()?;
    m.add_class::<PyGameResult>()?;
    m.add_class::<PyAiVsAiBatchResult>()?;
    m.add_class::<PyTurnSummary>()?;
    m.add_class::<PyObservation>()?;
    m.add_function(wrap_pyfunction!(run_game_vs_ai, m)?)?;
    m.add_function(wrap_pyfunction!(run_ai_vs_ai, m)?)?;
    m.add_function(wrap_pyfunction!(run_ai_vs_ai_batch, m)?)?;
    Ok(())
}
