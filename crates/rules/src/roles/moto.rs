//! Moto (Nomad Role Ability) — WP-519.
//!
//! ## Rulebook mechanics (pp.161–162)
//!
//! The Nomad's Role Ability is **Moto**. It has two components:
//!
//! ### Nomad Vehicle Familiarity (p.161)
//!
//! Being part of a Nomad Family means spending your life in the driver's seat
//! and under the hood. **A Nomad adds their Moto Rank to any Drive Land
//! Vehicle, Pilot Air Vehicle, Pilot Sea Vehicle, Air Vehicle Tech, Land
//! Vehicle Tech, or Sea Vehicle Tech Skill Check they make.**
//!
//! This module expresses the handling bonus as the additive value applied to
//! those six skill checks (equal to `rank`).
//!
//! ### Family Motorpool (pp.161–162)
//!
//! Whenever a Nomad increases their Role Ability Rank, they may add a stock
//! vehicle from their Family Motorpool. The vehicles available depend on rank:
//!
//! | Rank   | Vehicle classes available                          |
//! |--------|----------------------------------------------------|
//! | 1 to 4 | Compact Groundcar, Gyrocopter, Jetski, Roadbike   |
//! | 5 to 6 | + Helicopter, High Performance Groundcar, Speedboat |
//! | 7 to 8 | + AV-4, Cabin Cruiser, Superbike                  |
//! | 9 to 10| + Aerozep, AV-9, Super Groundcar, Yacht            |
//!
//! The table lists specific named vehicles; this module maps them to
//! [`VehicleKind`] buckets because the `Family Motorpool` table describes
//! vehicle *classes* rather than catalog slugs, and the public API contract
//! uses `VehicleKind`.
//!
//! **Mapping note:** the rulebook names individual vehicle models
//! (e.g. "Compact Groundcar", "AV-4"). The mapping to [`VehicleKind`]
//! follows the p.190 vehicle table:
//!
//! - Roadbike, Superbike → [`VehicleKind::Bike`]
//! - Compact Groundcar, High Performance Groundcar, Super Groundcar → [`VehicleKind::Car`]
//! - Jetski, Speedboat, Cabin Cruiser, Yacht → [`VehicleKind::Boat`]
//! - Gyrocopter, Helicopter, AV-4 (Aerodyne), AV-9 (Super Aerodyne), Aerozep → [`VehicleKind::AV`]
//!
//! At rank 9-10 the Nomad has access to all four kinds: `Bike`, `Car`, `Boat`,
//! and `AV`. At rank 1–4 access is limited to `Bike`, `Car`, `Boat`, and `AV`
//! at the lower end (Roadbike, Compact Groundcar, Jetski, Gyrocopter).
//!
//! ## What this module provides
//!
//! - [`moto_rank`] — returns the effective Moto rank for a character.
//! - [`vehicle_handling_bonus`] — handling bonus added to drive/pilot/tech rolls.
//! - [`family_vehicles_at_rank`] — vehicle kinds available from the Family Motorpool.
//!
//! See pp.161–162.

use crate::catalog::vehicles::VehicleKind;
use crate::character::data::Role;
use crate::character::Character;

/// Returns the character's effective Moto rank.
///
/// Per p.161, Moto is the Nomad's Role Ability and its rank equals
/// `character.role_rank` when the character is a [`Role::Nomad`]. For any
/// other role the ability does not apply; this function returns `0` so
/// callers can skip application without special-casing the role check.
///
/// # Examples
///
/// ```
/// # use cpr_rules::roles::moto::moto_rank;
/// # use cpr_rules::character::Character;
/// // See test_moto_rank_for_nomad.
/// ```
///
/// See p.161.
pub fn moto_rank(character: &Character) -> u8 {
    // See p.161: Moto belongs exclusively to the Nomad role.
    if character.role == Role::Nomad {
        character.role_rank
    } else {
        0
    }
}

