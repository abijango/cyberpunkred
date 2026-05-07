//! Programs catalog (WP-208) — non-Black-ICE NET Programs.
//!
//! Defines the [`Program`] entry type plus its supporting enums
//! ([`ProgramClass`], [`ProgramEffect`], [`BoostableCheck`], [`DiceSpec`],
//! [`DieKind`]) and the RON loader [`load_programs_catalog`].
//!
//! Rulebook references:
//! - **p.201:** the NET combat formula and "Defeating a Program" rules.
//!   A Program is *Derezzed* when its REZ drops to 0; Attacker programs
//!   auto-Deactivate after one use.
//! - **p.202:** "The Three Kinds of Non-Black ICE Programs" (Booster,
//!   Defender, Attacker), and the "How to Read the Program Tables" key
//!   (Class / ATK / DEF / REZ / Effect / Icon / Cost).
//! - **p.203:** Booster table (Eraser, See Ya, Speedy Gonzalvez, Worm),
//!   Defender table (Armor, Flak, Shield), and the start of the Attacker
//!   table (Banhammer).
//! - **p.204:** the rest of the Attacker table (Sword, DeckKRASH, Hellbolt,
//!   Nervescrub, Poison Flatline, Superglue, Vrizzbolt) plus the Black
//!   ICE intro callout. Black ICE itself is WP-209.
//!
//! Black ICE programs are deliberately *out of scope* for this WP — they
//! are catalogued separately by WP-209 because the schema differs (they
//! gain PER and SPD stats and use 2 deck slots). Per p.204:
//! "Installing or Uninstalling a Black ICE Program takes an hour."
//!
//! The catalog file is `content/catalogs/programs.ron`; the loader expects
//! one entry per slug. All entries here use `slot_cost: 1` (every
//! non-Black-ICE program in this catalog occupies a single deck slot per
//! the standard Cyberdeck rules; Black ICE's 2-slot footprint lives in
//! WP-209's catalog).

use crate::catalog::Catalog;
use crate::effects::ProgramId;
use crate::error::RulesError;
use crate::types::{Eurobucks, PriceTier};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// DiceSpec / DieKind
// ---------------------------------------------------------------------------

/// A simple `n d K` dice expression — number of dice plus die kind.
///
/// Used by [`ProgramEffect`] variants that roll variable damage (Banhammer
/// rolls 3d6 vs non-Black-ICE programs and 2d6 vs Black ICE per p.203;
/// Hellbolt rolls 2d6 brain damage per p.204).
///
/// Local to this WP. WP-202 (`DamageDice`) and WP-204 (`DiceSpec`) may
/// supply equivalent types; a future PR can dedupe once the canonical
/// shape is settled. Keeping a local definition keeps this branch
/// independent.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DiceSpec {
    /// Number of dice rolled.
    pub n: u8,
    /// Which die to roll.
    pub die: DieKind,
}

/// Which kind of die a [`DiceSpec`] rolls.
///
/// The non-Black-ICE program tables on pp.203–204 use only d6 for damage
/// (Banhammer, Sword, Hellbolt, Vrizzbolt). `D10` is provided so future
/// content (and downstream WPs that share this type) can express d10 rolls
/// without an enum extension.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DieKind {
    /// A six-sided die.
    D6,
    /// A ten-sided die.
    D10,
}

// ---------------------------------------------------------------------------
// BoostableCheck
// ---------------------------------------------------------------------------

