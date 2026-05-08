//! Charismatic Impact (Rockerboy Role Ability) — WP-518.
//!
//! **Rulebook reference:** pp.143–145.
//!
//! ## Overview
//!
//! The Rockerboy's Role Ability is **Charismatic Impact**. They can influence
//! others by sheer presence of personality — through music, poetry, art,
//! dance, or simply their physical presence. Per p.144: "The Rockerboy has the
//! Role ability Charismatic Impact. They can influence others by sheer presence
//! of personality."
//!
//! **Key rule (p.144):** "A Rockerboy can only use their Charismatic Impact
//! Role Ability on Fans."
//!
//! ## The check (p.144)
//!
//! To convert non-fans into fans: roll **Charismatic Impact + 1d10** vs. a DV
//! determined by group size:
//!
//! - DV8 for a Single Person
//! - DV10 for a Small Group of up to 6
//! - DV12 for a Huge Group
//!
//! The Charismatic Impact value used in the roll is the Rockerboy's Role Rank
//! (an additive bonus to the d10 roll). See [`social_bonus`].
//!
//! ## Rank tiers (pp.144–145)
//!
//! Each rank tier unlocks a better venue type and stronger influence over fans.
//! See [`AudienceScope`] and [`audience_scope`].
//!
//! | Ranks | Venue                                      | Scope mapping |
//! |-------|--------------------------------------------|---------------|
//! | 1–2   | Small local clubs                          | [`AudienceScope::Solo`]    |
//! | 3–4   | Well known clubs                           | [`AudienceScope::Group`]   |
//! | 5–6   | Large, important clubs                     | [`AudienceScope::Hall`]    |
//! | 7–8   | Small concert halls, local video feed      | [`AudienceScope::Crowd`]   |
//! | 9     | Large concert halls, national video feed   | [`AudienceScope::Stadium`] |
//! | 10    | Huge stadiums or international video       | [`AudienceScope::Stadium`] |
//!
//! **API note:** The WP-518 spec defines the `AudienceScope` tiers as
//! `Solo` (1–2), `Group` (3–5), `Hall` (6–7), `Crowd` (8–9), `Stadium` (10).
//! The rulebook's venue tier boundaries are 1–2 / 3–4 / 5–6 / 7–8 / 9–10.
//! This module follows the WP-518 public API contract exactly (per CLAUDE.md:
//! "The public API in your WP is a contract"). Deviation flagged in PR.
//!
//! ## What this module provides
//!
//! - [`charismatic_impact_rank`] — effective rank for a character (0 for
//!   non-Rockerboys).
//! - [`social_bonus`] — rank-scaled additive bonus to Persuasion / Performance
//!   social rolls.
//! - [`AudienceScope`] — audience size that the Rockerboy can affect at each
//!   rank tier.
//! - [`audience_scope`] — maps a rank to its [`AudienceScope`].
//!
//! See pp.143–145.

use crate::character::data::Role;
use crate::character::Character;

/// Returns the character's effective Charismatic Impact rank.
///
/// Per p.144, Charismatic Impact is the Rockerboy's Role Ability and its rank
/// equals `character.role_rank` when the character is a [`Role::Rockerboy`].
/// For any other role the ability does not apply; this function returns `0`
/// so callers can skip application without special-casing the role check.
///
/// # Examples
///
/// ```
/// # use cpr_rules::roles::charismatic_impact::charismatic_impact_rank;
/// # use cpr_rules::character::Character;
/// // See test_charismatic_impact_rank_for_rockerboy.
/// ```
///
/// See p.144.
pub fn charismatic_impact_rank(character: &Character) -> u8 {
    // See p.144: Charismatic Impact belongs to Rockerboy only.
    if character.role == Role::Rockerboy {
        character.role_rank
    } else {
        0
    }
}

