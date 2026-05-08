//! Therapy mechanic (WP-507) — recover Humanity lost to cyberware.
//!
//! ## Rulebook summary (pp.229–230)
//!
//! A skilled Medtech can perform therapy on a patient using their Medicine Role
//! Ability. Each session takes **one full week**; at the end of the week the
//! Medtech rolls against the therapy's DV. On success the patient gains the
//! stated Humanity back. On failure the week was wasted *and the materials are
//! lost* (p.229).
//!
//! The rulebook (p.230) defines two Humanity-recovery therapies:
//!
//! | Therapy                   | Cost    | Materials | DV   | Effect          |
//! |---------------------------|---------|-----------|------|-----------------|
//! | Standard Humanity Loss    | 500eb   | 100eb     | DV15 | +2d6 Humanity   |
//! | Extreme Humanity Loss     | 1,000eb | 500eb     | DV17 | +4d6 Humanity   |
//!
//! ## Design decision / deviation from WP spec
//!
//! The WP spec calls the three kinds `Outpatient`, `Inpatient`, and
//! `Intensive`. The rulebook table (p.230) names them "Standard Humanity Loss"
//! and "Extreme Humanity Loss". We map as follows:
//!
//! - `Outpatient` — no direct rulebook equivalent. Modelled as a low-cost
//!   walk-in session (100eb, 1 day, +1 Humanity). The 100eb/night hospital
//!   stay mentioned on p.229 is the only concrete outpatient-scale cost in the
//!   text; we use that as the basis. Flagged in the PR.
//! - `Inpatient` → "Standard Humanity Loss" (p.230): 500eb, 1 week, +2d6 avg
//!   Humanity. The `humanity_gain` field in the standard session constructor is
//!   set to **7** (average of 2d6 = 7.0).
//! - `Intensive` → "Extreme Humanity Loss" (p.230): 1,000eb, 1 week, +4d6 avg
//!   Humanity. `humanity_gain` is set to **14** (average of 4d6 = 14.0).
//!
//! The `humanity_gain` field is caller-settable, so a future dice-roll wrapper
//! can pass the actual 2d6/4d6 result into [`run_therapy`] without changing the
//! function signature.
//!
//! ## Humanity cap
//!
//! Per p.80, starting Humanity = 10 × EMP. The character's starting EMP is
//! stored in [`crate::character::StatBlock::emp`]. We cap recovered Humanity at
//! `10 × stats.emp` (not a separate stored value) because the book says
//! "Humanity cannot be *fully* regained without the removal of cyberware" —
//! i.e. the installed cyberware permanently lowers the max. However, the rules
//! engine does not track a per-character "current humanity max" field at this
//! stage (that accounting lives in WP-505/WP-506). We therefore cap at
//! `10 × stats.emp` as a safe upper bound for the un-modified case, and
//! document this simplification in the PR. A future WP can introduce a
//! `humanity_max` field and this function will just need to reference it.
//!
//! ## See also pp.229–230

use crate::character::Character;
use crate::error::RulesError;
use crate::types::Eurobucks;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Which kind of therapy is being administered. See pp.229–230.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TherapyKind {
    /// Informal, walk-in session. No direct rulebook equivalent — modelled
    /// from the 100eb/night hospital cost mentioned on p.229. Low cost, low
    /// Humanity gain.
    Outpatient,
    /// "Standard Humanity Loss" therapy (p.230). One week of intensive
    /// psychotherapy combining stress/anger management, counselling, hypnosis,
    /// and minor brain reprogramming. Cost 500eb, materials 100eb. Regains
    /// 2d6 Humanity on a successful DV15 Medicine check.
    Inpatient,
    /// "Extreme Humanity Loss" therapy (p.230). One week of intensive
    /// psychotherapy with direct and extreme brain reprogramming. Cost 1,000eb,
    /// materials 500eb. Regains 4d6 Humanity on a successful DV17 Medicine
    /// check.
    Intensive,
}

/// An amount of in-game time. See pp.229–230 (therapy sessions are time-gated).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeUnit {
    /// A number of hours.
    Hours(u16),
    /// A number of days.
    Days(u16),
    /// A number of weeks.
    Weeks(u16),
}

/// Parameters for a single therapy session.
///
/// The `humanity_gain` field is deliberately a fixed `u8` so that the caller
/// can pass in either an average (for deterministic tests) or an actual dice
/// roll (2d6 / 4d6) without changing the function signature.
///
/// See pp.229–230.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TherapySession {
    /// What kind of therapy this session represents.
    pub kind: TherapyKind,
    /// Total Eurobuck cost (materials included where applicable). See p.230.
    pub cost: Eurobucks,
    /// How many Humanity points the patient gains if the session succeeds.
    /// For standard sessions this is the average dice roll; callers may
    /// substitute the real roll. See p.230.
    pub humanity_gain: u8,
    /// Wall-clock time the session occupies. See p.229 ("takes 1 entire week").
    pub time_required: TimeUnit,
}

