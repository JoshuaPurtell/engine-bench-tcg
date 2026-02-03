// Evaluation tests for DF-073 Copycat

#[cfg(test)]
mod eval_tests {
    use super::*;
    #[test]
    fn eval_set_constant() { assert_eq!(SET, "DF"); }
    #[test]
    fn eval_number_constant() { assert_eq!(NUMBER, 73); }
    #[test]
    fn eval_name_constant() { assert_eq!(NAME, "Copycat"); }
}


#[cfg(test)]
mod trainer_ast_tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn eval_copycat_trainer_ast_custom() {
        let ast = crate::df::trainer_effect_ast("73", "Copycat", "Supporter")
            .expect("expected trainer ast");
        assert_eq!(ast.get("op").and_then(Value::as_str), Some("Custom"));
        assert_eq!(ast.get("id").and_then(Value::as_str), Some("DF-73:Copycat"));
    }
}


#[cfg(test)]
mod deep_stateful_tests {
    use super::*;
    use tcg_core::{CardDefId, CardInstance, CardInstanceId, CardMeta, CardMetaMap, GameState, PlayerId, PokemonSlot, Stage, Type};
    use tcg_rules_ex::RulesetConfig;

    fn create_pokemon_meta(name: &str, hp: u16) -> CardMeta {
        CardMeta {
            card_type: String::new(),
            delta_species: false,
            name: name.to_string(),
            is_basic: true,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp,
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
        game.card_meta.insert(p1_def.clone(), create_pokemon_meta("P1 Active", 100));
        let p1_card = CardInstance::new(p1_def, PlayerId::P1);
        game.players[0].active = Some(PokemonSlot::new(p1_card));

        let p2_def = CardDefId::new("P2-ACTIVE".to_string());
        game.card_meta.insert(p2_def.clone(), create_pokemon_meta("P2 Active", 100));
        let p2_card = CardInstance::new(p2_def, PlayerId::P2);
        game.players[1].active = Some(PokemonSlot::new(p2_card));

        game.turn.player = PlayerId::P1;

        let source_id = game.players[0].active.as_ref().unwrap().card.id;
        (game, source_id)
    }

    fn add_to_hand(game: &mut GameState, player: PlayerId, def_id: &str) {
        let card_def_id = CardDefId::new(def_id.to_string());
        game.card_meta.insert(card_def_id.clone(), create_pokemon_meta(def_id, 60));
        let card = CardInstance::new(card_def_id, player);
        let idx = if player == PlayerId::P1 { 0 } else { 1 };
        game.players[idx].hand.add(card);
    }

    fn add_to_deck(game: &mut GameState, player: PlayerId, def_id: &str) {
        let card_def_id = CardDefId::new(def_id.to_string());
        game.card_meta.insert(card_def_id.clone(), create_pokemon_meta(def_id, 60));
        let card = CardInstance::new(card_def_id, player);
        let idx = if player == PlayerId::P1 { 0 } else { 1 };
        game.players[idx].deck.add(card);
    }

    #[test]
    fn eval_copycat_shuffles_and_draws_to_match_opponent() {
        let (mut game, source_id) = setup_game();

        for i in 0..2 {
            add_to_hand(&mut game, PlayerId::P1, &format!("P1-HAND-{i}"));
        }
        for i in 0..4 {
            add_to_hand(&mut game, PlayerId::P2, &format!("P2-HAND-{i}"));
        }
        for i in 0..6 {
            add_to_deck(&mut game, PlayerId::P1, &format!("P1-DECK-{i}"));
        }

        let initial_hand = game.players[0].hand.count();
        let initial_deck = game.players[0].deck.count();
        let opponent_hand = game.players[1].hand.count();

        assert!(crate::df::runtime::execute_power(&mut game, "Copycat", source_id));
        assert_eq!(game.players[0].hand.count(), opponent_hand);
        let expected_deck = initial_deck + initial_hand - opponent_hand;
        assert_eq!(game.players[0].deck.count(), expected_deck);
    }
}