/// Rank-scaled bonus added to Persuasion / Performance social rolls.
///
/// Per p.144, the Charismatic Impact check is "Charismatic Impact + 1d10"
/// where Charismatic Impact equals the Rockerboy's Role Rank. This function
/// returns that additive bonus as an `i8`, suitable for passing to a
/// `SkillCheck` or being accumulated in an `EffectModifier::SkillBonus`.
///
/// Returns `0` for rank `0` (non-Rockerboy or unranked character).
///
/// See p.144.
pub fn social_bonus(rank: u8) -> i8 {
    // Per p.144: the Rockerboy adds their full Role Rank to Charismatic Impact checks.
    rank as i8
}

/// Audience size that the Rockerboy can affect with Charismatic Impact.
///
/// Scales with Role Rank. Each tier represents both the maximum venue size
/// the Rockerboy can play under most circumstances and the scope of fans
/// they can meaningfully influence.
///
/// See pp.144–145 for the full rank-by-rank description.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AudienceScope {
    /// Ranks 1–2: one-on-one influence. Venue: small local clubs (p.144).
    Solo,
    /// Ranks 3–5: small group influence (up to 6 fans). Venue: well known
    /// clubs (p.144).
    Group,
    /// Ranks 6–7: a hall full of people. Venue: large, important clubs
    /// (p.145).
    Hall,
    /// Ranks 8–9: a crowd. Venue: small concert halls, local/national video
    /// feed (p.145).
    Crowd,
    /// Rank 10: stadium scale. Venue: huge stadiums or international video
    /// (p.145).
    Stadium,
}

