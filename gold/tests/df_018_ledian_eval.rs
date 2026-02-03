// Evaluation tests for DF-018 Ledian δ

#[cfg(test)]
mod eval_tests {
    use super::*;
    #[test]
    fn eval_set_constant() { assert_eq!(SET, "DF"); }
    #[test]
    fn eval_number_constant() { assert_eq!(NUMBER, 18); }
    #[test]
    fn eval_name_constant() { assert_eq!(NAME, "Ledian δ"); }
    #[test]
    fn eval_ability_1_name() { assert_eq!(ABILITY_1_NAME, "Prowl"); }
    #[test]
    fn eval_attack_1_name() { assert_eq!(ATTACK_1_NAME, "Metal Star"); }
    #[test]
    fn eval_attack_1_damage() { assert_eq!(ATTACK_1_DAMAGE, 30); }
    #[test] fn eval_ability_1_text() { assert_eq!(ABILITY_1_TEXT, "Once during your turn, when you play Ledian from your hand to evolve 1 of your Pokémon, you may search your deck for any 1 card and put it into your hand. Shuffle your deck afterward."); }
    #[test] fn eval_attack_1_text() { assert_eq!(ATTACK_1_TEXT, "If Ledian has a Pokémon Tool card attached to it, draw 3 cards."); }
}

#[cfg(test)]
mod deep_stateful_tests {
    use super::*;
    use tcg_core::{CardDefId, CardInstance, CardMeta, CardMetaMap, GameState, PlayerId, PokemonSlot, Prompt, SelectionDestination, Stage, Type};
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
            hp: 70,
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

    fn setup_game() -> (GameState, tcg_core::CardInstanceId) {
        let card_meta = CardMetaMap::new();
        let mut game = GameState::new_with_card_meta(
            Vec::new(),
            Vec::new(),
            2024,
            RulesetConfig::default(),
            card_meta,
        );

        let p1_def = CardDefId::new("P1-ACTIVE".to_string());
        game.card_meta.insert(p1_def.clone(), create_pokemon_meta("Ledian δ"));
        let p1_card = CardInstance::new(p1_def, PlayerId::P1);
        game.players[0].active = Some(PokemonSlot::new(p1_card));
        game.turn.player = PlayerId::P1;

        let source_id = game.players[0].active.as_ref().unwrap().card.id;
        (game, source_id)
    }

    fn add_to_deck(game: &mut GameState, def_id: &str) -> tcg_core::CardInstanceId {
        let card_def_id = CardDefId::new(def_id.to_string());
        game.card_meta.insert(card_def_id.clone(), create_pokemon_meta(def_id));
        let card = CardInstance::new(card_def_id, PlayerId::P1);
        game.players[0].deck.add(card.clone());
        card.id
    }

    #[test]
    fn eval_prowl_prompts_deck_search() {
        let (mut game, source_id) = setup_game();
        let first_id = add_to_deck(&mut game, "P1-DECK-0");
        let second_id = add_to_deck(&mut game, "P1-DECK-1");

        assert!(crate::df::runtime::execute_power(&mut game, "Prowl", source_id));
        match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
            Some(Prompt::ChooseCardsFromDeck { player, count, options, min, max, destination, shuffle, .. }) => {
                assert_eq!(*player, PlayerId::P1);
                assert_eq!(*count, 1);
                assert!(min.is_none());
                assert!(max.is_none());
                assert_eq!(*destination, SelectionDestination::Hand);
                assert!(*shuffle);
                assert!(options.contains(&first_id));
                assert!(options.contains(&second_id));
            }
            other => panic!("unexpected prowl prompt: {other:?}"),
        }
    }
}
