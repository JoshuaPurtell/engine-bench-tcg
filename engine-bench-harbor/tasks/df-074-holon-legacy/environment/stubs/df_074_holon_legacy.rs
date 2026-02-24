//! Holon Canonical - Dragon Frontiers #74

pub const SET: &str = "DF";
pub const NUMBER: u32 = 74;
pub const NAME: &str = "Holon Canonical";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_card_identifiers() {
        assert_eq!(SET, "DF");
        assert_eq!(NUMBER, 74);
        assert_eq!(NAME, "Holon Canonical");
    }
}
