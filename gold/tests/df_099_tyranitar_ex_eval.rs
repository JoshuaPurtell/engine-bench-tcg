// Evaluation tests for DF-099 Tyranitar ex δ

#[cfg(test)]
mod eval_tests {
    use super::*;
    #[test]
    fn eval_set_constant() { assert_eq!(SET, "DF"); }
    #[test]
    fn eval_number_constant() { assert_eq!(NUMBER, 99); }
    #[test]
    fn eval_name_constant() { assert_eq!(NAME, "Tyranitar ex δ"); }
    #[test]
    fn eval_attack_1_name() { assert_eq!(ATTACK_1_NAME, "Electromark"); }
    #[test]
    fn eval_attack_1_damage() { assert_eq!(ATTACK_1_DAMAGE, 0); }
    #[test]
    fn eval_attack_2_name() { assert_eq!(ATTACK_2_NAME, "Hyper Claws"); }
    #[test]
    fn eval_attack_2_damage() { assert_eq!(ATTACK_2_DAMAGE, 70); }
    #[test]
    fn eval_attack_3_name() { assert_eq!(ATTACK_3_NAME, "Shock-wave"); }
    #[test]
    fn eval_attack_3_damage() { assert_eq!(ATTACK_3_DAMAGE, 0); }
    #[test] fn eval_attack_1_text() { assert_eq!(ATTACK_1_TEXT, "Put a Shock-wave marker on 1 of your opponent's Pokémon."); }
    #[test] fn eval_attack_2_text() { assert_eq!(ATTACK_2_TEXT, "If the Defending Pokémon is a Stage 2 Evolved Pokémon, this attack does 70 damage plus 20 more damage."); }
    #[test] fn eval_attack_3_text() { assert_eq!(ATTACK_3_TEXT, "Choose 1 of your opponent's Pokémon that has any Shock-wave markers on it. That Pokémon is Knocked Out."); }
}

#[cfg(test)]
mod deep_stateful_tests {
    use super::*;
    use tcg_core::{CardDefId, CardInstance, CardInstanceId, CardMeta, CardMetaMap, GameState, PlayerId, PokemonSlot, Prompt, Stage, Type};
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

    fn setup_game() -> (GameState, CardInstanceId, CardInstanceId) {
        let card_meta = CardMetaMap::new();
        let mut game = GameState::new_with_card_meta(
            Vec::new(),
            Vec::new(),
            12345,
            RulesetConfig::default(),
            card_meta,
        );

        let p1_def = CardDefId::new("P1-ACTIVE".to_string());
        game.card_meta.insert(p1_def.clone(), create_pokemon_meta("Tyranitar ex δ", 150));
        let p1_card = CardInstance::new(p1_def, PlayerId::P1);
        game.players[0].active = Some(PokemonSlot::new(p1_card));

        let p2_def = CardDefId::new("P2-ACTIVE".to_string());
        game.card_meta.insert(p2_def.clone(), create_pokemon_meta("P2 Active", 80));
        let p2_card = CardInstance::new(p2_def, PlayerId::P2);
        let mut p2_active = PokemonSlot::new(p2_card);
        p2_active.hp = 80;
        game.players[1].active = Some(p2_active);

        let bench_def = CardDefId::new("P2-BENCH".to_string());
        game.card_meta.insert(bench_def.clone(), create_pokemon_meta("P2 Bench", 60));
        let bench_card = CardInstance::new(bench_def, PlayerId::P2);
        game.players[1].bench.push(PokemonSlot::new(bench_card.clone()));

        game.turn.player = PlayerId::P1;

        let source_id = game.players[0].active.as_ref().unwrap().card.id;
        (game, source_id, bench_card.id)
    }

    #[test]
    fn eval_shock_wave_prompts_only_marked_targets() {
        let (mut game, source_id, bench_id) = setup_game();
        let active_id = game.players[1].active.as_ref().unwrap().card.id;
        assert!(game.add_marker(active_id, tcg_core::Marker::new("Shock-wave")));

        assert!(crate::df::runtime::execute_power(&mut game, "Shock-wave", source_id));
        match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
            Some(Prompt::ChoosePokemonInPlay { options, min, max, .. }) => {
                assert_eq!(*min, 1);
                assert_eq!(*max, 1);
                assert!(options.contains(&active_id));
                assert!(!options.contains(&bench_id));
            }
            other => panic!("unexpected prompt: {other:?}"),
        }

        assert!(crate::df::runtime::resolve_custom_prompt(
            &mut game,
            "DF-99:Shock-wave",
            Some(source_id),
            &[active_id],
        ));
        let active = game.players[1].active.as_ref().unwrap();
        let remaining = active.hp.saturating_sub(active.damage_counters * 10);
        assert_eq!(remaining, 0);
    }
}
