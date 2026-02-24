// Evaluation tests for DF-074 Holon Canonical

#[cfg(test)]
mod eval_tests {
    use super::*;
    #[test]
    fn eval_set_constant() { assert_eq!(SET, "DF"); }
    #[test]
    fn eval_number_constant() { assert_eq!(NUMBER, 74); }
    #[test]
    fn eval_name_constant() { assert_eq!(NAME, "Holon Canonical"); }
}


#[cfg(test)]
mod trainer_ast_tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn eval_holon_canonical_trainer_ast_custom() {
        let ast = crate::df::trainer_effect_ast("74", "Holon Canonical", "Stadium")
            .expect("expected trainer ast");
        assert_eq!(ast.get("op").and_then(Value::as_str), Some("Custom"));
        assert_eq!(ast.get("id").and_then(Value::as_str), Some("DF-74:Holon Canonical"));
    }
}