/// The four NET Action Checks that Booster programs can buff.
///
/// Drawn from the Booster effects on p.203:
/// - `Cloak` (Eraser): the NET Action you take to hide your presence
///   from Black ICE; see pp.198–199.
/// - `Pathfinder` (See Ya): the NET Action used to map a NET
///   Architecture's floors; see p.199.
/// - `Backdoor` (Worm): the NET Action used to bypass Passwords; see
///   p.198.
/// - `Speed` (Speedy Gonzalvez): the Netrunner's MOVE-equivalent inside
///   the NET Architecture (so technically not a *Check* in the dice
///   sense, but the Booster bumps it the same way). The variant is
///   named for the value it modifies.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BoostableCheck {
    /// Hide your presence from Black ICE (Eraser, p.203).
    Cloak,
    /// Map the Architecture (See Ya, p.203).
    Pathfinder,
    /// Bypass Passwords (Worm, p.203).
    Backdoor,
    /// Netrunner's NET Speed (Speedy Gonzalvez, p.203). Strictly a
    /// derived value, not a Check, but the boost mechanic is identical.
    Speed,
}

// ---------------------------------------------------------------------------
// ProgramClass
// ---------------------------------------------------------------------------

/// The four classes a non-Black-ICE Program can belong to (p.202).
///
/// The book lists three top-level kinds — Booster, Defender, Attacker —
/// and then splits Attackers further by target on the per-program rows
/// (p.203 onward): Anti-Personnel Attackers go after Netrunner brains,
/// Anti-Program Attackers go after other Programs (or Black ICE). Per
/// p.201: "A Program whose Class specifies a type of target […] is only
/// effective when used against its intended target." We split that
/// distinction at the type level so call sites can't accidentally aim a
/// Banhammer at a Netrunner's brain.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProgramClass {
    /// "Improves your abilities in the NET Architecture while Rezzed."
    /// (p.202, table.) See p.203.
    Booster,
    /// "Stops or otherwise reduces the attacks of Programs or other
    /// Netrunners while Rezzed." (p.202, table.) See p.203.
    Defender,
    /// Anti-Personnel Attacker — targets enemy Netrunners directly
    /// (DeckKRASH, Hellbolt, Nervescrub, Poison Flatline, Superglue,
    /// Vrizzbolt). See p.204.
    AntiPersonnelAttacker,
    /// Anti-Program Attacker — targets Programs and Black ICE
    /// (Banhammer, Sword). See pp.203–204.
    AntiProgramAttacker,
}

// ---------------------------------------------------------------------------
// ProgramEffect
// ---------------------------------------------------------------------------

