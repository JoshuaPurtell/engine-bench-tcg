// Evaluation tests for DF-080 Professor Oak's Research

#[cfg(test)]
mod eval_tests {
    use super::*;
    #[test]
    fn eval_set_constant() { assert_eq!(SET, "DF"); }
    #[test]
    fn eval_number_constant() { assert_eq!(NUMBER, 80); }
    #[test]
    fn eval_name_constant() { assert_eq!(NAME, "Professor Oak's Research"); }
}


#[cfg(test)]
mod trainer_ast_tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn eval_professor_oak_trainer_ast_sequence() {
        let ast = crate::df::trainer_effect_ast("80", "Professor Oak's Research", "Supporter")
            .expect("expected trainer ast");
        assert_eq!(ast.get("op").and_then(Value::as_str), Some("Sequence"));
        let effects = ast.get("effects").and_then(Value::as_array).expect("effects array");
        assert_eq!(effects.len(), 2);
        assert_eq!(effects[0].get("op").and_then(Value::as_str), Some("ShuffleHandIntoDeck"));
        assert_eq!(effects[1].get("op").and_then(Value::as_str), Some("DrawCards"));
        assert_eq!(effects[1].get("count").and_then(Value::as_u64), Some(5));
    }
}


#[cfg(test)]
mod deep_stateful_tests {
    use super::*;
    use tcg_core::{CardDefId, CardInstance, CardInstanceId, CardMeta, CardMetaMap, GameState, PlayerId, PokemonSlot, Stage, Type};
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

    fn add_to_hand(game: &mut GameState, def_id: &str) {
        let card_def_id = CardDefId::new(def_id.to_string());
        game.card_meta.insert(card_def_id.clone(), create_pokemon_meta(def_id));
        let card = CardInstance::new(card_def_id, PlayerId::P1);
        game.players[0].hand.add(card);
    }

    fn add_to_deck(game: &mut GameState, def_id: &str) {
        let card_def_id = CardDefId::new(def_id.to_string());
        game.card_meta.insert(card_def_id.clone(), create_pokemon_meta(def_id));
        let card = CardInstance::new(card_def_id, PlayerId::P1);
        game.players[0].deck.add(card);
    }

    #[test]
    fn eval_professor_oak_shuffles_hand_then_draws_five() {
        let (mut game, source_id) = setup_game();
        for i in 0..3 {
            add_to_hand(&mut game, &format!("P1-HAND-{i}"));
        }
        for i in 0..10 {
            add_to_deck(&mut game, &format!("P1-DECK-{i}"));
        }

        let initial_hand = game.players[0].hand.count();
        let initial_deck = game.players[0].deck.count();

        assert!(crate::df::runtime::execute_power(
            &mut game,
            "Professor Oak's Research",
            source_id,
        ));
        assert_eq!(game.players[0].hand.count(), 5);
        let expected_deck = initial_deck + initial_hand - 5;
        assert_eq!(game.players[0].deck.count(), expected_deck);
    }
}
