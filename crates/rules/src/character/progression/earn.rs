//! Improvement Point earning — objective milestone awards.
//!
//! Cyberpunk RED awards Improvement Points (IP) at the end of a session for
//! concrete in-fiction events. This module models the *objective* side of that
//! award: fixed IP values tied to named milestones. The LLM-narrative bonus
//! (a capped per-session award) is handled separately in the `gm` crate
//! (out of scope here — see `IMPLEMENTATION_PLAN.md` §0.2 hybrid IP design).
//!
//! # Design
//!
//! IP values are compile-time constants derived from the §0.2 hybrid IP
//! design table. See `IMPLEMENTATION_PLAN.md` §0.2 and §4 WP-509.

use crate::character::Character;
use serde::{Deserialize, Serialize};

/// A discrete in-fiction event that triggers a fixed IP award.
///
/// Variants map directly to the §0.2 hybrid IP design table.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IpMilestone {
    /// The team accepted and started a gig. No IP is awarded for merely
    /// starting; IP flows on completion (see [`GigCompletedSuccessfully`] /
    /// [`GigCompletedFailure`]).
    ///
    /// [`GigCompletedSuccessfully`]: IpMilestone::GigCompletedSuccessfully
    /// [`GigCompletedFailure`]: IpMilestone::GigCompletedFailure
    GigStarted,
    /// The gig was completed with the primary objective achieved.
    GigCompletedSuccessfully,
    /// The gig ended with the primary objective **not** achieved, but the
    /// character survived to fight another day. Partial IP for the effort.
    GigCompletedFailure,
    /// A notably dangerous enemy was defeated (not just wounded or driven
    /// off). GM judgment — the engine trusts the caller to gate this
    /// correctly.
    EnemyDefeatedNotably,
    /// A NET architecture was fully cleared (all ICE defeated or bypassed and
    /// the run completed). See Netrunning rules.
    NetrunCleared,
    /// A narrative Beat was resolved — a story beat or personal arc moment
    /// that moved the fiction meaningfully forward.
    BeatResolved,
    /// The character visited a significant location for the first time.
    FirstTimeVisit,
}

/// Returns the fixed IP awarded for `milestone`.
///
/// These values implement the §0.2 hybrid IP design. The LLM-narrative bonus
/// (a separate per-session cap) is NOT included here.
///
/// | Milestone                    | IP |
/// |------------------------------|----|
/// | `GigStarted`                 |  0 |
/// | `GigCompletedSuccessfully`   | 30 |
/// | `GigCompletedFailure`        | 15 |
/// | `EnemyDefeatedNotably`       | 50 |
/// | `NetrunCleared`              | 30 |
/// | `BeatResolved`               | 10 |
/// | `FirstTimeVisit`             |  5 |
pub fn milestone_ip(milestone: IpMilestone) -> u32 {
    match milestone {
        IpMilestone::GigStarted => 0,
        IpMilestone::GigCompletedSuccessfully => 30,
        IpMilestone::GigCompletedFailure => 15,
        IpMilestone::EnemyDefeatedNotably => 50,
        IpMilestone::NetrunCleared => 30,
        IpMilestone::BeatResolved => 10,
        IpMilestone::FirstTimeVisit => 5,
    }
}

/// Adds the fixed IP for `milestone` to `character.improvement_points` and
/// returns the amount awarded.
///
/// The returned value is the delta (same as [`milestone_ip`]`(milestone)`).
/// The caller may use it for logging or aggregating session totals.
///
/// # Overflow
///
/// `improvement_points` is `u32`. Saturating addition is **not** used
/// intentionally — in practice a character cannot accumulate enough IP to
/// overflow a `u32` within a campaign lifetime (max ~50 IP per event ×
/// plausible sessions). If the pool ever overflows it is a programming error
/// upstream, and the panic is the right signal.
pub fn award_milestone_ip(character: &mut Character, milestone: IpMilestone) -> u32 {
    let ip = milestone_ip(milestone);
    character.improvement_points += ip;
    ip
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::data::{ArmorKind, ArmorPiece, SkillSet, StatBlock, Wounds};
    use crate::character::{Inventory, Lifepath, Role, WornArmor};
    use crate::effects::{EffectStack, WoundState};
    use crate::types::{CharacterId, Eurobucks};
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

    #[test]
    fn test_milestone_ip_values() {
        assert_eq!(milestone_ip(IpMilestone::GigStarted), 0);
        assert_eq!(milestone_ip(IpMilestone::GigCompletedSuccessfully), 30);
        assert_eq!(milestone_ip(IpMilestone::GigCompletedFailure), 15);
        assert_eq!(milestone_ip(IpMilestone::EnemyDefeatedNotably), 50);
        assert_eq!(milestone_ip(IpMilestone::NetrunCleared), 30);
        assert_eq!(milestone_ip(IpMilestone::BeatResolved), 10);
        assert_eq!(milestone_ip(IpMilestone::FirstTimeVisit), 5);
    }

    #[test]
    fn test_award_milestone_ip_increments_pool() {
        let mut c = make_character();
        assert_eq!(c.improvement_points, 0);

        let awarded = award_milestone_ip(&mut c, IpMilestone::GigCompletedSuccessfully);
        assert_eq!(awarded, 30);
        assert_eq!(c.improvement_points, 30);

        let awarded2 = award_milestone_ip(&mut c, IpMilestone::BeatResolved);
        assert_eq!(awarded2, 10);
        assert_eq!(c.improvement_points, 40);
    }

    #[test]
    fn test_gig_started_zero_ip() {
        let mut c = make_character();
        let awarded = award_milestone_ip(&mut c, IpMilestone::GigStarted);
        assert_eq!(awarded, 0);
        assert_eq!(c.improvement_points, 0);
    }
}