/// What a Program *does* once Activated (and, for Defenders/Boosters,
/// for as long as it remains Rezzed). See pp.203–204 for the per-program
/// effect text.
///
/// Variants are kept narrow — one variant per distinct game-mechanical
/// shape rather than one per program. A new variant is added rather than
/// shoehorning an existing one when a program's effect doesn't cleanly
/// fit (per the WP guidance).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProgramEffect {
    /// Add `by` to a specific kind of Check (or Speed) while Rezzed.
    /// Used by Eraser, See Ya, Worm, and Speedy Gonzalvez (p.203).
    BoostCheck {
        /// Which Check (or derived value) is modified.
        check: BoostableCheck,
        /// Magnitude of the modifier — `+2` in every published Booster.
        by: i8,
    },
    /// Lowers all brain damage taken from Black ICE by `reduction`.
    /// Used by Armor (p.203). Per p.203: "Lowers all brain damage you
    /// would receive by 4". One copy at a time, one use per Netrun.
    BlockBlackIceDamage {
        /// Damage reduction — `4` for Armor as printed.
        reduction: u8,
    },
    /// Reduces the ATK of every Non-Black-ICE Attacker Program run
    /// against the Netrunner to 0 while Rezzed. Used by Flak (p.203).
    /// One copy at a time, one use per Netrun.
    NullifyAttackerAtk,
    /// Stops the first successful Non-Black-ICE Program Effect from
    /// dealing brain damage; the program then auto-Derezzes. Used by
    /// Shield (p.203). One copy at a time, one use per Netrun.
    StopFirstNonBlackIceEffect,
    /// Anti-Program Attacker damage profile — different dice depending
    /// on whether the target is a non-Black-ICE Program or a Black ICE
    /// Program. Used by Banhammer (3d6 / 2d6) and Sword (2d6 / 3d6,
    /// flipped). Both per pp.203–204.
    AnyAttackerProgramDamage {
        /// Dice rolled when targeting a non-Black-ICE Program.
        dice_vs_non_black_ice: DiceSpec,
        /// Dice rolled when targeting a Black ICE Program.
        dice_vs_black_ice: DiceSpec,
    },
    /// DeckKRASH — forcibly and unsafely Jacks Out the target Netrunner.
    /// "Suffering the effect of all Rezzed enemy Black ICE they've
    /// encountered in the Architecture as they leave." (p.204.)
    ForceUnsafeJackOut,
    /// Hellbolt — `dice` brain damage now plus a sustained on-fire
    /// effect that does flat `burn_per_turn` damage at end of every turn
    /// until the target spends a Meat Action to extinguish; multiple
    /// copies do not stack (p.204).
    BrainDamageWithBurn {
        /// Initial brain-damage dice (2d6 for Hellbolt as printed).
        dice: DiceSpec,
        /// Recurring HP damage per turn until extinguished (2 for
        /// Hellbolt as printed).
        burn_per_turn: u8,
    },
    /// Nervescrub — lowers each of the target's INT, REF, and DEX by
    /// 1d6 for the next hour (minimum 1). Effects are psychosomatic and
    /// leave no permanent damage (p.204).
    NervescrubStatDrain {
        /// Dice rolled per stat (1d6 for Nervescrub as printed).
        dice: DiceSpec,
        /// Floor each affected stat is reduced *to* (1 per p.204).
        minimum: u8,
        /// Real-world duration in minutes (60 per p.204).
        duration_minutes: u32,
    },
    /// Poison Flatline — destroys a single Non-Black-ICE Program installed
    /// on the target Netrunner's Cyberdeck at random (p.204).
    DestroyRandomInstalledProgram,
    /// Superglue — target cannot Jack Out safely (or progress deeper into
    /// the Architecture) for `dice` rounds; an unsafe Jack Out is still
    /// possible. Each copy can be used once per Netrun (p.204).
    BlockJackOutAndProgress {
        /// Dice rolled for the duration in rounds (1d6 for Superglue).
        dice: DiceSpec,
    },
    /// Vrizzbolt — `dice` brain damage and lowers the target's NET
    /// Action budget on their next Turn by `action_penalty` (minimum 2
    /// remaining actions per p.204).
    BrainDamageAndNetActionPenalty {
        /// Brain-damage dice (1d6 for Vrizzbolt as printed).
        dice: DiceSpec,
        /// How many NET Actions are subtracted from the target's next
        /// Turn (1 for Vrizzbolt as printed).
        action_penalty: u8,
    },
}

// ---------------------------------------------------------------------------
// Program
// ---------------------------------------------------------------------------

