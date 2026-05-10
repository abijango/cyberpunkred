//! LLM-judged narrative-quality IP bonus awarding.
//!
//! At the end of a gig the LLM scores how well the player engaged with the
//! fiction — creativity, roleplaying, problem-solving — and returns a bonus
//! IP award. This module defines the request/response types, validates the
//! response, and integrates with the rules-engine IP pool via
//! [`cpr_rules::character::progression::earn::award_milestone_ip`]-style
//! direct mutation.
//!
//! The LLM prompt that generates the [`IpBonusResponse`] is built in
//! WP-707 (`cpr_llm`). This module is *prompt-agnostic* — it only enforces
//! the cap and writes the result into the character's IP pool.
//!
//! # Design deviation from WP-609 plan signature
//!
//! The plan's public API lists `award_llm_bonus_ip(character, response)`.
//! This implementation adds a third parameter `request_cap: u32` because
//! the cap logically belongs to the **caller** (who sent the
//! [`IpBonusRequest`]), not to the response that came back from the LLM.
//! Embedding the cap in the response would allow a misbehaving LLM response
//! to raise its own ceiling. The caller controls the cap; the response is
//! trusted only for the `awarded` value (which is then clamped).
//!
//! # Negative-value rejection
//!
//! The plan acceptance criterion `test_negative_rejected` is satisfied at the
//! type level: `IpBonusResponse::awarded` is `u32`, so a negative value is
//! unrepresentable and cannot reach this function. This is strictly stronger
//! than runtime rejection; the runtime path `GmError::InvalidIpBonus` is
//! reserved for future structural checks (e.g. a parser emitting an
//! out-of-range value before it reaches this layer).
//!
//! # Rulebook reference
//!
//! Improvement Points: *Cyberpunk RED Core Rules* p.411.

use crate::error::GmError;
use crate::ids::GigId;
use cpr_rules::character::Character;
use serde::{Deserialize, Serialize};

/// Input the caller passes when requesting an LLM narrative-quality score.
///
/// The caller packages the gig identity and a relevant excerpt of the session
/// log; the LLM returns an [`IpBonusResponse`] whose `awarded` value is then
/// handed to [`award_llm_bonus_ip`] together with this request's `cap`.
///
/// See *Cyberpunk RED Core Rules* p.411 (Improvement Points).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IpBonusRequest {
    /// The gig this bonus is associated with.
    pub gig: GigId,
    /// Excerpt of the campaign-log text the LLM uses to score narrative quality.
    pub log_excerpt: String,
    /// Maximum IP the LLM is permitted to award for this gig.
    ///
    /// The caller enforces this cap by passing it to [`award_llm_bonus_ip`]
    /// as `request_cap`. The value here is informational for the LLM prompt
    /// (WP-707 uses it to build the prompt's system instructions).
    pub cap: u32,
}

/// The scored bonus IP the LLM returns after evaluating narrative quality.
///
/// Constructed by the LLM-layer adapter (WP-707) after parsing the raw LLM
/// completion. Negative values are unrepresentable — `awarded` is `u32`.
/// Values exceeding `request_cap` are silently clamped inside
/// [`award_llm_bonus_ip`].
///
/// See *Cyberpunk RED Core Rules* p.411 (Improvement Points).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IpBonusResponse {
    /// IP the LLM recommends awarding. Clamped to `request_cap` before
    /// being added to the character pool.
    pub awarded: u32,
    /// Human-readable justification the LLM provided for the score.
    ///
    /// Surfaced to the player as narrative feedback ("You gained N IP because
    /// …"). Not validated by this module.
    pub reason: String,
}