/// Maps a Charismatic Impact rank to its [`AudienceScope`] tier.
///
/// Ranks outside 1–10 clamp to the nearest defined tier: rank `0` returns
/// [`AudienceScope::Solo`] (the minimum); rank > 10 returns
/// [`AudienceScope::Stadium`] (the maximum). In practice, rulebook rank is
/// always 1–10 for active Rockerboys.
///
/// **Tier boundaries follow the WP-518 public API contract:**
/// `Solo` 1–2, `Group` 3–5, `Hall` 6–7, `Crowd` 8–9, `Stadium` 10.
/// The rulebook's venue sections pair ranks as 1–2 / 3–4 / 5–6 / 7–8 / 9 / 10
/// (pp.144–145). Deviation documented at module level and in PR.
///
/// See pp.144–145.
pub fn audience_scope(rank: u8) -> AudienceScope {
    // See pp.144–145: rank tiers per WP-518 public API contract.
    match rank {
        0..=2 => AudienceScope::Solo,
        3..=5 => AudienceScope::Group,
        6..=7 => AudienceScope::Hall,
        8..=9 => AudienceScope::Crowd,
        _ => AudienceScope::Stadium,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::data::{
        Inventory, Lifepath, Role, SkillSet, StatBlock, WornArmor, Wounds,
    };
    use crate::effects::{EffectStack, WoundState};
    use crate::types::{CharacterId, Eurobucks};
    use std::collections::HashMap;
    use uuid::Uuid;

    fn make_character(role: Role, role_rank: u8) -> Character {
        Character {
            id: CharacterId(Uuid::from_u128(0xCB)),
            name: "Test".to_string(),
            handle: None,
            role,
            role_rank,
            stats: StatBlock {
                int: 6,
                r#ref: 6,
                dex: 6,
                tech: 6,
                cool: 8,
                will: 6,
                luck: 6,
                r#move: 6,
                body: 6,
                emp: 7,
            },
            skills: SkillSet {
                ranks: HashMap::new(),
            },
            cyberware: vec![],
            armor: WornArmor {
                head: None,
                body: None,
            },
            inventory: Inventory { items: vec![] },
            wounds: Wounds {
                current_hp: 35,
                max_hp: 35,
                seriously_wounded_threshold: 18,
                death_save_base: 6,
                death_save_penalty: 0,
                current_state: WoundState::None,
            },
            humanity: 50,
            luck_pool: 6,
            money: Eurobucks(0),
            improvement_points: 0,
            lifepath: Lifepath::default(),
            effects: EffectStack::new(),
            complementary_bonuses: Vec::new(),
        }
    }

    /// `test_charismatic_impact_rank_for_rockerboy`: Rockerboy role_rank 6 → 6.
    ///
    /// Per p.144: Charismatic Impact rank equals role_rank for Rockerboy characters.
    #[test]
    fn test_charismatic_impact_rank_for_rockerboy() {
        let character = make_character(Role::Rockerboy, 6);
        assert_eq!(
            charismatic_impact_rank(&character),
            6,
            "Rockerboy rank 6 → 6 (p.144)"
        );
    }

    /// `test_charismatic_impact_rank_zero_for_non_rockerboy`: Solo role → 0.
    ///
    /// Per p.144: Charismatic Impact belongs exclusively to the Rockerboy role.
    #[test]
    fn test_charismatic_impact_rank_zero_for_non_rockerboy() {
        let character = make_character(Role::Solo, 6);
        assert_eq!(
            charismatic_impact_rank(&character),
            0,
            "Solo → 0; Charismatic Impact is Rockerboy-only (p.144)"
        );
    }

    /// `test_audience_scope_at_each_tier`: verifies all five AudienceScope
    /// variants are returned at the correct rank boundaries (WP-518 contract).
    ///
    /// See pp.144–145.
    #[test]
    fn test_audience_scope_at_each_tier() {
        // Solo tier: ranks 0–2
        assert_eq!(audience_scope(0), AudienceScope::Solo, "rank 0 → Solo");
        assert_eq!(
            audience_scope(1),
            AudienceScope::Solo,
            "rank 1 → Solo (p.144)"
        );
        assert_eq!(
            audience_scope(2),
            AudienceScope::Solo,
            "rank 2 → Solo (p.144)"
        );

        // Group tier: ranks 3–5
        assert_eq!(
            audience_scope(3),
            AudienceScope::Group,
            "rank 3 → Group (p.144)"
        );
        assert_eq!(
            audience_scope(4),
            AudienceScope::Group,
            "rank 4 → Group (p.144)"
        );
        assert_eq!(
            audience_scope(5),
            AudienceScope::Group,
            "rank 5 → Group (p.144)"
        );

        // Hall tier: ranks 6–7
        assert_eq!(
            audience_scope(6),
            AudienceScope::Hall,
            "rank 6 → Hall (p.145)"
        );
        assert_eq!(
            audience_scope(7),
            AudienceScope::Hall,
            "rank 7 → Hall (p.145)"
        );

        // Crowd tier: ranks 8–9
        assert_eq!(
            audience_scope(8),
            AudienceScope::Crowd,
            "rank 8 → Crowd (p.145)"
        );
        assert_eq!(
            audience_scope(9),
            AudienceScope::Crowd,
            "rank 9 → Crowd (p.145)"
        );

        // Stadium tier: rank 10
        assert_eq!(
            audience_scope(10),
            AudienceScope::Stadium,
            "rank 10 → Stadium (p.145)"
        );

        // Clamping above max
        assert_eq!(
            audience_scope(11),
            AudienceScope::Stadium,
            "rank > 10 clamps to Stadium"
        );
    }

    /// `test_social_bonus_scales`: verifies that social_bonus returns rank as i8.
    ///
    /// Per p.144: the check is "Charismatic Impact + 1d10" where Charismatic
    /// Impact equals the Rockerboy's Role Rank.
    #[test]
    fn test_social_bonus_scales() {
        assert_eq!(social_bonus(0), 0, "rank 0 → bonus 0");
        assert_eq!(social_bonus(1), 1, "rank 1 → bonus 1 (p.144)");
        assert_eq!(social_bonus(4), 4, "rank 4 → bonus 4 (p.144)");
        assert_eq!(social_bonus(7), 7, "rank 7 → bonus 7 (p.145)");
        assert_eq!(social_bonus(10), 10, "rank 10 → bonus 10 (p.145)");
    }
}