/// A row in the canonical programs catalog (`content/catalogs/programs.ron`).
///
/// Mirrors the program table layout on p.202: every program has a Class,
/// ATK, DEF, REZ, an Effect, an Icon (descriptive flavour for narration),
/// and a Cost. `slot_cost` is the deck-slot footprint — `1` for every
/// program in this catalog (Black ICE programs occupy `2` slots and live
/// in WP-209's catalog).
///
/// `price` is the printed Price Category; `price_eb` is the explicit
/// Eurobuck cost as printed (Cyberpunk RED occasionally diverges from
/// the canonical tier value, so we store both rather than recomputing).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Program {
    /// String-slug identifier — the lookup key in [`Catalog`].
    pub id: ProgramId,
    /// Display name as printed in the rulebook (p.203 / p.204), preserving
    /// punctuation (e.g. "Speedy Gonzalvez", "Poison Flatline").
    pub display_name: String,
    /// Class banner (Booster / Defender / Attacker subclass). See p.202.
    pub class: ProgramClass,
    /// Bonus added to NET attack rolls when this Program attacks (p.202).
    /// Booster/Defender programs are 0; Attackers vary (Hellbolt 2,
    /// Banhammer 1, etc.).
    pub atk: u8,
    /// Bonus added to defense Checks made by the Program (p.202). Every
    /// non-Black-ICE program in this catalog is 0.
    pub def: u8,
    /// Program "hit points" — REZ is the damage the program can absorb
    /// before being Derezzed (p.202). Boosters/Defenders are 7 as
    /// printed; Attackers are 0 because they auto-Deactivate after firing
    /// (per p.201's "Defeating a Program" sidebar).
    pub rez: u8,
    /// What the Program does. See [`ProgramEffect`].
    pub effect: ProgramEffect,
    /// In-fiction icon description (e.g. "A pink glob exuding tiny soap
    /// bubbles."). Used by the LLM narrator and the UI; the engine
    /// itself ignores this string. See p.202 ("Icon" row in the table
    /// key).
    pub icon: String,
    /// Printed Price Category (Everyday, Costly, Premium…). See p.202.
    pub price: PriceTier,
    /// Explicit Eurobuck cost as printed. Paired with `price` rather than
    /// derived because the rulebook occasionally lists prices that don't
    /// match the canonical tier value.
    pub price_eb: Eurobucks,
    /// Number of deck slots this Program occupies. `1` for every program
    /// in this catalog (non-Black-ICE). Black ICE = `2` per p.204.
    pub slot_cost: u8,
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Schema for the on-disk RON file `content/catalogs/programs.ron`.
///
/// One `(programs: [ ... ])` envelope so the file reads as a flat list
/// rather than a map literal — same convention as the Skills catalog
/// (WP-201).
#[derive(Debug, Deserialize)]
struct ProgramsFile {
    programs: Vec<ProgramsFileEntry>,
}

/// One row in the on-disk programs catalog file.
///
/// `slug` is the lookup key inside the resulting `Catalog<Program>`. The
/// loader copies it into `Program::id` (which is itself a `ProgramId`
/// newtype around `String`).
#[derive(Debug, Deserialize)]
struct ProgramsFileEntry {
    slug: String,
    display_name: String,
    class: ProgramClass,
    atk: u8,
    def: u8,
    rez: u8,
    effect: ProgramEffect,
    icon: String,
    price: PriceTier,
    price_eb: Eurobucks,
    slot_cost: u8,
}