/// Vehicle handling bonus for Drive/Pilot/Tech skill checks at a given Moto rank.
///
/// Per p.161: "A Nomad adds their Moto Rank to any Drive Land Vehicle, Pilot
/// Air Vehicle, Pilot Sea Vehicle, Air Vehicle Tech, Land Vehicle Tech, or Sea
/// Vehicle Tech Skill Check they make."
///
/// The bonus is additive and equal to the Moto rank. Returns `0` for rank `0`
/// (non-Nomads or untrained Nomads).
///
/// The six skill checks this bonus applies to:
/// - Drive Land Vehicle
/// - Pilot Air Vehicle
/// - Pilot Sea Vehicle
/// - Air Vehicle Tech
/// - Land Vehicle Tech
/// - Sea Vehicle Tech
///
/// See p.161.
pub fn vehicle_handling_bonus(rank: u8) -> i8 {
    // See p.161: "A Nomad adds their Moto Rank to any Drive Land Vehicle,
    // Pilot Air Vehicle, Pilot Sea Vehicle, Air Vehicle Tech, Land Vehicle
    // Tech, or Sea Vehicle Tech Skill Check they make."
    rank as i8
}

/// Vehicle kinds available from the Nomad's Family Motorpool at a given rank.
///
/// Per pp.161–162, whenever a Nomad increases their Moto Rank they may add a
/// stock vehicle (with minimum specs of their Moto Rank or lower) from the
/// Family Motorpool. The rulebook table on p.162 specifies which vehicle
/// models are available at each rank tier. This function returns the
/// [`VehicleKind`] buckets that include those models.
///
/// ## Rank → available vehicle kinds (p.162 table)
///
/// | Rank   | Vehicles added                                     | Kinds                    |
/// |--------|----------------------------------------------------|--------------------------|
/// | 1 to 4 | Compact Groundcar, Gyrocopter, Jetski, Roadbike   | Car, AV, Boat, Bike      |
/// | 5 to 6 | + Helicopter, High Performance Groundcar, Speedboat | (same kinds, new models) |
/// | 7 to 8 | + AV-4, Cabin Cruiser, Superbike                  | (same kinds, new models) |
/// | 9 to 10| + Aerozep, AV-9, Super Groundcar, Yacht            | (same kinds, new models) |
///
/// Because every rank tier covers all four `VehicleKind`s (Bike, Car, Boat,
/// AV), the returned `Vec` contains all four kinds for any rank ≥ 1. At rank
/// 0 the vec is empty (the Nomad has no Family Motorpool access). The
/// cumulative set does not change across tiers — only the *specific models*
/// that become eligible grow with rank. Callers who need to gate specific
/// model selection by rank should compare the model's required rank against
/// `rank` directly.
///
/// Returns an empty `Vec` for `rank == 0`.
///
/// See p.162.
pub fn family_vehicles_at_rank(rank: u8) -> Vec<VehicleKind> {
    // See p.162 Family Motorpool table.
    // Rank 1-4:  Compact Groundcar (Car), Gyrocopter (AV), Jetski (Boat), Roadbike (Bike)
    // Rank 5-6:  + Helicopter (AV), High Performance Groundcar (Car), Speedboat (Boat)
    // Rank 7-8:  + AV-4 (AV), Cabin Cruiser (Boat), Superbike (Bike)
    // Rank 9-10: + Aerozep (AV), AV-9 (AV), Super Groundcar (Car), Yacht (Boat)
    //
    // All four VehicleKind buckets (Bike, Car, AV, Boat) appear at rank 1-4
    // already (Roadbike=Bike, Compact Groundcar=Car, Gyrocopter=AV,
    // Jetski=Boat), so every rank ≥ 1 returns all four kinds.
    if rank == 0 {
        return Vec::new();
    }
    vec![
        VehicleKind::Bike,
        VehicleKind::Car,
        VehicleKind::AV,
        VehicleKind::Boat,
    ]
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
                r#ref: 7,
                dex: 6,
                tech: 5,
                cool: 6,
                will: 7,
                luck: 6,
                r#move: 6,
                body: 7,
                emp: 5,
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
                death_save_base: 7,
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

    /// `test_moto_rank_for_nomad`: Nomad with role_rank 5 → 5.
    ///
    /// Per p.161: Moto rank equals role_rank for Nomad characters.
    #[test]
    fn test_moto_rank_for_nomad() {
        let character = make_character(Role::Nomad, 5);
        assert_eq!(moto_rank(&character), 5, "Nomad rank 5 → 5 (p.161)");
    }

    /// `test_moto_rank_zero_for_non_nomad`: Solo role → 0.
    ///
    /// Per p.161: Moto belongs exclusively to the Nomad role.
    #[test]
    fn test_moto_rank_zero_for_non_nomad() {
        let character = make_character(Role::Solo, 5);
        assert_eq!(moto_rank(&character), 0, "Solo → 0 (p.161)");
    }

    /// `test_vehicle_handling_bonus_scales`: handling bonus equals rank.
    ///
    /// Per p.161: "A Nomad adds their Moto Rank to any Drive Land Vehicle,
    /// Pilot Air Vehicle, Pilot Sea Vehicle, Air Vehicle Tech, Land Vehicle
    /// Tech, or Sea Vehicle Tech Skill Check they make."
    #[test]
    fn test_vehicle_handling_bonus_scales() {
        // rank 0 → 0 (no bonus)
        assert_eq!(vehicle_handling_bonus(0), 0, "rank 0 → +0");
        // rank 1 → 1
        assert_eq!(vehicle_handling_bonus(1), 1, "rank 1 → +1 (p.161)");
        // rank 5 → 5
        assert_eq!(vehicle_handling_bonus(5), 5, "rank 5 → +5 (p.161)");
        // rank 10 → 10 (maximum rank)
        assert_eq!(vehicle_handling_bonus(10), 10, "rank 10 → +10 (p.161)");
    }

    /// `test_family_vehicles_at_rank`: vehicle kinds available per rank tier.
    ///
    /// Per p.162 Family Motorpool table: all four vehicle kinds are available
    /// at rank ≥ 1; rank 0 returns empty.
    #[test]
    fn test_family_vehicles_at_rank() {
        // rank 0 → empty (no Family Motorpool access)
        let none = family_vehicles_at_rank(0);
        assert!(none.is_empty(), "rank 0 → no vehicles (p.162)");

        // rank 1-4: Compact Groundcar (Car), Gyrocopter (AV), Jetski (Boat),
        // Roadbike (Bike) — all four kinds present.
        for rank in 1u8..=4 {
            let kinds = family_vehicles_at_rank(rank);
            assert!(
                kinds.contains(&VehicleKind::Bike),
                "rank {rank}: Roadbike (Bike) must be available (p.162)"
            );
            assert!(
                kinds.contains(&VehicleKind::Car),
                "rank {rank}: Compact Groundcar (Car) must be available (p.162)"
            );
            assert!(
                kinds.contains(&VehicleKind::AV),
                "rank {rank}: Gyrocopter (AV) must be available (p.162)"
            );
            assert!(
                kinds.contains(&VehicleKind::Boat),
                "rank {rank}: Jetski (Boat) must be available (p.162)"
            );
        }

        // rank 5-6: same four kinds (Helicopter/AV, High Performance Groundcar/Car,
        // Speedboat/Boat join Bike at this tier).
        for rank in 5u8..=6 {
            let kinds = family_vehicles_at_rank(rank);
            assert!(
                kinds.contains(&VehicleKind::AV),
                "rank {rank}: Helicopter (AV) must be available (p.162)"
            );
            assert!(
                kinds.contains(&VehicleKind::Car),
                "rank {rank}: High Performance Groundcar (Car) must be available (p.162)"
            );
            assert!(
                kinds.contains(&VehicleKind::Boat),
                "rank {rank}: Speedboat (Boat) must be available (p.162)"
            );
        }

        // rank 7-8: AV-4 (AV), Cabin Cruiser (Boat), Superbike (Bike).
        for rank in 7u8..=8 {
            let kinds = family_vehicles_at_rank(rank);
            assert!(
                kinds.contains(&VehicleKind::AV),
                "rank {rank}: AV-4 (AV) must be available (p.162)"
            );
            assert!(
                kinds.contains(&VehicleKind::Boat),
                "rank {rank}: Cabin Cruiser (Boat) must be available (p.162)"
            );
            assert!(
                kinds.contains(&VehicleKind::Bike),
                "rank {rank}: Superbike (Bike) must be available (p.162)"
            );
        }

        // rank 9-10: Aerozep/AV-9 (AV), Super Groundcar (Car), Yacht (Boat).
        for rank in 9u8..=10 {
            let kinds = family_vehicles_at_rank(rank);
            assert!(
                kinds.contains(&VehicleKind::AV),
                "rank {rank}: Aerozep/AV-9 (AV) must be available (p.162)"
            );
            assert!(
                kinds.contains(&VehicleKind::Car),
                "rank {rank}: Super Groundcar (Car) must be available (p.162)"
            );
            assert!(
                kinds.contains(&VehicleKind::Boat),
                "rank {rank}: Yacht (Boat) must be available (p.162)"
            );
        }
    }
}
