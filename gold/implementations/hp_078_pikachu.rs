//! Pikachu - Holon Phantoms #78
use tcg_core::{CardInstanceId, GameState, PlayerId, SpecialCondition, Prompt};

pub const SET: &str = "HP";
pub const NUMBER: u32 = 78;
pub const NAME: &str = "Pikachu";

pub const QUICK_ATTACK_BASE: i32 = 20;
pub const QUICK_ATTACK_BONUS: i32 = 10;

pub fn execute_thunder_wave(game: &mut GameState, attacker_id: CardInstanceId) -> bool {
    let (player, player_idx) = match find_owner(game, attacker_id) {
        Some((p, idx)) => (p, idx),
        None => return false,
    };
    let opp_idx = 1 - player_idx;
    let prompt = Prompt::CoinFlipForEffect {
            player: player,
            effect_description: "Paralyze the Defending Pokemon".to_string(),
        };
    game.set_pending_prompt(prompt, player);
    true
}

pub fn quick_attack_damage(heads: bool) -> i32 {
    if heads { QUICK_ATTACK_BASE + QUICK_ATTACK_BONUS } else { QUICK_ATTACK_BASE }
}

fn find_owner(game: &GameState, card_id: CardInstanceId) -> Option<(PlayerId, usize)> {
    if game.players[0].active.as_ref().map(|s| s.card.id == card_id).unwrap_or(false) { return Some((PlayerId::P1, 0)); }
    if game.players[1].active.as_ref().map(|s| s.card.id == card_id).unwrap_or(false) { return Some((PlayerId::P2, 1)); }
    None
}