/// Load the programs catalog from a RON file at `path`.
///
/// On success returns a [`Catalog<Program>`] keyed by slug. On failure
/// returns [`RulesError::CatalogLoadFailed`] carrying the file path and
/// a stringified description of the underlying I/O or parse error.
///
/// Loader-enforced invariants:
/// 1. Slugs are unique within the file (a duplicate fails the load).
/// 2. Every entry has `slot_cost == 1` — Black ICE programs (2 slots)
///    belong in WP-209's catalog and must not appear here.
///
/// See `IMPLEMENTATION_PLAN.md` §2.5 for the broader Phase 2 catalog
/// loading conventions.
pub fn load_programs_catalog(path: &Path) -> Result<Catalog<Program>, RulesError> {
    let bytes = std::fs::read_to_string(path).map_err(|e| RulesError::CatalogLoadFailed {
        path: path.to_path_buf(),
        source: format!("read failed: {e}"),
    })?;
    let parsed: ProgramsFile =
        ron::de::from_str(&bytes).map_err(|e| RulesError::CatalogLoadFailed {
            path: path.to_path_buf(),
            source: format!("parse failed: {e}"),
        })?;

    let mut entries: HashMap<String, Program> = HashMap::with_capacity(parsed.programs.len());
    for row in parsed.programs {
        if row.slot_cost != 1 {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!(
                    "program '{}' has slot_cost {} — non-Black-ICE programs must be 1; Black ICE belongs in WP-209's catalog",
                    row.slug, row.slot_cost
                ),
            });
        }
        let prog = Program {
            id: ProgramId(row.slug.clone()),
            display_name: row.display_name,
            class: row.class,
            atk: row.atk,
            def: row.def,
            rez: row.rez,
            effect: row.effect,
            icon: row.icon,
            price: row.price,
            price_eb: row.price_eb,
            slot_cost: row.slot_cost,
        };
        if entries.insert(row.slug.clone(), prog).is_some() {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!("duplicate slug: '{}'", row.slug),
            });
        }
    }

    Ok(Catalog::new(entries))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Workspace-relative path to the canonical programs catalog file.
    ///
    /// `CARGO_MANIFEST_DIR` resolves to `crates/rules/`; the catalog lives
    /// two parents up at `content/catalogs/programs.ron`.
    fn catalog_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules -> crates
        p.pop(); // crates -> repo root
        p.push("content");
        p.push("catalogs");
        p.push("programs.ron");
        p
    }

    /// Acceptance: every Program from pp.202–204 (non-Black-ICE only)
    /// appears in the catalog, and the catalog contains exactly that
    /// many entries — no duplicates, no extras.
    ///
    /// Per pp.203–204 the non-Black-ICE programs are:
    /// - Boosters (4): Eraser, See Ya, Speedy Gonzalvez, Worm
    /// - Defenders (3): Armor, Flak, Shield
    /// - Anti-Program Attackers (2): Banhammer, Sword
    /// - Anti-Personnel Attackers (6): DeckKRASH, Hellbolt, Nervescrub,
    ///   Poison Flatline, Superglue, Vrizzbolt
    ///
    /// Total: 4 + 3 + 2 + 6 = **15**.
    #[test]
    fn test_programs_catalog_complete() {
        let cat = load_programs_catalog(&catalog_path()).expect("catalog must load");
        assert_eq!(
            cat.len(),
            15,
            "expected 15 non-Black-ICE programs per pp.202-204 (got {}); verify the catalog RON file",
            cat.len()
        );

        // Spot-check one program from each class.
        let must_be_present = [
            "eraser",
            "see_ya",
            "speedy_gonzalvez",
            "worm",
            "armor",
            "flak",
            "shield",
            "banhammer",
            "sword",
            "deckkrash",
            "hellbolt",
            "nervescrub",
            "poison_flatline",
            "superglue",
            "vrizzbolt",
        ];
        for slug in must_be_present {
            assert!(
                cat.get(slug).is_some(),
                "missing program slug '{slug}' (pp.203-204)"
            );
        }
    }

    /// Acceptance: Eraser boosts Cloak Checks by +2 (p.203).
    #[test]
    fn test_eraser_boosts_cloak_by_2() {
        let cat = load_programs_catalog(&catalog_path()).expect("catalog must load");
        let eraser = cat.get("eraser").expect("eraser must be present");
        assert_eq!(eraser.class, ProgramClass::Booster);
        assert_eq!(
            eraser.effect,
            ProgramEffect::BoostCheck {
                check: BoostableCheck::Cloak,
                by: 2,
            },
            "Eraser must boost Cloak by +2 per p.203"
        );
    }

    /// Acceptance: Banhammer rolls 3d6 vs non-Black-ICE Programs and
    /// 2d6 vs Black ICE Programs (p.203).
    #[test]
    fn test_banhammer_dice() {
        let cat = load_programs_catalog(&catalog_path()).expect("catalog must load");
        let banhammer = cat.get("banhammer").expect("banhammer must be present");
        assert_eq!(banhammer.class, ProgramClass::AntiProgramAttacker);
        assert_eq!(banhammer.atk, 1, "Banhammer ATK is 1 per p.203");
        match &banhammer.effect {
            ProgramEffect::AnyAttackerProgramDamage {
                dice_vs_non_black_ice,
                dice_vs_black_ice,
            } => {
                assert_eq!(
                    *dice_vs_non_black_ice,
                    DiceSpec {
                        n: 3,
                        die: DieKind::D6
                    },
                    "Banhammer rolls 3d6 vs non-Black-ICE Programs (p.203)"
                );
                assert_eq!(
                    *dice_vs_black_ice,
                    DiceSpec {
                        n: 2,
                        die: DieKind::D6
                    },
                    "Banhammer rolls 2d6 vs Black ICE Programs (p.203)"
                );
            }
            other => panic!("Banhammer must use AnyAttackerProgramDamage, got {other:?}"),
        }
    }

    /// Acceptance: every entry in the catalog round-trips through RON
    /// serialisation without loss. The loader already parses RON; this
    /// test additionally serialises the in-memory `Program` back to RON
    /// and re-parses it, pinning the contract that the file schema and
    /// in-memory schema agree.
    #[test]
    fn test_programs_round_trip_ron() {
        let cat = load_programs_catalog(&catalog_path()).expect("catalog must load");
        for (slug, prog) in cat.iter() {
            let serialised = ron::ser::to_string(prog)
                .unwrap_or_else(|e| panic!("serialise '{slug}' failed: {e}"));
            let restored: Program = ron::de::from_str(&serialised)
                .unwrap_or_else(|e| panic!("re-parse '{slug}' failed: {e}"));
            assert_eq!(
                &restored, prog,
                "round-trip mismatch for '{slug}': RON serialisation is lossy"
            );
        }
    }

    /// Regression: every program in the catalog has `slot_cost == 1` —
    /// Black ICE (2 slots, p.204) belongs in WP-209's catalog and must
    /// never appear here. Loader-enforced; pinned as a test in case a
    /// future refactor weakens the loader.
    #[test]
    fn test_all_programs_use_one_slot() {
        let cat = load_programs_catalog(&catalog_path()).expect("catalog must load");
        for (slug, prog) in cat.iter() {
            assert_eq!(
                prog.slot_cost, 1,
                "program '{slug}' has slot_cost {} — should be 1 (non-Black-ICE)",
                prog.slot_cost
            );
        }
    }

    /// Regression: every Booster has REZ 7, every Defender has REZ 7,
    /// and every Attacker has REZ 0 (per pp.203–204 the entire table
    /// uses these values).
    #[test]
    fn test_program_rez_by_class() {
        let cat = load_programs_catalog(&catalog_path()).expect("catalog must load");
        for (slug, prog) in cat.iter() {
            let expected = match prog.class {
                ProgramClass::Booster | ProgramClass::Defender => 7,
                ProgramClass::AntiPersonnelAttacker | ProgramClass::AntiProgramAttacker => 0,
            };
            assert_eq!(
                prog.rez, expected,
                "program '{slug}' ({:?}) REZ {} — expected {expected} per pp.203-204",
                prog.class, prog.rez,
            );
        }
    }

    /// Regression: each program's printed Eurobuck cost matches its
    /// printed Price Category (per pp.203–204 the two always agree for
    /// non-Black-ICE programs — Everyday=20, Costly=50, Premium=100).
    #[test]
    fn test_program_price_eb_matches_tier() {
        let cat = load_programs_catalog(&catalog_path()).expect("catalog must load");
        for (slug, prog) in cat.iter() {
            assert_eq!(
                prog.price_eb,
                prog.price.canonical_cost(),
                "program '{slug}' price_eb {:?} disagrees with PriceTier::{:?} canonical cost {:?}",
                prog.price_eb,
                prog.price,
                prog.price.canonical_cost()
            );
        }
    }

    /// Regression: the Defender programs map to their distinct effect
    /// shapes (not all collapsed into a generic placeholder).
    /// See p.203.
    #[test]
    fn test_defenders_have_distinct_effects() {
        let cat = load_programs_catalog(&catalog_path()).expect("catalog must load");

        let armor = cat.get("armor").expect("armor must be present");
        assert_eq!(
            armor.effect,
            ProgramEffect::BlockBlackIceDamage { reduction: 4 },
            "Armor reduces brain damage by 4 (p.203)"
        );

        let flak = cat.get("flak").expect("flak must be present");
        assert_eq!(
            flak.effect,
            ProgramEffect::NullifyAttackerAtk,
            "Flak reduces non-Black-ICE Attacker ATK to 0 (p.203)"
        );

        let shield = cat.get("shield").expect("shield must be present");
        assert_eq!(
            shield.effect,
            ProgramEffect::StopFirstNonBlackIceEffect,
            "Shield stops first non-Black-ICE program effect (p.203)"
        );
    }

    /// Regression: the Booster programs all use BoostCheck +2 with the
    /// matching BoostableCheck variant. See p.203.
    #[test]
    fn test_boosters_match_their_check() {
        let cat = load_programs_catalog(&catalog_path()).expect("catalog must load");

        let cases = [
            ("eraser", BoostableCheck::Cloak),
            ("see_ya", BoostableCheck::Pathfinder),
            ("worm", BoostableCheck::Backdoor),
            ("speedy_gonzalvez", BoostableCheck::Speed),
        ];
        for (slug, check) in cases {
            let prog = cat.get(slug).unwrap_or_else(|| panic!("missing {slug}"));
            assert_eq!(prog.class, ProgramClass::Booster, "{slug} must be Booster");
            assert_eq!(
                prog.effect,
                ProgramEffect::BoostCheck { check, by: 2 },
                "{slug} must boost {check:?} by +2 (p.203)"
            );
        }
    }

    /// Regression: Sword is the mirror of Banhammer — 2d6 vs non-Black-ICE,
    /// 3d6 vs Black ICE (p.204).
    #[test]
    fn test_sword_dice() {
        let cat = load_programs_catalog(&catalog_path()).expect("catalog must load");
        let sword = cat.get("sword").expect("sword must be present");
        assert_eq!(sword.class, ProgramClass::AntiProgramAttacker);
        assert_eq!(sword.atk, 1, "Sword ATK is 1 per p.204");
        match &sword.effect {
            ProgramEffect::AnyAttackerProgramDamage {
                dice_vs_non_black_ice,
                dice_vs_black_ice,
            } => {
                assert_eq!(
                    *dice_vs_non_black_ice,
                    DiceSpec {
                        n: 2,
                        die: DieKind::D6
                    },
                    "Sword rolls 2d6 vs non-Black-ICE Programs (p.204)"
                );
                assert_eq!(
                    *dice_vs_black_ice,
                    DiceSpec {
                        n: 3,
                        die: DieKind::D6
                    },
                    "Sword rolls 3d6 vs Black ICE Programs (p.204)"
                );
            }
            other => panic!("Sword must use AnyAttackerProgramDamage, got {other:?}"),
        }
    }

    /// Regression: Hellbolt does 2d6 brain damage and 2 burn-per-turn
    /// (p.204).
    #[test]
    fn test_hellbolt_brain_damage_and_burn() {
        let cat = load_programs_catalog(&catalog_path()).expect("catalog must load");
        let hellbolt = cat.get("hellbolt").expect("hellbolt must be present");
        assert_eq!(hellbolt.class, ProgramClass::AntiPersonnelAttacker);
        assert_eq!(hellbolt.atk, 2, "Hellbolt ATK is 2 per p.204");
        assert_eq!(
            hellbolt.effect,
            ProgramEffect::BrainDamageWithBurn {
                dice: DiceSpec {
                    n: 2,
                    die: DieKind::D6
                },
                burn_per_turn: 2,
            },
            "Hellbolt does 2d6 brain damage + 2 HP/turn burn (p.204)"
        );
    }

    /// Regression: Vrizzbolt fits the BrainDamageAndNetActionPenalty
    /// variant — 1d6 brain damage and `-1` NET Action (minimum 2 left)
    /// per p.204.
    #[test]
    fn test_vrizzbolt_action_penalty() {
        let cat = load_programs_catalog(&catalog_path()).expect("catalog must load");
        let v = cat.get("vrizzbolt").expect("vrizzbolt must be present");
        assert_eq!(v.class, ProgramClass::AntiPersonnelAttacker);
        assert_eq!(v.atk, 1, "Vrizzbolt ATK is 1 per p.204");
        assert_eq!(
            v.effect,
            ProgramEffect::BrainDamageAndNetActionPenalty {
                dice: DiceSpec {
                    n: 1,
                    die: DieKind::D6
                },
                action_penalty: 1,
            },
            "Vrizzbolt: 1d6 brain damage and -1 NET Action (p.204)"
        );
    }

    /// Regression: the loader rejects a duplicate slug.
    #[test]
    fn test_loader_rejects_duplicate_slug() {
        // Build a tiny RON file in a tempdir with two entries sharing
        // the same slug.
        let tmp = std::env::temp_dir().join("cpr_wp208_dup.ron");
        std::fs::write(
            &tmp,
            r#"ProgramsFile(
                programs: [
                    (
                        slug: "dup",
                        display_name: "Dup",
                        class: Booster,
                        atk: 0, def: 0, rez: 7,
                        effect: BoostCheck(check: Cloak, by: 2),
                        icon: "x",
                        price: Everyday,
                        price_eb: (20),
                        slot_cost: 1,
                    ),
                    (
                        slug: "dup",
                        display_name: "Dup2",
                        class: Booster,
                        atk: 0, def: 0, rez: 7,
                        effect: BoostCheck(check: Cloak, by: 2),
                        icon: "x",
                        price: Everyday,
                        price_eb: (20),
                        slot_cost: 1,
                    ),
                ]
            )"#,
        )
        .expect("temp file write must succeed");
        let result = load_programs_catalog(&tmp);
        assert!(matches!(
            result,
            Err(RulesError::CatalogLoadFailed { source, .. }) if source.contains("duplicate")
        ));
        let _ = std::fs::remove_file(&tmp);
    }

    /// Regression: the loader rejects an entry with `slot_cost != 1`
    /// (Black ICE belongs in WP-209's catalog).
    #[test]
    fn test_loader_rejects_two_slot_program() {
        let tmp = std::env::temp_dir().join("cpr_wp208_2slot.ron");
        std::fs::write(
            &tmp,
            r#"ProgramsFile(
                programs: [
                    (
                        slug: "intruder",
                        display_name: "Intruder",
                        class: AntiPersonnelAttacker,
                        atk: 2, def: 2, rez: 15,
                        effect: ForceUnsafeJackOut,
                        icon: "y",
                        price: Premium,
                        price_eb: (100),
                        slot_cost: 2,
                    ),
                ]
            )"#,
        )
        .expect("temp file write must succeed");
        let result = load_programs_catalog(&tmp);
        assert!(matches!(
            result,
            Err(RulesError::CatalogLoadFailed { source, .. }) if source.contains("slot_cost")
        ));
        let _ = std::fs::remove_file(&tmp);
    }

    /// Regression: `DiceSpec` and `DieKind` are `Copy + Hash` and
    /// round-trip through RON. Pins the contract that downstream code can
    /// store these in `HashMap` keys / arrays without `Clone`.
    #[test]
    fn test_dice_spec_serialises() {
        let d = DiceSpec {
            n: 3,
            die: DieKind::D6,
        };
        let s = ron::ser::to_string(&d).expect("serialise");
        let back: DiceSpec = ron::de::from_str(&s).expect("round-trip");
        assert_eq!(back, d);

        // Also d10.
        let d2 = DiceSpec {
            n: 1,
            die: DieKind::D10,
        };
        let s2 = ron::ser::to_string(&d2).expect("serialise");
        let back2: DiceSpec = ron::de::from_str(&s2).expect("round-trip");
        assert_eq!(back2, d2);
    }
}