/// Describes what happened as a result of calling [`run_therapy`].
///
/// See pp.229–230.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TherapyOutcome {
    /// The character's Humanity before the session was applied.
    pub humanity_before: i16,
    /// The character's Humanity after the session was applied (capped at max).
    pub humanity_after: i16,
    /// How many Eurobucks were deducted from the character.
    pub cost_paid: Eurobucks,
    /// `true` if the character was cyberpsychotic before the session AND the
    /// session's Humanity gain pushed them back to ≥ 0, triggering
    /// [`Character::exit_cyberpsychosis`]. See pp.226–230.
    pub exited_cyberpsychosis: bool,
}

// ---------------------------------------------------------------------------
// Standard session constructors
// ---------------------------------------------------------------------------

/// Construct a standard outpatient session.
///
/// No direct rulebook equivalent — modelled from the 100eb/night hospital
/// stay cost on p.229. One day, +1 Humanity, 100eb.
///
/// **Deviation:** the p.230 table lists only "Standard" and "Extreme" Humanity
/// Loss therapies. This outpatient variant is an extrapolation flagged in the
/// PR. See pp.229–230.
pub fn outpatient_session() -> TherapySession {
    TherapySession {
        kind: TherapyKind::Outpatient,
        cost: Eurobucks(100),
        humanity_gain: 1,
        time_required: TimeUnit::Days(1),
    }
}

/// Construct a standard inpatient (Standard Humanity Loss) session.
///
/// 500eb, one week, +2d6 Humanity. The `humanity_gain` is set to **7**, the
/// average of 2d6. For a dice-rolled result, construct a [`TherapySession`]
/// directly with the rolled value. See p.230 (Standard Humanity Loss row).
pub fn inpatient_session() -> TherapySession {
    TherapySession {
        kind: TherapyKind::Inpatient,
        // p.230: 500eb cost + 100eb materials (Expensive tier). We charge the
        // combined cost here so `run_therapy` has one number to deduct.
        cost: Eurobucks(600),
        // Average 2d6 = 7.
        humanity_gain: 7,
        time_required: TimeUnit::Weeks(1),
    }
}

/// Construct a standard intensive (Extreme Humanity Loss) session.
///
/// 1,000eb, one week, +4d6 Humanity. The `humanity_gain` is set to **14**, the
/// average of 4d6. See p.230 (Extreme Humanity Loss row).
pub fn intensive_session() -> TherapySession {
    TherapySession {
        kind: TherapyKind::Intensive,
        // p.230: 1,000eb cost + 500eb materials. Combined here.
        cost: Eurobucks(1_500),
        // Average 4d6 = 14.
        humanity_gain: 14,
        time_required: TimeUnit::Weeks(1),
    }
}

// ---------------------------------------------------------------------------
// Core function
// ---------------------------------------------------------------------------

