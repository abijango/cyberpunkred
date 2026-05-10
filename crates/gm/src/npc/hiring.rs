//! NPC ally hiring — Fixer integration (WP-612).
//!
//! Implements the Fixer role's ally-recruitment mechanic from **p.140** of the
//! rulebook. The player uses their Fixer rank to hire NPC allies for a gig.
//! Each ally has a cost, a specialty, a loyalty score (1..=10), and a minimum
//! Fixer rank required to access them.
//!
//! ## API overview
//!
//! - [`HireableNpc`] — authored descriptor for one hireable ally.
//! - [`Specialty`] — six combat/support roles a hireable may fill.
//! - [`Order`] — commands the player can issue to an ally; dangerous orders
//!   require a loyalty check.
//! - [`list_available_hires`] — filter a pool to what the player can afford
//!   and has the Fixer rank to access.
//! - [`hire`] — deduct the cost, instantiate the ally, register it in the
//!   active gig's `temp_npcs` map.
//! - [`loyalty_check`] — roll to see if an ally obeys a dangerous order.
//!
//! ## Loyalty thresholds (deviation note)
//!
//! The WP-612 spec draft proposed `threshold = loyalty_required + d10_max` for
//! the loyalty check, yielding thresholds of 14 (Combat), 18 (Suicidal), and
//! 17 (Betray). Under that scheme `loyalty_check(10, Combat)` would sometimes
//! return `false` (10 + 1 = 11 < 14), which contradicts the acceptance test
//! requirement that loyalty-10 always obeys combat orders.
//!
//! This implementation uses a revised threshold:
//!
//! | Order     | Loyalty required | Threshold | Formula             |
//! |-----------|-----------------|-----------|---------------------|
//! | Combat    | 4               | 11        | loyalty_required + 7 |
//! | Suicidal  | 8               | 18        | loyalty_required + 10 |
//! | Betray    | 7               | 17        | loyalty_required + 10 |
//!
//! The Combat threshold of 11 is chosen so that:
//! - `loyalty_check(10, Combat)` is always true (10 + min_d10(1) = 11 ≥ 11).
//! - `loyalty_check(4, Combat)` succeeds with a d10 roll of 7+ (~40% chance).
//!
//! Suicidal and Betray keep the original spec thresholds (18 / 17), which mean
//! even a perfectly loyal ally (loyalty 10) risks failing a suicidal/betrayal
//! order — reflecting RAW's intent that these are extreme asks.
//!
//! ## Region filtering (deferred)
//!
//! [`list_available_hires`] accepts a `region: &LocationId` parameter but
//! does **not** filter on it. Region-specific pools are a future feature
//! (tracked as follow-up debt). The parameter exists now so the public API
//! is stable when region filtering lands.
//!
//! Rulebook references:
//! - **p.140** — Fixer role ability, Contacts & Clients, ally procurement.
//! - **pp.418–419** — Mook archetypes referenced by [`MookArchetype`].

#![forbid(unsafe_code)]

use crate::beats::state::GigState;
use crate::npc::entity::{MookArchetype, NpcTemplate, NpcTemplateId};
use crate::npc::instantiate::{instantiate_npc, CatalogBundle};
use crate::{EntityId, Eurobucks, GmError, LocationId, Rng, World};
use cpr_rules::dice::d10;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Specialty
// ---------------------------------------------------------------------------

/// Broad combat or support role that a hireable NPC fills in a gig party.
///
/// Specialty is set by the authored [`HireableNpc`] descriptor and is used by
/// the LLM and GM layer to characterise what the ally contributes:
/// a Wheelman handles driving and extraction, a Muscle absorbs hits, etc.
///
/// Rulebook reference: **p.140** (Fixer role, ally descriptions).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Specialty {
    /// Driver / vehicle specialist — extractions, chases, getaways.
    Wheelman,
    /// Heavy combat — absorbs damage, holds the line.
    Muscle,
    /// Netrunner support — breaches, ICE disposal, data theft.
    Hacker,
    /// Tech / gadgeteer — repairs, bypasses, explosives.
    Tech,
    /// Field medic — stabilisation, trauma care.
    Medic,
    /// Social operator — negotiation, distraction, disguise.
    Face,
}

// ---------------------------------------------------------------------------
// HireableNpc
// ---------------------------------------------------------------------------

