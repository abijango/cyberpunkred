//! Credibility (Media Role Ability) — WP-514.
//!
//! ## Rulebook mechanics (pp.151–152)
//!
//! **Credibility** is the Media's Role Ability (p.151). The Media not only can
//! convince an audience of what they publish, but also has a larger audience
//! the more credible they are. They also have greater levels of access to
//! sources and information. Medias are also in the know and pick up on rumors
//! passively.
//!
//! ### Audience scope by rank (p.152)
//!
//! The rulebook describes four tiers of audience/impact, mapped to rank pairs:
//!
//! | Rank(s) | Audience scope |
//! |---------|----------------|
//! | 1–2     | Immediate neighborhood (`Single`) |
//! | 3–4     | Local screamsheet / Data Pool contributor (`SmallGroup`) |
//! | 5–6     | Citywide reach (`Crowd`) |
//! | 7–8     | Statewide reach (`Mob`) |
//! | 9–10    | National / international reach (`Mob`) |
//!
//! **Deviation note:** The WP-514 spec lists brackets 1-2 / 3-5 / 6-7 / 8-10.
//! The rulebook (p.152) uses even pairs: 1-2, 3-4, 5-6, 7-8, and then 9 (with
//! rank 10 implied). This implementation follows RAW: ranks 1-2 → `Single`,
//! ranks 3-4 → `SmallGroup`, ranks 5-6 → `Crowd`, ranks 7-10 → `Mob`.
//! Flagged in PR.
//!
//! ### Rumors (p.151)
//!
//! Passive rumour-gathering: at least twice per week the GM secretly rolls
//! `Credibility Rank + 1d10`. The check is compared to the Rumor Table's
//! Passive DV column (Vague 7, Typical 9, Substantial 11, Detailed 13). Not
//! modelled mechanically here — LLM-narrated at the GM layer.
//!
//! ### Publishing stories and scoops (pp.151–152)
//!
//! Access/Sources, Audience, Believability, and Impact all scale with rank.
//! Those are social/narrative outcomes handled by the GM layer; this module
//! exposes the rank accessor and the scope classifier that the GM layer uses
//! to gate those outcomes.
//!
//! ## What this module provides
//!
//! - [`credibility_rank`] — returns the effective Credibility rank for a character.
//! - [`CredibilityScope`] — audience-reach tier at a given rank.
//! - [`credibility_scope`] — maps a rank to its [`CredibilityScope`].
//!
//! See pp.151–152.

use crate::character::data::Role;
use crate::character::Character;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Returns the character's effective Credibility rank.
///
/// Per p.151, Credibility is the Media's Role Ability and its rank equals
/// `character.role_rank` when the character is a [`Role::Media`]. For any
/// other role the ability does not apply; this function returns `0` so callers
/// can skip application without special-casing the role check.
///
/// # Examples
///
/// ```
/// # use cpr_rules::roles::credibility::credibility_rank;
/// # use cpr_rules::character::Character;
/// // See test_credibility_rank_for_media.
/// ```
///
/// See p.151.
pub fn credibility_rank(character: &Character) -> u8 {
    // See p.151 — Credibility belongs exclusively to the Media role.
    if character.role == Role::Media {
        character.role_rank
    } else {
        0
    }
}

/// Audience scope that a Media's Credibility can reach at a given rank.
///
/// The rulebook (p.152) describes four tiers, mapping rank pairs to
/// progressively larger audiences and levels of impact:
///
/// - [`Single`][CredibilityScope::Single] — ranks 1–2: immediate neighbourhood.
/// - [`SmallGroup`][CredibilityScope::SmallGroup] — ranks 3–4: city-level
///   contributor, local screamsheet / Data Pool audience.
/// - [`Crowd`][CredibilityScope::Crowd] — ranks 5–6: citywide reach.
/// - [`Mob`][CredibilityScope::Mob] — ranks 7–10: statewide to national /
///   international impact.
///
/// See p.152.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CredibilityScope {
    /// Ranks 1–2: reach is an immediate neighbourhood (single-person or
    /// very local audience). Impact is small and incremental. See p.152.
    Single,
    /// Ranks 3–4: audience is city-level; Media is a known local contributor
    /// on a screamsheet or Data Pool. See p.152.
    SmallGroup,
    /// Ranks 5–6: audience is citywide; Media is a regular columnist or
    /// TV contributor. See p.152.
    Crowd,
    /// Ranks 7–10: audience is statewide or broader; Media is a recognised
    /// figure with national / international impact. See p.152.
    Mob,
}

