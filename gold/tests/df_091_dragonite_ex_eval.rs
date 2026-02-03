// Evaluation tests for DF-091 Dragonite ex δ

#[cfg(test)]
mod eval_tests {
    use super::*;
    #[test]
    fn eval_set_constant() { assert_eq!(SET, "DF"); }
    #[test]
    fn eval_number_constant() { assert_eq!(NUMBER, 91); }
    #[test]
    fn eval_name_constant() { assert_eq!(NAME, "Dragonite ex δ"); }
    #[test]
    fn eval_attack_1_name() { assert_eq!(ATTACK_1_NAME, "Deafen"); }
    #[test]
    fn eval_attack_1_damage() { assert_eq!(ATTACK_1_DAMAGE, 40); }
    #[test]
    fn eval_attack_2_name() { assert_eq!(ATTACK_2_NAME, "Dragon Roar"); }
    #[test]
    fn eval_attack_2_damage() { assert_eq!(ATTACK_2_DAMAGE, 0); }
    #[test] fn eval_attack_1_text() { assert_eq!(ATTACK_1_TEXT, "Your opponent can't play any Trainer cards (except for Supporter cards) from his or her hand during your opponent's next turn."); }
    #[test] fn eval_attack_2_text() { assert_eq!(ATTACK_2_TEXT, "Put 8 damage counters on the Defending Pokémon. If that Pokémon would be Knocked Out by this attack, put any damage counters not necessary to Knock Out the Defending Pokémon on your opponent's Benched Pokémon in any way you like."); }
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

    fn setup_game() -> (GameState, CardInstanceId, CardInstanceId, CardInstanceId) {
        let card_meta = CardMetaMap::new();
        let mut game = GameState::new_with_card_meta(
            Vec::new(),
            Vec::new(),
            12345,
            RulesetConfig::default(),
            card_meta,
        );

        let p1_def = CardDefId::new("P1-ACTIVE".to_string());
        game.card_meta.insert(p1_def.clone(), create_pokemon_meta("Dragonite ex δ", 150));
        let p1_card = CardInstance::new(p1_def, PlayerId::P1);
        game.players[0].active = Some(PokemonSlot::new(p1_card));

        let p2_def = CardDefId::new("P2-ACTIVE".to_string());
        game.card_meta.insert(p2_def.clone(), create_pokemon_meta("P2 Active", 50));
        let p2_card = CardInstance::new(p2_def, PlayerId::P2);
        let mut p2_active = PokemonSlot::new(p2_card);
        p2_active.hp = 50;
        game.players[1].active = Some(p2_active);

        let bench1_def = CardDefId::new("P2-BENCH-1".to_string());
        game.card_meta.insert(bench1_def.clone(), create_pokemon_meta("P2 Bench 1", 60));
        let bench1_card = CardInstance::new(bench1_def, PlayerId::P2);
        game.players[1].bench.push(PokemonSlot::new(bench1_card.clone()));

        let bench2_def = CardDefId::new("P2-BENCH-2".to_string());
        game.card_meta.insert(bench2_def.clone(), create_pokemon_meta("P2 Bench 2", 60));
        let bench2_card = CardInstance::new(bench2_def, PlayerId::P2);
        game.players[1].bench.push(PokemonSlot::new(bench2_card.clone()));

        game.turn.player = PlayerId::P1;

        let source_id = game.players[0].active.as_ref().unwrap().card.id;
        (game, source_id, bench1_card.id, bench2_card.id)
    }

    #[test]
    fn eval_dragon_roar_overflow_prompts_bench_distribution() {
        let (mut game, source_id, bench1_id, bench2_id) = setup_game();

        assert!(crate::df::runtime::execute_power(&mut game, "Dragon Roar", source_id));
        match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
            Some(Prompt::ChoosePokemonInPlay { min, max, options, .. }) => {
                assert_eq!(*min, 1);
                assert!(*max >= 1);
                assert!(options.contains(&bench1_id));
                assert!(options.contains(&bench2_id));
            }
            other => panic!("unexpected bench prompt: {other:?}"),
        }

        assert!(crate::df::runtime::resolve_custom_prompt(
            &mut game,
            "DF-91:Dragon Roar:30",
            Some(source_id),
            &[bench1_id, bench2_id],
        ));
        let bench1 = game.players[1].bench.iter().find(|slot| slot.card.id == bench1_id).unwrap();
        let bench2 = game.players[1].bench.iter().find(|slot| slot.card.id == bench2_id).unwrap();
        assert_eq!(bench1.damage_counters + bench2.damage_counters, 3);
    }
}