/// Award LLM-judged narrative-quality IP bonus to the character.
///
/// The `response.awarded` value is silently clamped to `request_cap` to
/// prevent LLM inflation. The function then increments
/// `character.improvement_points` by the clamped amount and returns that
/// amount.
///
/// # Parameters
///
/// - `character` — the player character whose IP pool is mutated.
/// - `request_cap` — the hard ceiling set by the caller (typically the value
///   that was placed in [`IpBonusRequest::cap`] when the request was sent).
/// - `response` — the scored response from the LLM adapter.
///
/// # Returns
///
/// The amount actually awarded (post-clamp), in the range `0..=request_cap`.
///
/// # Errors
///
/// Returns [`GmError::InvalidIpBonus`] for structurally malformed responses
/// (reserved for future validation). Current behaviour has no error paths:
/// all `u32` values are valid inputs; clamping is silent.
///
/// # Rulebook reference
///
/// Improvement Points: *Cyberpunk RED Core Rules* p.411.
pub fn award_llm_bonus_ip(
    character: &mut Character,
    request_cap: u32,
    response: &IpBonusResponse,
) -> Result<u32, GmError> {
    let awarded = response.awarded.min(request_cap);
    character.improvement_points += awarded;
    Ok(awarded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpr_rules::character::data::{ArmorKind, ArmorPiece, SkillSet, StatBlock, Wounds};
    use cpr_rules::character::{Inventory, Lifepath, Role, WornArmor};
    use cpr_rules::effects::{EffectStack, WoundState};
    use cpr_rules::types::{CharacterId, Eurobucks};
    use uuid::Uuid;

    fn make_character() -> Character {
        Character {
            id: CharacterId(Uuid::from_u128(0xAB)),
            name: "Test".to_string(),
            handle: None,
            role: Role::Solo,
            role_rank: 1,
            stats: StatBlock {
                int: 5,
                r#ref: 5,
                dex: 5,
                tech: 5,
                cool: 5,
                will: 5,
                luck: 5,
                r#move: 5,
                body: 5,
                emp: 5,
            },
            skills: SkillSet {
                ranks: std::collections::HashMap::new(),
            },
            cyberware: vec![],
            armor: WornArmor {
                head: Some(ArmorPiece {
                    kind: ArmorKind::LightArmorjack,
                    current_sp: 11,
                    max_sp: 11,
                }),
                body: Some(ArmorPiece {
                    kind: ArmorKind::LightArmorjack,
                    current_sp: 11,
                    max_sp: 11,
                }),
            },
            inventory: Inventory { items: vec![] },
            wounds: Wounds {
                current_hp: 25,
                max_hp: 25,
                seriously_wounded_threshold: 13,
                death_save_base: 5,
                death_save_penalty: 0,
                current_state: WoundState::None,
            },
            humanity: 40,
            luck_pool: 5,
            money: Eurobucks(0),
            improvement_points: 0,
            lifepath: Lifepath::default(),
            effects: EffectStack::new(),
            complementary_bonuses: vec![],
        }
    }

    /// Verifies that a response exceeding the cap is silently clamped.
    ///
    /// Per WP-609 acceptance criterion `test_cap_enforced`.
    #[test]
    fn test_cap_enforced() {
        let mut c = make_character();
        let response = IpBonusResponse {
            awarded: 50,
            reason: "Excellent roleplay".to_string(),
        };
        let actual = award_llm_bonus_ip(&mut c, 30, &response).expect("should succeed");
        assert_eq!(actual, 30);
        assert_eq!(c.improvement_points, 30);
    }

    /// Verifies that a response within the cap passes through unchanged.
    ///
    /// Per WP-609 acceptance criterion `test_within_cap_passes_through`.
    #[test]
    fn test_within_cap_passes_through() {
        let mut c = make_character();
        let response = IpBonusResponse {
            awarded: 15,
            reason: "Good engagement".to_string(),
        };
        let actual = award_llm_bonus_ip(&mut c, 30, &response).expect("should succeed");
        assert_eq!(actual, 15);
        assert_eq!(c.improvement_points, 15);
    }

    /// Verifies that a zero award is a no-op — character's IP is unchanged.
    ///
    /// Per WP-609 acceptance criterion `test_zero_award_is_noop`.
    #[test]
    fn test_zero_award_is_noop() {
        let mut c = make_character();
        let response = IpBonusResponse {
            awarded: 0,
            reason: "No notable narrative moments".to_string(),
        };
        let actual = award_llm_bonus_ip(&mut c, 30, &response).expect("should succeed");
        assert_eq!(actual, 0);
        assert_eq!(c.improvement_points, 0);
    }
}