/// Maps a Credibility rank to its [`CredibilityScope`] tier.
///
/// Ranks follow the four-tier table on p.152:
///
/// | Rank | Scope |
/// |------|-------|
/// | 0    | [`Single`][CredibilityScope::Single] (no rank — treated as tier floor) |
/// | 1–2  | [`Single`][CredibilityScope::Single] |
/// | 3–4  | [`SmallGroup`][CredibilityScope::SmallGroup] |
/// | 5–6  | [`Crowd`][CredibilityScope::Crowd] |
/// | 7–10 | [`Mob`][CredibilityScope::Mob] |
///
/// Values above 10 (unreachable in normal play) clamp to [`Mob`][CredibilityScope::Mob].
///
/// **RAW note:** The WP-514 spec brackets differ slightly from the rulebook;
/// this implementation uses the rulebook brackets (p.152). Deviation is
/// documented in the PR.
///
/// See p.152.
pub fn credibility_scope(rank: u8) -> CredibilityScope {
    // See p.152 — Credibility Ranks table.
    match rank {
        0..=2 => CredibilityScope::Single,
        3..=4 => CredibilityScope::SmallGroup,
        5..=6 => CredibilityScope::Crowd,
        _ => CredibilityScope::Mob, // ranks 7–10 (and hypothetical higher values)
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
            id: CharacterId(Uuid::from_u128(0xAB)),
            name: "Test".to_string(),
            handle: None,
            role,
            role_rank,
            stats: StatBlock {
                int: 6,
                r#ref: 6,
                dex: 6,
                tech: 5,
                cool: 8,
                will: 6,
                luck: 6,
                r#move: 6,
                body: 5,
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
                current_hp: 30,
                max_hp: 30,
                seriously_wounded_threshold: 15,
                death_save_base: 5,
                death_save_penalty: 0,
                current_state: WoundState::None,
            },
            humanity: 60,
            luck_pool: 6,
            money: Eurobucks(0),
            improvement_points: 0,
            lifepath: Lifepath::default(),
            effects: EffectStack::new(),
            complementary_bonuses: Vec::new(),
        }
    }

    /// `test_credibility_rank_for_media`: Media with role_rank 6 → 6.
    ///
    /// Per p.151: Credibility rank equals role_rank for Media characters.
    #[test]
    fn test_credibility_rank_for_media() {
        let character = make_character(Role::Media, 6);
        assert_eq!(credibility_rank(&character), 6, "Media rank 6 → 6 (p.151)");
    }

    /// `test_credibility_rank_zero_for_non_media`: Solo role → 0.
    ///
    /// Per p.151: Credibility belongs exclusively to the Media role.
    #[test]
    fn test_credibility_rank_zero_for_non_media() {
        let character = make_character(Role::Solo, 7);
        assert_eq!(
            credibility_rank(&character),
            0,
            "Solo → 0 for Credibility (p.151)"
        );
    }

    /// `test_credibility_scope_at_each_tier`: verify all four scope tiers.
    ///
    /// Per p.152 Credibility Ranks table:
    /// - 0–2  → `Single`
    /// - 3–4  → `SmallGroup`
    /// - 5–6  → `Crowd`
    /// - 7–10 → `Mob`
    #[test]
    fn test_credibility_scope_at_each_tier() {
        // Rank 0 — no rank at all, treated as tier floor. See p.152.
        assert_eq!(
            credibility_scope(0),
            CredibilityScope::Single,
            "rank 0 → Single"
        );
        // Ranks 1–2: immediate neighbourhood. See p.152.
        assert_eq!(
            credibility_scope(1),
            CredibilityScope::Single,
            "rank 1 → Single"
        );
        assert_eq!(
            credibility_scope(2),
            CredibilityScope::Single,
            "rank 2 → Single"
        );
        // Ranks 3–4: city-level contributor. See p.152.
        assert_eq!(
            credibility_scope(3),
            CredibilityScope::SmallGroup,
            "rank 3 → SmallGroup"
        );
        assert_eq!(
            credibility_scope(4),
            CredibilityScope::SmallGroup,
            "rank 4 → SmallGroup"
        );
        // Ranks 5–6: citywide. See p.152.
        assert_eq!(
            credibility_scope(5),
            CredibilityScope::Crowd,
            "rank 5 → Crowd"
        );
        assert_eq!(
            credibility_scope(6),
            CredibilityScope::Crowd,
            "rank 6 → Crowd"
        );
        // Ranks 7–10: statewide / national. See p.152.
        assert_eq!(credibility_scope(7), CredibilityScope::Mob, "rank 7 → Mob");
        assert_eq!(credibility_scope(8), CredibilityScope::Mob, "rank 8 → Mob");
        assert_eq!(credibility_scope(9), CredibilityScope::Mob, "rank 9 → Mob");
        assert_eq!(
            credibility_scope(10),
            CredibilityScope::Mob,
            "rank 10 → Mob"
        );
    }
}
