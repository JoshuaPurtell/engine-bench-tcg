// Evaluation tests for DF-075 Holon Mentor

#[cfg(test)]
mod eval_tests {
    use super::*;
    #[test]
    fn eval_set_constant() { assert_eq!(SET, "DF"); }
    #[test]
    fn eval_number_constant() { assert_eq!(NUMBER, 75); }
    #[test]
    fn eval_name_constant() { assert_eq!(NAME, "Holon Mentor"); }
}


#[cfg(test)]
mod trainer_ast_tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn eval_holon_mentor_trainer_ast_structure() {
        let ast = crate::df::trainer_effect_ast("75", "Holon Mentor", "Supporter")
            .expect("expected trainer ast");
        assert_eq!(ast.get("op").and_then(Value::as_str), Some("IfHandNotEmpty"));
        let effect = ast.get("effect").expect("missing effect");
        let effects = effect.get("effects").and_then(Value::as_array).expect("effects array");
        assert_eq!(effects.len(), 2);
        assert_eq!(effects[0].get("op").and_then(Value::as_str), Some("DiscardFromHand"));
        assert_eq!(effects[0].get("count").and_then(Value::as_u64), Some(1));
        assert_eq!(effects[1].get("op").and_then(Value::as_str), Some("SearchDeckWithSelector"));
        let selector = effects[1].get("selector").expect("missing selector");
        assert_eq!(selector.get("is_pokemon").and_then(Value::as_bool), Some(true));
        assert_eq!(selector.get("is_basic").and_then(Value::as_bool), Some(true));
        assert_eq!(selector.get("max_hp").and_then(Value::as_u64), Some(100));
    }
}


#[cfg(test)]
mod deep_stateful_tests {
    use super::*;
    use tcg_core::{CardDefId, CardInstance, CardInstanceId, CardMeta, CardMetaMap, GameState, PlayerId, PokemonSlot, Prompt, Stage, Type};
    use tcg_rules_ex::RulesetConfig;

    fn create_pokemon_meta(name: &str, hp: u16, is_basic: bool) -> CardMeta {
        CardMeta {
            card_type: String::new(),
            delta_species: false,
            name: name.to_string(),
            is_basic,
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
            stage: if is_basic { Stage::Basic } else { Stage::Stage1 },
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
        game.card_meta.insert(p1_def.clone(), create_pokemon_meta("P1 Active", 100, true));
        let p1_card = CardInstance::new(p1_def, PlayerId::P1);
        game.players[0].active = Some(PokemonSlot::new(p1_card));

        let p2_def = CardDefId::new("P2-ACTIVE".to_string());
        game.card_meta.insert(p2_def.clone(), create_pokemon_meta("P2 Active", 100, true));
        let p2_card = CardInstance::new(p2_def, PlayerId::P2);
        game.players[1].active = Some(PokemonSlot::new(p2_card));

        game.turn.player = PlayerId::P1;

        let source_id = game.players[0].active.as_ref().unwrap().card.id;
        (game, source_id)
    }

    fn add_to_hand(game: &mut GameState, def_id: &str) -> CardInstanceId {
        let card_def_id = CardDefId::new(def_id.to_string());
        game.card_meta.insert(card_def_id.clone(), create_pokemon_meta(def_id, 60, true));
        let card = CardInstance::new(card_def_id, PlayerId::P1);
        game.players[0].hand.add(card.clone());
        card.id
    }

    fn add_to_deck(game: &mut GameState, def_id: &str, hp: u16) {
        let card_def_id = CardDefId::new(def_id.to_string());
        game.card_meta.insert(card_def_id.clone(), create_pokemon_meta(def_id, hp, true));
        let card = CardInstance::new(card_def_id, PlayerId::P1);
        game.players[0].deck.add(card);
    }

    #[test]
    fn eval_holon_mentor_discards_then_prompts_deck_search() {
        let (mut game, source_id) = setup_game();
        let discard_id = add_to_hand(&mut game, "P1-HAND-1");
        add_to_hand(&mut game, "P1-HAND-2");
        add_to_deck(&mut game, "BASIC-80", 80);
        add_to_deck(&mut game, "BASIC-120", 120);

        assert!(crate::df::runtime::execute_power(&mut game, "Holon Mentor", source_id));
        match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
            Some(Prompt::ChooseCardsFromHand { count, min, max, .. }) => {
                assert_eq!(*count, 1);
                assert_eq!(*min, Some(1));
                assert_eq!(*max, Some(1));
            }
            other => panic!("unexpected prompt: {other:?}"),
        }

        assert!(crate::df::runtime::resolve_custom_prompt(
            &mut game,
            "DF-75:Holon Mentor:Discard",
            Some(source_id),
            &[discard_id],
        ));
        assert!(game.players[0].discard.order().contains(&discard_id));
        match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
            Some(Prompt::ChooseCardsFromDeck { count, min, max, .. }) => {
                assert_eq!(*count, 3);
                assert_eq!(*min, Some(0));
                assert_eq!(*max, Some(3));
            }
            other => panic!("unexpected prompt after discard: {other:?}"),
        }
    }
}
