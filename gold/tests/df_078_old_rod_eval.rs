// Evaluation tests for DF-078 Old Rod

#[cfg(test)]
mod eval_tests {
    use super::*;
    #[test]
    fn eval_set_constant() { assert_eq!(SET, "DF"); }
    #[test]
    fn eval_number_constant() { assert_eq!(NUMBER, 78); }
    #[test]
    fn eval_name_constant() { assert_eq!(NAME, "Old Rod"); }
}


#[cfg(test)]
mod trainer_ast_tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn eval_old_rod_trainer_ast_custom() {
        let ast = crate::df::trainer_effect_ast("78", "Old Rod", "Item")
            .expect("expected trainer ast");
        assert_eq!(ast.get("op").and_then(Value::as_str), Some("Custom"));
        assert_eq!(ast.get("id").and_then(Value::as_str), Some("DF-78:Old Rod"));
    }
}


#[cfg(test)]
mod deep_stateful_tests {
    use super::*;
    use rand::Rng;
    use rand_chacha::ChaCha8Rng;
    use rand_core::SeedableRng;
    use tcg_core::{CardDefId, CardInstance, CardInstanceId, CardMeta, CardMetaMap, GameState, PlayerId, PokemonSlot, Prompt, SelectionDestination, Stage, Type};
    use tcg_rules_ex::RulesetConfig;

    fn create_pokemon_meta(name: &str) -> CardMeta {
        CardMeta {
            card_type: String::new(),
            delta_species: false,
            name: name.to_string(),
            is_basic: true,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp: 60,
            energy_kind: None,
            provides: Vec::new(),
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
            attacks: vec![],
        }
    }

    fn create_trainer_meta(name: &str, kind: &str) -> CardMeta {
        CardMeta {
            card_type: String::new(),
            delta_species: false,
            name: name.to_string(),
            is_basic: false,
            is_tool: false,
            is_stadium: false,
            is_pokemon: false,
            is_energy: false,
            hp: 0,
            energy_kind: None,
            provides: Vec::new(),
            trainer_kind: Some(kind.to_string()),
            is_ex: false,
            is_star: false,
            is_delta: false,
            stage: Stage::Basic,
            types: Vec::new(),
            weakness: None,
            resistance: None,
            retreat_cost: None,
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![],
        }
    }

    fn setup_game() -> (GameState, CardInstanceId) {
        let card_meta = CardMetaMap::new();
        let mut game = GameState::new_with_card_meta(
            Vec::new(),
            Vec::new(),
            12345,
            RulesetConfig::default(),
            card_meta,
        );

        let p1_def = CardDefId::new("P1-ACTIVE".to_string());
        game.card_meta.insert(p1_def.clone(), create_pokemon_meta("P1 Active"));
        let p1_card = CardInstance::new(p1_def, PlayerId::P1);
        game.players[0].active = Some(PokemonSlot::new(p1_card));

        let p2_def = CardDefId::new("P2-ACTIVE".to_string());
        game.card_meta.insert(p2_def.clone(), create_pokemon_meta("P2 Active"));
        let p2_card = CardInstance::new(p2_def, PlayerId::P2);
        game.players[1].active = Some(PokemonSlot::new(p2_card));

        game.turn.player = PlayerId::P1;

        let source_id = game.players[0].active.as_ref().unwrap().card.id;
        (game, source_id)
    }

    fn add_to_discard(game: &mut GameState, def_id: &str, meta: CardMeta) -> CardInstanceId {
        let card_def_id = CardDefId::new(def_id.to_string());
        game.card_meta.insert(card_def_id.clone(), meta);
        let card = CardInstance::new(card_def_id, PlayerId::P1);
        game.players[0].discard.add(card.clone());
        card.id
    }

    #[test]
    fn eval_old_rod_branches_on_coin_flips() {
        let (mut game, source_id) = setup_game();
        let pokemon_id = add_to_discard(&mut game, "P1-DISCARD-POKEMON", create_pokemon_meta("Discard Pokemon"));
        let trainer_id = add_to_discard(&mut game, "P1-DISCARD-TRAINER", create_trainer_meta("Discard Trainer", "Item"));

        let seed = 777;
        game.reseed_rng(seed);
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let heads_count = rng.gen_bool(0.5) as u8 + rng.gen_bool(0.5) as u8;

        assert!(crate::df::runtime::execute_power(&mut game, "Old Rod", source_id));

        match heads_count {
            2 => match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
                Some(Prompt::ChooseCardsFromDiscard { count, min, max, destination, options, .. }) => {
                    assert_eq!(*count, 1);
                    assert_eq!(*min, Some(1));
                    assert_eq!(*max, Some(1));
                    assert_eq!(*destination, SelectionDestination::Hand);
                    assert!(options.contains(&pokemon_id));
                    assert!(!options.contains(&trainer_id));
                }
                other => panic!("unexpected prompt for HH: {other:?}"),
            },
            0 => match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
                Some(Prompt::ChooseCardsFromDiscard { count, min, max, destination, options, .. }) => {
                    assert_eq!(*count, 1);
                    assert_eq!(*min, Some(1));
                    assert_eq!(*max, Some(1));
                    assert_eq!(*destination, SelectionDestination::Hand);
                    assert!(options.contains(&trainer_id));
                    assert!(!options.contains(&pokemon_id));
                }
                other => panic!("unexpected prompt for TT: {other:?}"),
            },
            _ => {
                assert!(game.pending_prompt.is_none());
            }
        }
    }
}