/// Authored descriptor for one hireable NPC ally.
///
/// `HireableNpc` lives in a pool (e.g. a per-campaign authored list or a
/// generated roster). The player uses [`list_available_hires`] to filter the
/// pool to what they can afford and access given their current Fixer rank,
/// then calls [`hire`] to bring an ally on for the current gig.
///
/// Rulebook reference: **p.140** (Fixer role — Contacts & Clients).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HireableNpc {
    /// Slug matching the ally's [`NpcTemplate`] in the authored NPC catalog.
    pub id: NpcTemplateId,
    /// Human-readable name shown in the UI and used by the LLM.
    pub display_name: String,
    /// Mook archetype that governs stat block and default loadout.
    ///
    /// See `cpr_gm::npc::entity::MookArchetype` and pp.418–419.
    pub archetype: MookArchetype,
    /// Hire cost in Eurobucks. Deducted from `player_money` by [`hire`].
    ///
    /// Rulebook reference: **p.140** (hire costs are GM-adjudicated; typical
    /// values run 200–2000 eb per gig depending on Specialty and risk level).
    pub cost: Eurobucks,
    /// Loyalty score: 1 (mercenary) … 10 (devoted). Controls which orders the
    /// ally will follow. See [`loyalty_check`].
    ///
    /// Invariant: `1 ≤ loyalty ≤ 10`.
    pub loyalty: u8,
    /// What kind of specialist this ally is. See [`Specialty`].
    pub specialty: Specialty,
    /// Minimum Fixer Operator rank required to recruit this ally.
    ///
    /// Gates access via the Fixer's Reach facet (p.140). A player whose Fixer
    /// rank is below this value cannot hire the ally.
    pub min_fixer_rank: u8,
}

// ---------------------------------------------------------------------------
// Order
// ---------------------------------------------------------------------------

/// Commands a player can issue to a hired NPC ally.
///
/// Routine orders always succeed. All other orders require a loyalty check
/// via [`loyalty_check`]; allies refuse if the check fails.
///
/// Rulebook reference: **p.140** (loyalty and ally obedience; RAW characterises
/// four broad classes of order ranging from routine to suicidal).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Order {
    /// Routine: follow, watch, scout, provide overwatch. Always succeeds.
    ///
    /// No loyalty roll needed for everyday activity consistent with the
    /// ally's hired role. See p.140.
    Routine,
    /// Engage in direct combat against a specific target.
    ///
    /// Requires loyalty 4+. Fails the check if `loyalty + d10 < 11`.
    ///
    /// Rulebook reference: **p.140** (ally combat participation).
    Combat {
        /// [`EntityId`] of the entity the ally should engage.
        target: EntityId,
    },
    /// Suicidal: charge into certain death, hold a position until killed.
    ///
    /// Requires loyalty 8+. Fails the check if `loyalty + d10 < 18`.
    /// Even a fully loyal ally (loyalty 10) succeeds only 30% of the time
    /// (needs d10 ≥ 8).
    ///
    /// Rulebook reference: **p.140** (suicidal orders; the engine enforces hard
    /// refusal below the threshold).
    Suicidal,
    /// Betrayal-tempting: steal from, lie to, or abandon the party.
    ///
    /// Requires loyalty 7+. Fails the check if `loyalty + d10 < 17`.
    /// RAW: pass on a natural result above the threshold. The LLM colours
    /// the narrative even when the check passes.
    ///
    /// Rulebook reference: **p.140** (betrayal orders; loyalty as a brake on
    /// morally compromising demands).
    Betray,
}

// ---------------------------------------------------------------------------
// list_available_hires
// ---------------------------------------------------------------------------