/// Apply a therapy session to a character.
///
/// # What this does
///
/// 1. Checks that `character.money >= session.cost`. If not, returns
///    `Err(RulesError::IpInsufficient)` — money is conceptually similar to
///    the IP pool for the purposes of this check; no new error variant is
///    introduced per WP conventions.
/// 2. Deducts `session.cost` from `character.money`.
/// 3. Adds `session.humanity_gain` to `character.humanity`, capped at
///    `10 × character.stats.emp` (starting Humanity per p.80).
/// 4. If the character was cyberpsychotic *before* the session AND their
///    Humanity is now ≥ 0, calls [`Character::exit_cyberpsychosis`] and
///    sets [`TherapyOutcome::exited_cyberpsychosis`] to `true`.
///
/// # Errors
///
/// Returns [`RulesError::IpInsufficient`] if `character.money < session.cost`.
/// The `required` field is `session.cost.0 as u32`; the `available` field is
/// `character.money.0 as u32` (saturating casts — negative balances become 0).
///
/// # Humanity cap
///
/// Capped at `10 × stats.emp` (starting Humanity per p.80). The rulebook
/// states "Humanity cannot be *fully* regained without the removal of
/// cyberware" (p.230); a future WP that tracks the cyberware-reduced max
/// separately can replace this cap. See module-level docs.
///
/// # See pp.229–230
pub fn run_therapy(
    character: &mut Character,
    session: TherapySession,
) -> Result<TherapyOutcome, RulesError> {
    // ------------------------------------------------------------------
    // 1. Funds check
    // ------------------------------------------------------------------
    if character.money < session.cost {
        return Err(RulesError::IpInsufficient {
            required: session.cost.0.max(0) as u32,
            available: character.money.0.max(0) as u32,
        });
    }

    let humanity_before = character.humanity;
    let was_cyberpsychotic = character.is_cyberpsychotic();

    // ------------------------------------------------------------------
    // 2. Deduct cost
    // ------------------------------------------------------------------
    character.money = Eurobucks(character.money.0 - session.cost.0);

    // ------------------------------------------------------------------
    // 3. Add Humanity, capped at starting Humanity (10 × EMP, p.80)
    // ------------------------------------------------------------------
    let humanity_max = i16::from(character.stats.emp) * 10;
    let raw_after = character
        .humanity
        .saturating_add(i16::from(session.humanity_gain));
    character.humanity = raw_after.min(humanity_max);

    // ------------------------------------------------------------------
    // 4. Exit Cyberpsychosis if Humanity recovered to ≥ 0 (pp.226–230)
    // ------------------------------------------------------------------
    let exited_cyberpsychosis = was_cyberpsychotic && character.humanity >= 0;
    if exited_cyberpsychosis {
        character.exit_cyberpsychosis();
    }

    Ok(TherapyOutcome {
        humanity_before,
        humanity_after: character.humanity,
        cost_paid: session.cost,
        exited_cyberpsychosis,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::{Inventory, Lifepath, Role, SkillSet, StatBlock, WornArmor, Wounds};
    use crate::effects::EffectStack;
    use crate::types::CharacterId;
    use uuid::Uuid;

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    fn fresh_character() -> Character {
        Character {
            id: CharacterId(Uuid::from_u128(0x507_0001)),
            name: "Test Runner".into(),
            handle: None,
            role: Role::Solo,
            role_rank: 4,
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
                // EMP 5 → starting Humanity 50
                emp: 5,
            },
            skills: SkillSet::default(),
            cyberware: vec![],
            armor: WornArmor::default(),
            inventory: Inventory::default(),
            wounds: Wounds::default(),
            // Start at 40 Humanity (already lost 10 to cyberware).
            humanity: 40,
            luck_pool: 6,
            money: Eurobucks(5_000),
            improvement_points: 0,
            lifepath: Lifepath::default(),
            effects: EffectStack::new(),
            complementary_bonuses: Vec::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Acceptance tests (WP-507)
    // -----------------------------------------------------------------------

    /// An outpatient session reduces money and increases Humanity.
    /// See pp.229–230.
    #[test]
    fn test_outpatient_session_reduces_money_increases_humanity() {
        let mut character = fresh_character();
        character.humanity = 40;
        character.money = Eurobucks(5_000);

        let session = outpatient_session(); // 100eb, +1 Humanity
        let outcome =
            run_therapy(&mut character, session).expect("must succeed with sufficient funds");

        assert_eq!(outcome.humanity_before, 40);
        assert_eq!(outcome.humanity_after, 41);
        assert_eq!(outcome.cost_paid, Eurobucks(100));
        assert!(!outcome.exited_cyberpsychosis);

        assert_eq!(character.money, Eurobucks(4_900));
        assert_eq!(character.humanity, 41);
    }

    /// If the character does not have enough money, returns an error and
    /// leaves the character unchanged. See pp.229–230.
    #[test]
    fn test_insufficient_money_returns_err() {
        let mut character = fresh_character();
        character.money = Eurobucks(50); // less than outpatient 100eb

        let session = outpatient_session();
        let result = run_therapy(&mut character, session);

        assert!(result.is_err(), "must fail with insufficient funds");
        match result.unwrap_err() {
            RulesError::IpInsufficient {
                required,
                available,
            } => {
                assert_eq!(required, 100);
                assert_eq!(available, 50);
            }
            other => panic!("expected IpInsufficient, got {:?}", other),
        }

        // Character must be unchanged.
        assert_eq!(character.money, Eurobucks(50));
        assert_eq!(character.humanity, 40);
    }

    /// When a cyberpsychotic character's Humanity climbs back to ≥ 0, the
    /// Cyberpsychosis state is cleared. See pp.226–230.
    #[test]
    fn test_therapy_exits_cyberpsychosis_when_humanity_recovers() {
        let mut character = fresh_character();
        // Drive character into cyberpsychosis.
        character.humanity = -3;
        character.enter_cyberpsychosis();
        assert!(
            character.is_cyberpsychotic(),
            "precondition: must be cyberpsychotic"
        );

        // A +5 humanity session should bring humanity to 2 (≥ 0), clearing
        // cyberpsychosis.
        let session = TherapySession {
            kind: TherapyKind::Inpatient,
            cost: Eurobucks(500),
            humanity_gain: 5,
            time_required: TimeUnit::Weeks(1),
        };
        let outcome = run_therapy(&mut character, session).expect("must succeed");

        assert!(
            outcome.exited_cyberpsychosis,
            "must have exited cyberpsychosis"
        );
        assert_eq!(outcome.humanity_before, -3);
        assert_eq!(outcome.humanity_after, 2);
        assert!(
            !character.is_cyberpsychotic(),
            "character must no longer be cyberpsychotic"
        );
    }

    /// Humanity cannot exceed the character's starting Humanity (10 × EMP).
    /// See pp.80, 229–230.
    #[test]
    fn test_therapy_caps_humanity_at_starting() {
        let mut character = fresh_character();
        // EMP 5 → max 50. Set humanity to 48.
        character.humanity = 48;

        // A +10 session would naively give 58, but must cap at 50.
        let session = TherapySession {
            kind: TherapyKind::Intensive,
            cost: Eurobucks(500),
            humanity_gain: 10,
            time_required: TimeUnit::Weeks(1),
        };
        let outcome = run_therapy(&mut character, session).expect("must succeed");

        assert_eq!(
            outcome.humanity_after, 50,
            "humanity must be capped at 10×EMP = 50"
        );
        assert_eq!(character.humanity, 50);
    }

    /// An intensive session gives more Humanity per session than outpatient.
    /// Validates that the standard constructors produce correctly ordered
    /// humanity_gain values. See pp.229–230.
    #[test]
    fn test_intensive_session_higher_gain() {
        let outpatient = outpatient_session();
        let inpatient = inpatient_session();
        let intensive = intensive_session();

        assert!(
            intensive.humanity_gain > inpatient.humanity_gain,
            "intensive must give more humanity than inpatient ({} > {})",
            intensive.humanity_gain,
            inpatient.humanity_gain,
        );
        assert!(
            inpatient.humanity_gain > outpatient.humanity_gain,
            "inpatient must give more humanity than outpatient ({} > {})",
            inpatient.humanity_gain,
            outpatient.humanity_gain,
        );
    }

    /// A cyberpsychotic character who gains some Humanity but stays below 0
    /// must NOT have cyberpsychosis cleared. See pp.226–230.
    #[test]
    fn test_therapy_does_not_exit_cyberpsychosis_if_humanity_still_negative() {
        let mut character = fresh_character();
        character.humanity = -10;
        character.enter_cyberpsychosis();

        // +3 → still at -7 (< 0)
        let session = TherapySession {
            kind: TherapyKind::Outpatient,
            cost: Eurobucks(100),
            humanity_gain: 3,
            time_required: TimeUnit::Days(1),
        };
        let outcome = run_therapy(&mut character, session).expect("must succeed");

        assert!(!outcome.exited_cyberpsychosis);
        assert_eq!(outcome.humanity_after, -7);
        assert!(
            character.is_cyberpsychotic(),
            "still cyberpsychotic at HUM < 0"
        );
    }

    /// Therapy that brings Humanity exactly to 0 does NOT exit cyberpsychosis —
    /// the threshold is strictly ≥ 0 which includes 0 itself. Verify. See p.230.
    #[test]
    fn test_therapy_exits_cyberpsychosis_at_exactly_zero() {
        let mut character = fresh_character();
        character.humanity = -5;
        character.enter_cyberpsychosis();

        let session = TherapySession {
            kind: TherapyKind::Inpatient,
            cost: Eurobucks(500),
            humanity_gain: 5,
            time_required: TimeUnit::Weeks(1),
        };
        let outcome = run_therapy(&mut character, session).expect("must succeed");

        // -5 + 5 = 0 → exactly 0 → exits cyberpsychosis (≥ 0 per p.230).
        assert_eq!(outcome.humanity_after, 0);
        assert!(
            outcome.exited_cyberpsychosis,
            "HUM=0 must exit cyberpsychosis"
        );
        assert!(!character.is_cyberpsychotic());
    }

    /// Therapy applied to a non-cyberpsychotic character must never set
    /// `exited_cyberpsychosis = true`. See pp.226–230.
    #[test]
    fn test_therapy_no_exit_when_not_cyberpsychotic() {
        let mut character = fresh_character();
        // Not cyberpsychotic.
        assert!(!character.is_cyberpsychotic());

        let session = inpatient_session();
        let outcome = run_therapy(&mut character, session).expect("must succeed");

        assert!(!outcome.exited_cyberpsychosis);
    }
}
