//! Numel - Holon Phantoms #72
use tcg_core::{CardInstanceId, GameState, PlayerId, SpecialCondition, Prompt};

pub const SET: &str = "HP";
pub const NUMBER: u32 = 72;
pub const NAME: &str = "Numel";

pub const FLARE_DAMAGE: i32 = 20;

pub fn execute_singe_burn(game: &mut GameState, attacker_id: CardInstanceId) -> bool {
    let (player, player_idx) = match find_owner(game, attacker_id) {
        Some((p, idx)) => (p, idx),
        None => return false,
    };
    let opp_idx = 1 - player_idx;
    let prompt = Prompt::CoinFlipForEffect {
            player: player,
            effect_description: "Burn the Defending Pokemon".to_string(),
        };
    game.set_pending_prompt(prompt, player);
    true
}

fn find_owner(game: &GameState, card_id: CardInstanceId) -> Option<(PlayerId, usize)> {
    if game.players[0].active.as_ref().map(|s| s.card.id == card_id).unwrap_or(false) { return Some((PlayerId::P1, 0)); }
    if game.players[1].active.as_ref().map(|s| s.card.id == card_id).unwrap_or(false) { return Some((PlayerId::P2, 1)); }
    None
}