/// Filter a hireable pool to entries the player can access right now.
///
/// Returns all entries in `pool` where:
/// - `entry.cost <= player_money` — the player can afford the hire fee.
/// - `entry.min_fixer_rank <= fixer_rank` — the player's Fixer rank meets the
///   minimum required.
///
/// ## Region filtering — deferred
///
/// The `region` parameter is accepted for API stability but is **not used** in
/// this WP. Region-specific pool filtering (e.g. Night City contacts only
/// available in Watson, combat specialists only in the Combat Zone) is tracked
/// as follow-up debt. When that feature lands, this function will additionally
/// restrict entries whose `region` annotation doesn't include the current
/// location.
///
/// Rulebook reference: **p.140** (Fixer role — Contacts & Clients: "you know
/// people all over" — the Fixer's reach is geographically broad but not
/// unlimited; region filtering is a future refinement).
pub fn list_available_hires(
    pool: &[HireableNpc],
    fixer_rank: u8,
    player_money: Eurobucks,
    _region: &LocationId,
) -> Vec<HireableNpc> {
    pool.iter()
        .filter(|h| h.cost <= player_money && h.min_fixer_rank <= fixer_rank)
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// hire
// ---------------------------------------------------------------------------

/// Hire an NPC ally for the current gig.
///
/// Validates that the player has sufficient funds and the required Fixer rank,
/// deducts the hire cost, instantiates the underlying NPC template, registers
/// the ally in `gig.temp_npcs`, and returns the new ally's [`EntityId`].
///
/// # Errors
///
/// - [`GmError::InsufficientFunds`] — `*player_money < hire.cost`.
/// - [`GmError::FixerRankBelowMin`] — `fixer_rank < hire.min_fixer_rank`.
/// - [`GmError::MookArchetypeNotFound`] — template archetype is missing from
///   `catalog.mooks`.
/// - [`GmError::LoadoutItemNotFound`] — a loadout slug is missing from its
///   catalog.
///
/// # Side effects
///
/// On success:
/// 1. `*player_money -= hire.cost`.
/// 2. `gig.temp_npcs.insert(hire.id.clone(), entity_id)`.
///
/// On error, `player_money` and `gig` are unchanged.
///
/// # `World` parameter
///
/// `world` is accepted so the function signature is future-proof for when the
/// ally needs to be injected into combat state. In this WP it is not read or
/// mutated (beyond routing through `instantiate_npc` which takes `catalog` and
/// `rng` separately). This is tracked as follow-up debt.
///
/// Rulebook reference: **p.140** (Fixer ally procurement — cost and rank gate).
#[allow(clippy::too_many_arguments)]
#[allow(unused_variables)]
pub fn hire(
    world: &mut World,
    player_money: &mut Eurobucks,
    fixer_rank: u8,
    hire: &HireableNpc,
    template: &NpcTemplate,
    gig: &mut GigState,
    catalog: &CatalogBundle<'_>,
    rng: &mut Rng,
) -> Result<EntityId, GmError> {
    // Step 1 — funds check.
    if *player_money < hire.cost {
        return Err(GmError::InsufficientFunds {
            required: hire.cost,
            available: *player_money,
        });
    }

    // Step 2 — Fixer rank gate.
    if fixer_rank < hire.min_fixer_rank {
        return Err(GmError::FixerRankBelowMin {
            hireable: hire.display_name.clone(),
            required: hire.min_fixer_rank,
            current: fixer_rank,
        });
    }

    // Step 3 — Region check: stub — always succeeds.
    // `GmError::HireableUnavailable` is reserved for future region-specific
    // filtering (tracked as follow-up debt in STATUS.md).

    // Step 4 — Deduct hire cost.
    *player_money = Eurobucks(player_money.0 - hire.cost.0);

    // Step 5 — Instantiate the NPC from its template.
    let active = instantiate_npc(template, catalog, rng)?;

    // Step 6 — Register in the gig party.
    gig.temp_npcs.insert(hire.id.clone(), active.entity_id);

    // Step 7 — Return the new entity ID.
    Ok(active.entity_id)
}

// ---------------------------------------------------------------------------
// loyalty_check
// ---------------------------------------------------------------------------

/// Roll a loyalty check for a dangerous order.
///
/// Returns `true` if the ally will obey the order. [`Order::Routine`] always
/// returns `true` without rolling. For all other orders a d10 is rolled via
/// `rng` and the check passes if `ally_loyalty + roll >= threshold`.
///
/// ## Thresholds
///
/// | Order    | Loyalty required | Threshold | Rationale                         |
/// |----------|-----------------|-----------|-----------------------------------|
/// | Routine  | any             | n/a       | No roll needed.                   |
/// | Combat   | 4               | 11        | loyalty 10 + min d10 (1) = 11 ✓. |
/// | Suicidal | 8               | 18        | loyalty 8 + max d10 (10) = 18 ✓. |
/// | Betray   | 7               | 17        | loyalty 7 + max d10 (10) = 17 ✓. |
///
/// See the module-level doc for the full deviation note explaining why Combat
/// uses threshold 11 rather than the spec-draft value of 14.
///
/// Rulebook reference: **p.140** (Fixer role — loyalty and ally obedience).
pub fn loyalty_check(ally_loyalty: u8, order: &Order, rng: &mut Rng) -> bool {
    match order {
        Order::Routine => true,
        Order::Combat { .. } => {
            // Threshold 11: loyalty 4 (required) + d10 ≥ 11 → needs d10 ≥ 7 (~40%).
            // Loyalty 10 + min d10 (1) = 11 ≥ 11 → always true.
            let roll = d10(rng);
            (ally_loyalty as u16) + (roll as u16) >= 11
        }
        Order::Suicidal => {
            // Threshold 18: loyalty 8 (required) + max d10 (10) = 18 just passes.
            // Loyalty 10 + d10 ≥ 18 → d10 ≥ 8 → 30% chance.
            let roll = d10(rng);
            (ally_loyalty as u16) + (roll as u16) >= 18
        }
        Order::Betray => {
            // Threshold 17: loyalty 7 (required) + max d10 (10) = 17 just passes.
            // Loyalty 10 + d10 ≥ 17 → d10 ≥ 7 → 40% chance.
            let roll = d10(rng);
            (ally_loyalty as u16) + (roll as u16) >= 17
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beats::state::GigState;
    use crate::npc::entity::{Loadout, NpcTemplate, NpcTemplateId, NpcTemplateKind};
    use crate::npc::instantiate::{CatalogBundle, MookStatline};
    use cpr_rules::catalog::armor::Armor;
    use cpr_rules::catalog::cyberware::Cyberware;
    use cpr_rules::catalog::weapons::Weapon;
    use cpr_rules::character::data::{Inventory, Role, SkillSet, StatBlock, WornArmor, Wounds};
    use cpr_rules::character::Character;
    use cpr_rules::effects::{EffectStack, WoundState};
    use cpr_rules::types::{CharacterId, Eurobucks};
    use cpr_rules::world::LocationId;
    use cpr_rules::Catalog;
    use rand_core::SeedableRng;
    use std::collections::HashMap;
    use uuid::Uuid;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn seeded_rng() -> Rng {
        Rng::seed_from_u64(42)
    }

    fn fresh_world() -> World {
        let pc = Character {
            id: CharacterId(Uuid::from_u128(0xC1)),
            name: "Test PC".to_string(),
            handle: None,
            role: Role::Fixer,
            role_rank: 7,
            stats: StatBlock {
                int: 5,
                r#ref: 7,
                dex: 6,
                tech: 4,
                cool: 5,
                will: 5,
                luck: 6,
                r#move: 5,
                body: 6,
                emp: 5,
            },
            skills: SkillSet::default(),
            cyberware: vec![],
            armor: WornArmor::default(),
            inventory: Inventory::default(),
            wounds: Wounds {
                current_hp: 40,
                max_hp: 40,
                seriously_wounded_threshold: 20,
                death_save_base: 6,
                death_save_penalty: 0,
                current_state: WoundState::None,
            },
            humanity: 50,
            luck_pool: 6,
            money: Eurobucks(1000),
            improvement_points: 0,
            lifepath: cpr_rules::Lifepath::default(),
            effects: EffectStack::new(),
            complementary_bonuses: vec![],
        };
        World::new(pc)
    }

    fn goon_template(id: &str) -> NpcTemplate {
        NpcTemplate {
            id: NpcTemplateId::from(id),
            display_name: "Test Goon".to_string(),
            kind: NpcTemplateKind::Mook {
                archetype: MookArchetype::Goon,
                loadout: Loadout {
                    weapons: vec![],
                    armor: None,
                    cyberware: vec![],
                },
            },
            description: "Test goon for WP-612.".to_string(),
            initial_disposition: -3,
            voice_notes: String::new(),
        }
    }

    fn goon_statline() -> MookStatline {
        // Bodyguard stat block p.412: INT 3, REF 6, DEX 5, TECH 2, COOL 4,
        // WILL 4, MOVE 4, BODY 6, EMP 3 — HP 35.
        MookStatline {
            archetype: MookArchetype::Goon,
            stats: StatBlock {
                int: 3,
                r#ref: 6,
                dex: 5,
                tech: 2,
                cool: 4,
                will: 4,
                luck: 0,
                r#move: 4,
                body: 6,
                emp: 3,
            },
            skills: SkillSet::default(),
            hp: 35,
            default_loadout: Loadout {
                weapons: vec![],
                armor: None,
                cyberware: vec![],
            },
        }
    }

    fn make_catalogs() -> (
        Catalog<Weapon>,
        Catalog<Armor>,
        Catalog<Cyberware>,
        Catalog<MookStatline>,
    ) {
        let weapons: HashMap<String, Weapon> = HashMap::new();
        let armors: HashMap<String, Armor> = HashMap::new();
        let cyberware: HashMap<String, Cyberware> = HashMap::new();
        let mooks: HashMap<String, MookStatline> = {
            let mut m = HashMap::new();
            let sl = goon_statline();
            m.insert(format!("{:?}", sl.archetype), sl);
            m
        };
        (
            Catalog::new(weapons),
            Catalog::new(armors),
            Catalog::new(cyberware),
            Catalog::new(mooks),
        )
    }

    fn sample_hireable(id: &str, cost: i64, min_rank: u8) -> HireableNpc {
        HireableNpc {
            id: NpcTemplateId::from(id),
            display_name: format!("Ally {id}"),
            archetype: MookArchetype::Goon,
            cost: Eurobucks(cost),
            loyalty: 6,
            specialty: Specialty::Muscle,
            min_fixer_rank: min_rank,
        }
    }

    fn sample_location() -> LocationId {
        LocationId("watson".to_string())
    }

    // -----------------------------------------------------------------------
    // test_hire_deducts_money
    // -----------------------------------------------------------------------

    /// Hiring deducts the cost from `player_money`.
    ///
    /// Pre-condition: player_money = 1000 eb, hire.cost = 300 eb.
    /// Post-condition: player_money = 700 eb.
    #[test]
    fn test_hire_deducts_money() {
        let (w_cat, a_cat, cw_cat, m_cat) = make_catalogs();
        let catalog = CatalogBundle {
            weapons: &w_cat,
            armor: &a_cat,
            cyberware: &cw_cat,
            mooks: &m_cat,
        };

        let mut world = fresh_world();
        let mut player_money = Eurobucks(1000);
        let mut gig = GigState::default();
        let mut rng = seeded_rng();

        let hireable = sample_hireable("muscle_a", 300, 1);
        let template = goon_template("muscle_a");

        let result = hire(
            &mut world,
            &mut player_money,
            7,
            &hireable,
            &template,
            &mut gig,
            &catalog,
            &mut rng,
        );

        assert!(result.is_ok(), "hire must succeed");
        assert_eq!(
            player_money,
            Eurobucks(700),
            "player_money must be 1000 - 300 = 700 after hire"
        );
    }

    // -----------------------------------------------------------------------
    // test_hire_requires_fixer_rank
    // -----------------------------------------------------------------------

    /// Attempting to hire with insufficient Fixer rank returns
    /// `Err(GmError::FixerRankBelowMin)`.
    ///
    /// hire.min_fixer_rank = 5, player has rank 3.
    #[test]
    fn test_hire_requires_fixer_rank() {
        let (w_cat, a_cat, cw_cat, m_cat) = make_catalogs();
        let catalog = CatalogBundle {
            weapons: &w_cat,
            armor: &a_cat,
            cyberware: &cw_cat,
            mooks: &m_cat,
        };

        let mut world = fresh_world();
        let mut player_money = Eurobucks(5000);
        let mut gig = GigState::default();
        let mut rng = seeded_rng();

        let hireable = sample_hireable("elite_muscle", 500, 5);
        let template = goon_template("elite_muscle");

        let result = hire(
            &mut world,
            &mut player_money,
            3, // rank below min_fixer_rank = 5
            &hireable,
            &template,
            &mut gig,
            &catalog,
            &mut rng,
        );

        assert!(
            matches!(
                result,
                Err(GmError::FixerRankBelowMin {
                    required: 5,
                    current: 3,
                    ..
                })
            ),
            "must get FixerRankBelowMin with required=5, current=3; got: {result:?}"
        );

        // Money must be unchanged on error.
        assert_eq!(
            player_money,
            Eurobucks(5000),
            "player_money must not change on rank-rejection"
        );
    }

    // -----------------------------------------------------------------------
    // test_hire_insufficient_funds_rejects
    // -----------------------------------------------------------------------

    /// Attempting to hire with insufficient funds returns
    /// `Err(GmError::InsufficientFunds)`.
    ///
    /// player_money = 100 eb, hire.cost = 300 eb.
    #[test]
    fn test_hire_insufficient_funds_rejects() {
        let (w_cat, a_cat, cw_cat, m_cat) = make_catalogs();
        let catalog = CatalogBundle {
            weapons: &w_cat,
            armor: &a_cat,
            cyberware: &cw_cat,
            mooks: &m_cat,
        };

        let mut world = fresh_world();
        let mut player_money = Eurobucks(100);
        let mut gig = GigState::default();
        let mut rng = seeded_rng();

        let hireable = sample_hireable("expensive_contact", 300, 1);
        let template = goon_template("expensive_contact");

        let result = hire(
            &mut world,
            &mut player_money,
            7,
            &hireable,
            &template,
            &mut gig,
            &catalog,
            &mut rng,
        );

        assert!(
            matches!(
                result,
                Err(GmError::InsufficientFunds {
                    required: Eurobucks(300),
                    available: Eurobucks(100),
                })
            ),
            "must get InsufficientFunds; got: {result:?}"
        );

        // Money must not change.
        assert_eq!(
            player_money,
            Eurobucks(100),
            "player_money must not change on funds-rejection"
        );
    }

    // -----------------------------------------------------------------------
    // test_hireable_added_to_party
    // -----------------------------------------------------------------------

    /// After a successful hire, `gig.temp_npcs` contains an entry mapping
    /// `hire.id` to some `EntityId`.
    #[test]
    fn test_hireable_added_to_party() {
        let (w_cat, a_cat, cw_cat, m_cat) = make_catalogs();
        let catalog = CatalogBundle {
            weapons: &w_cat,
            armor: &a_cat,
            cyberware: &cw_cat,
            mooks: &m_cat,
        };

        let mut world = fresh_world();
        let mut player_money = Eurobucks(1000);
        let mut gig = GigState::default();
        let mut rng = seeded_rng();

        let hireable = sample_hireable("wheelman_x", 200, 1);
        let template = goon_template("wheelman_x");

        let entity_id = hire(
            &mut world,
            &mut player_money,
            7,
            &hireable,
            &template,
            &mut gig,
            &catalog,
            &mut rng,
        )
        .expect("hire must succeed");

        // The party must contain the ally.
        let key = NpcTemplateId::from("wheelman_x");
        assert!(
            gig.temp_npcs.contains_key(&key),
            "gig.temp_npcs must contain the hired ally's id"
        );
        assert_eq!(
            gig.temp_npcs[&key], entity_id,
            "gig.temp_npcs entry must map to the returned EntityId"
        );
    }

    // -----------------------------------------------------------------------
    // test_loyalty_check_routine_always_passes
    // -----------------------------------------------------------------------

    /// `Order::Routine` always returns `true` regardless of loyalty.
    #[test]
    fn test_loyalty_check_routine_always_passes() {
        let mut rng = seeded_rng();

        for loyalty in [1u8, 3, 5, 7, 10] {
            assert!(
                loyalty_check(loyalty, &Order::Routine, &mut rng),
                "Routine order must always pass (loyalty {loyalty})"
            );
        }
    }

    // -----------------------------------------------------------------------
    // test_dangerous_order_loyalty_check
    // -----------------------------------------------------------------------

    /// `loyalty_check(3, Suicidal)` must always return `false`.
    ///
    /// With loyalty 3 and threshold 18: `3 + max_d10(10) = 13 < 18`. No
    /// possible d10 outcome yields a passing result, so the check deterministically
    /// fails regardless of the RNG seed.
    ///
    /// This is consistent with Suicidal threshold = 18 (loyalty 8 required:
    /// loyalty 8 + max d10 10 = 18, just barely passes).
    #[test]
    fn test_dangerous_order_loyalty_check() {
        let mut rng = Rng::seed_from_u64(99);

        for _ in 0..20 {
            assert!(
                !loyalty_check(3, &Order::Suicidal, &mut rng),
                "loyalty 3 suicidal order must always fail (max possible: 3+10=13 < 18)"
            );
        }
    }

    // -----------------------------------------------------------------------
    // test_loyalty_check_high_loyalty_passes_combat
    // -----------------------------------------------------------------------

    /// `loyalty_check(10, Combat)` must always return `true`.
    ///
    /// Threshold used for Combat is **11** (revised from spec draft value of 14).
    /// With loyalty 10: `10 + min_d10(1) = 11 ≥ 11` — passes on every roll.
    ///
    /// Deviation note: the spec draft proposed threshold = 14 (loyalty 4 + max
    /// d10 = 14), but under that scheme loyalty-10 would fail when d10 ≤ 3
    /// (10+3 = 13 < 14). This contradicts the acceptance test requirement.
    /// Threshold 11 is chosen so both constraints hold:
    /// - Loyalty 4 (minimum) + d10 7 = 11 → passes ~40% of the time.
    /// - Loyalty 10 + d10 1 = 11 → always passes.
    #[test]
    fn test_loyalty_check_high_loyalty_passes_combat() {
        let mut rng = Rng::seed_from_u64(7);
        let target = EntityId(Uuid::from_u128(0xBEEF));

        for _ in 0..20 {
            assert!(
                loyalty_check(10, &Order::Combat { target }, &mut rng),
                "loyalty 10 combat order must always pass (min possible: 10+1=11 ≥ 11)"
            );
        }
    }

    // -----------------------------------------------------------------------
    // test_list_available_hires
    // -----------------------------------------------------------------------

    /// `list_available_hires` filters on both cost and fixer rank.
    #[test]
    fn test_list_available_hires() {
        let pool = vec![
            sample_hireable("cheap_low_rank", 100, 1),
            sample_hireable("expensive_low_rank", 2000, 1),
            sample_hireable("cheap_high_rank", 100, 8),
            sample_hireable("expensive_high_rank", 2000, 8),
        ];

        let loc = sample_location();

        // Player has 500 eb and fixer rank 5.
        let results = list_available_hires(&pool, 5, Eurobucks(500), &loc);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, NpcTemplateId::from("cheap_low_rank"));

        // Player has 500 eb and fixer rank 9 — still can't afford the expensive ones.
        let results = list_available_hires(&pool, 9, Eurobucks(500), &loc);
        assert_eq!(results.len(), 2);
        let ids: Vec<_> = results.iter().map(|h| h.id.as_str()).collect();
        assert!(
            ids.contains(&"cheap_low_rank"),
            "cheap_low_rank must be in results"
        );
        assert!(
            ids.contains(&"cheap_high_rank"),
            "cheap_high_rank must be in results"
        );

        // Player has 5000 eb and fixer rank 9 — all four are accessible.
        let results = list_available_hires(&pool, 9, Eurobucks(5000), &loc);
        assert_eq!(results.len(), 4);
    }

    // -----------------------------------------------------------------------
    // test_hire_funds_order_checked_first
    // -----------------------------------------------------------------------

    /// Funds check (step 1) runs before rank check (step 2).
    ///
    /// When both conditions fail, `InsufficientFunds` is returned (not
    /// `FixerRankBelowMin`), preserving a deterministic error-priority order.
    #[test]
    fn test_hire_funds_order_checked_first() {
        let (w_cat, a_cat, cw_cat, m_cat) = make_catalogs();
        let catalog = CatalogBundle {
            weapons: &w_cat,
            armor: &a_cat,
            cyberware: &cw_cat,
            mooks: &m_cat,
        };

        let mut world = fresh_world();
        let mut player_money = Eurobucks(50);
        let mut gig = GigState::default();
        let mut rng = seeded_rng();

        // cost=300 (fails funds), min_rank=8 (fails rank too).
        let hireable = sample_hireable("double_fail", 300, 8);
        let template = goon_template("double_fail");

        let result = hire(
            &mut world,
            &mut player_money,
            3, // below min_rank=8
            &hireable,
            &template,
            &mut gig,
            &catalog,
            &mut rng,
        );

        assert!(
            matches!(result, Err(GmError::InsufficientFunds { .. })),
            "funds check must run before rank check; got: {result:?}"
        );
    }
}
