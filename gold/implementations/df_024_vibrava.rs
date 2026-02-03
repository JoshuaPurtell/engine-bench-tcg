//! Vibrava δ - Dragon Frontiers #24

use tcg_core::{CardInstanceId, GameState};

pub const SET: &str = "DF";
pub const NUMBER: u32 = 24;
pub const NAME: &str = "Vibrava δ";

pub fn psychic_wing_active(game: &GameState, pokemon_id: CardInstanceId) -> bool {
    if let Some(slot) = game.find_pokemon(pokemon_id) {
        for card in &slot.attached {
            if let Some(meta) = game.card_meta(&card.def_id) {
                if meta.is_energy && meta.energy_kind.as_deref() == Some("Psychic") {
                    return true;
                }
            }
        }
    }
    false
}

pub fn psychic_wing_retreat_reduction(
    game: &GameState,
    pokemon_id: CardInstanceId,
    body_active: bool,
) -> i32 {
    if body_active && psychic_wing_active(game, pokemon_id) {
        -1 // Reduce retreat cost to 0
    } else {
        0
    }
}

pub fn quick_blow_damage(game: &mut GameState, _attacker_id: CardInstanceId) -> i32 {
    let base = 30;
    if game.flip_coin() {
        base + 20
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_card_identifiers() {
        assert_eq!(SET, "DF");
        assert_eq!(NUMBER, 24);
    }
}
