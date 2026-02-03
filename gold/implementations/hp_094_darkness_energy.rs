//! Darkness Energy (Special) - Holon Phantoms #94

use tcg_core::{CardInstanceId, GameState};

pub const SET: &str = "HP";
pub const NUMBER: u32 = 94;
pub const NAME: &str = "Darkness Energy";

pub const DAMAGE_BONUS: i32 = 10;

pub fn darkness_energy_bonus(game: &GameState, attacker_id: CardInstanceId) -> i32 {
    // Find attacker and check if it qualifies
    for player_idx in 0..2 {
        if let Some(active) = &game.players[player_idx].active {
            if active.card.id == attacker_id {
                let meta = game.card_meta.get(&active.card.def_id);
                if let Some(meta) = meta {
                    let name = &meta.name;
                    let types = &meta.types;
                    if qualifies_for_bonus(name, types) {
                        // Count Darkness Energy attached
                        let darkness_count = active.attached_energy.iter()
                            .filter(|e| {
                                game.card_meta.get(&e.def_id).map_or(false, |meta| {
                                    meta.energy_kind.as_deref() == Some("Special")
                                        && meta.name.contains("Darkness")
                                })
                            })
                            .count();
                        return (darkness_count as i32) * DAMAGE_BONUS;
                    }
                }
            }
        }
    }
    0
}

pub fn qualifies_for_bonus(pokemon_name: &str, pokemon_types: &[String]) -> bool {
    pokemon_name.contains("Dark") || pokemon_types.iter().any(|t| t == "darkness")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_qualifies() {
        assert!(qualifies_for_bonus("Dark Tyranitar", &[]));
        assert!(qualifies_for_bonus("Mightyena", &["darkness".to_string()]));
        assert!(!qualifies_for_bonus("Pikachu", &["lightning".to_string()]));
    }
}
