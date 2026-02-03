// Evaluation tests for DF-077 Mr. Stone's Project

#[cfg(test)]
mod eval_tests {
    use super::*;
    #[test]
    fn eval_set_constant() { assert_eq!(SET, "DF"); }
    #[test]
    fn eval_number_constant() { assert_eq!(NUMBER, 77); }
    #[test]
    fn eval_name_constant() { assert_eq!(NAME, "Mr. Stone's Project"); }
}


#[cfg(test)]
mod trainer_ast_tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn eval_mr_stone_project_trainer_ast_custom() {
        let ast = crate::df::trainer_effect_ast("77", "Mr. Stone's Project", "Supporter")
            .expect("expected trainer ast");
        assert_eq!(ast.get("op").and_then(Value::as_str), Some("Custom"));
        assert_eq!(ast.get("id").and_then(Value::as_str), Some("DF-77:Mr. Stone's Project"));
    }
}


#[cfg(test)]
mod deep_stateful_tests {
    use super::*;
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

    fn create_energy_meta(name: &str) -> CardMeta {
        CardMeta {
            card_type: String::new(),
            delta_species: false,
            name: name.to_string(),
            is_basic: false,
            is_tool: false,
            is_stadium: false,
            is_pokemon: false,
            is_energy: true,
            hp: 0,
            energy_kind: Some("Basic".to_string()),
            provides: vec![Type::Colorless],
            trainer_kind: None,
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

    fn add_energy_to_discard(game: &mut GameState, def_id: &str) {
        let card_def_id = CardDefId::new(def_id.to_string());
        game.card_meta.insert(card_def_id.clone(), create_energy_meta(def_id));
        let card = CardInstance::new(card_def_id, PlayerId::P1);
        game.players[0].discard.add(card);
    }

    fn add_energy_to_deck(game: &mut GameState, def_id: &str) {
        let card_def_id = CardDefId::new(def_id.to_string());
        game.card_meta.insert(card_def_id.clone(), create_energy_meta(def_id));
        let card = CardInstance::new(card_def_id, PlayerId::P1);
        game.players[0].deck.add(card);
    }

    #[test]
    fn eval_mr_stone_prompts_discard_then_deck() {
        let (mut game, source_id) = setup_game();
        add_energy_to_discard(&mut game, "ENERGY-DISCARD");
        add_energy_to_deck(&mut game, "ENERGY-DECK");

        assert!(crate::df::runtime::execute_power(&mut game, "Mr. Stone's Project", source_id));
        match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
            Some(Prompt::ChooseCardsFromDiscard { count, min, max, .. }) => {
                assert_eq!(*count, 2);
                assert_eq!(*min, Some(0));
                assert_eq!(*max, Some(2));
            }
            other => panic!("unexpected discard prompt: {other:?}"),
        }

        assert!(crate::df::runtime::resolve_custom_prompt(
            &mut game,
            "DF-77:Mr. Stone's Project:Discard",
            Some(source_id),
            &[],
        ));
        match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
            Some(Prompt::ChooseCardsFromDeck { count, min, max, destination, shuffle, .. }) => {
                assert_eq!(*count, 2);
                assert_eq!(*min, Some(0));
                assert_eq!(*max, Some(2));
                assert_eq!(*destination, SelectionDestination::Hand);
                assert!(*shuffle);
            }
            other => panic!("unexpected deck prompt: {other:?}"),
        }
    }
}
