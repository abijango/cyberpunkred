//! Black ICE catalog (WP-209).
//!
//! Defines the catalog row [`BlackIce`] for every Black ICE Program in the
//! *Cyberpunk RED* rulebook (pp.205–207), the supporting effect / class /
//! identifier types, and the RON loader [`load_black_ice_catalog`].
//!
//! Rulebook references:
//! - **p.204:** introductory paragraph — Black ICE programs use **2 deck
//!   slots** and "**Installing or Uninstalling a Black ICE Program takes
//!   an hour.**"
//! - **p.205:** "The Kinds of Black ICE Programs" (Anti-Personnel,
//!   Anti-Program, Demon) and the "Encountering and Using Black ICE"
//!   procedure.
//! - **p.206:** "How to Read the Black ICE Program Table" — defines the
//!   `PER / SPD / ATK / DEF / REZ` columns — and the first three rows
//!   (Asp, Giant, Hellhound).
//! - **p.207:** the remaining nine rows (Kraken, Liche, Raven, Scorpion,
//!   Skunk, Wisp, Dragon, Killer, Sabertooth).
//!
//! Demons are catalogued separately by WP-210 (`catalog::demons`) and have
//! a different stat block (REZ / Interface / Combat Number); they are
//! *not* part of this catalog even though p.205 lists Demon as a Black
//! ICE class.
//!
//! The catalog file is `content/catalogs/black_ice.ron`; the loader
//! expects one entry per slug.

use crate::catalog::Catalog;
use crate::error::RulesError;
use crate::types::{Eurobucks, PriceTier};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Dice notation (local — to be promoted to a shared type by a later WP)
// ---------------------------------------------------------------------------

/// Which die a [`DiceSpec`] rolls.
///
/// The Black ICE table on pp.206–207 only ever uses d6 (every damage and
/// stat-loss roll). `D10` is included up front so this local notation can
/// be reused by later catalogs (e.g. WP-208 Programs) without a breaking
/// change. Rulebook usage is exclusively `D6` for Black ICE; see the
/// individual entries.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DieKind {
    /// Six-sided die.
    D6,
    /// Ten-sided die — included for future reuse, not used by any Black
    /// ICE entry.
    D10,
}

/// `n` × [`DieKind`] dice expression, e.g. *3d6* is `DiceSpec { n: 3, die: D6 }`.
///
/// Used by [`BlackIceEffect`] variants that roll damage or stat penalties
/// (every Black ICE attack on pp.206–207 rolls `XdY` of some kind). Local
/// to this WP per the WP-209 contract; a workspace-wide `DamageDice` /
/// `DiceSpec` will be introduced by a later coordination WP.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DiceSpec {
    /// Number of dice rolled.
    pub n: u8,
    /// Which die is rolled (see [`DieKind`]).
    pub die: DieKind,
}

// ---------------------------------------------------------------------------
// BlackIceId
// ---------------------------------------------------------------------------

/// Catalog identifier for a Black ICE program.
///
/// The WP-209 contract (`IMPLEMENTATION_PLAN.md` §4) declares the public
/// shape `pub struct BlackIceId(pub String)`, paralleling the
/// `pub struct ProgramId(pub String)` in WP-208. The wrapped `String`
/// matches the slug under which the row is filed in the
/// `Catalog<BlackIce>` (e.g. `"asp"`, `"hellhound"`); the loader enforces
/// `id.0 == slug` for every entry.
///
/// Open string identifier (rather than a closed enum) because Black ICE
/// expansion is data-driven — supplements add new programs without code
/// changes. Compare [`crate::catalog::skills::SkillId`], which is closed
/// because the core skill list is fixed by the rulebook.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct BlackIceId(pub String);

// ---------------------------------------------------------------------------
// BlackIceClass
// ---------------------------------------------------------------------------

/// The three Black ICE classes listed on p.205 ("The Kinds of Black ICE
/// Programs").
///
/// Per p.205:
/// - **Anti-Personnel:** "Deadly single-minded Programs that hunt down
///   and kill Netrunners."
/// - **Anti-Program:** "Deadly single-minded Programs that hunt down and
///   kill a Netrunner's Rezzed Programs."
/// - **Demon:** "Black ICE Intelligent Systems that operate Control
///   Nodes... These are too big for Cyberdecks." Demons appear in this
///   enum so callers can pattern-match exhaustively, but the catalog
///   loaded by [`load_black_ice_catalog`] contains **no Demon entries** —
///   Demons are catalogued separately by WP-210 because they have a
///   different stat block (no PER/SPD/ATK/DEF, only REZ + Interface +
///   Combat Number; see p.212).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlackIceClass {
    /// Hunts and kills Netrunners.
    AntiPersonnel,
    /// Hunts and kills Rezzed Programs.
    AntiProgram,
    /// Defends a NET Architecture or physical space; too big for
    /// Cyberdecks. Modelled by WP-210, not here.
    Demon,
}

// ---------------------------------------------------------------------------
// JackOutKind
// ---------------------------------------------------------------------------

/// How a Black ICE forces a Netrunner out of (or stops them leaving) the
/// Architecture, per pp.206–207.
///
/// Only two Black ICE on pp.206–207 manipulate the jack-out flow:
/// - **Giant** (p.206): "**The Netrunner is forcibly and unsafely Jacked
///   Out** of their current Netrun." → [`JackOutKind::ForcedUnsafe`].
/// - **Kraken** (p.207): "**Until the end of the Netrunner's next Turn,
///   the Netrunner cannot progress deeper into the Architecture or Jack
///   Out safely** (The Netrunner can still perform an unsafe Jack Out)." →
///   [`JackOutKind::SafeJackOutBlocked`].
///
/// We model these as two distinct cases rather than a "consequence
/// severity" scalar so the behaviour engine (WP-414) can implement each
/// path explicitly. RAW: see pp.206–207.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JackOutKind {
    /// Target is forcibly and *unsafely* jacked out of the run (Giant —
    /// p.206).
    ForcedUnsafe,
    /// Target is locked into the current Architecture floor — they cannot
    /// progress deeper or Jack Out safely (an *unsafe* jack-out is still
    /// allowed). Lasts until the end of the Netrunner's next Turn
    /// (Kraken — p.207).
    SafeJackOutBlocked,
}

// ---------------------------------------------------------------------------
// BlackIceEffect
// ---------------------------------------------------------------------------

/// The effect a Black ICE inflicts when its attack hits.
///
/// One variant per distinct effect on pp.206–207 — Black ICE programs do
/// not share effects across rows, so each row maps to exactly one
/// variant. See the per-variant doc comments for the rulebook citation.
///
/// All damage / stat-loss rolls use `XdY` notation captured as a
/// [`DiceSpec`]; on the actual Black ICE table on pp.206–207 every die
/// is a d6.
///
/// A side note on stacking: the data table in the rulebook hard-codes
/// "Multiple instances of this effect cannot stack" for *Hellhound*
/// (p.206) and "the effects of multiple Skunks can stack" for *Skunk*
/// (p.207). The flag is recorded on each variant where the rulebook
/// calls it out.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlackIceEffect {
    /// **Asp** (p.206): "Destroys a single Program installed on the
    /// enemy Netrunner's Cyberdeck at random."
    DestroyRandomProgram,

    /// **Giant** (p.206): "Does 3d6 damage direct to an enemy
    /// Netrunner's brain. The Netrunner is forcibly and unsafely Jacked
    /// Out of their current Netrun. They suffer the effect of all Rezzed
    /// enemy Black ICE they've encountered in the Architecture as they
    /// leave, not including the Giant."
    ///
    /// `applies_remaining_black_ice_on_exit` distinguishes Giant
    /// (`true`) from any future Black ICE that combines brain damage
    /// with a forced jack-out without that secondary penalty.
    BrainDamageAndForceJackOut {
        /// Brain damage dealt (3d6 for Giant — p.206).
        dice: DiceSpec,
        /// Which jack-out flavour is inflicted.
        jackout_consequence: JackOutKind,
        /// `true` iff the Netrunner suffers the effect of every Rezzed
        /// enemy Black ICE in the Architecture on the way out (Giant —
        /// p.206).
        applies_remaining_black_ice_on_exit: bool,
    },

    /// **Hellhound** (p.206): "Does 2d6 damage direct to the
    /// Netrunner's brain. Unless insulated, their Cyberdeck catches
    /// fire along with their clothing. Until they spend a Meat Action
    /// to put themselves out, they take 2 damage to their HP whenever
    /// they end their Turn. Multiple instances of this effect cannot
    /// stack."
    BrainDamageAndCyberdeckFire {
        /// Initial brain damage (2d6 — p.206).
        dice: DiceSpec,
        /// HP damage per turn-end while burning (2 — p.206).
        burn_damage_per_turn_end: u8,
        /// `true` iff multiple instances of this effect cannot stack
        /// (Hellhound — p.206 carries this rider).
        cannot_stack: bool,
    },

    /// **Kraken** (p.207): "Does 3d6 damage direct to an enemy
    /// Netrunner's brain. Until the end of the Netrunner's next Turn,
    /// the Netrunner cannot progress deeper into the Architecture or
    /// Jack Out safely (The Netrunner can still perform an unsafe Jack
    /// Out)."
    BrainDamageAndJackOutLock {
        /// Brain damage dealt (3d6 — p.207).
        dice: DiceSpec,
        /// Which jack-out flavour is inflicted (always
        /// [`JackOutKind::SafeJackOutBlocked`] for Kraken).
        jackout_consequence: JackOutKind,
    },

    /// **Liche** (p.207): "Enemy Netrunner's INT, REF, and DEX are
    /// each lowered by 1d6 for the next hour (minimum 1). The effects
    /// are largely psychosomatic and leave no permanent effects."
    StatPenaltyIntRefDex {
        /// Penalty roll applied separately to each of INT, REF, DEX
        /// (1d6 — p.207).
        dice: DiceSpec,
        /// Floor each affected stat is clamped to (1 — p.207).
        minimum_stat: u8,
    },

    /// **Raven** (p.207): "Derezzes a single Defender Program the
    /// enemy Netrunner has Rezzed at random, then deals 1d6 damage
    /// direct to the Netrunner's brain."
    DerezzRandomDefenderAndBrainDamage {
        /// Brain damage dealt after the Derezz (1d6 — p.207).
        dice: DiceSpec,
    },

    /// **Scorpion** (p.207): "Enemy Netrunner's MOVE is lowered by
    /// 1d6 for the next hour (minimum 1). The effects are largely
    /// psychosomatic and leave no permanent effects."
    StatPenaltyMove {
        /// Penalty roll applied to MOVE (1d6 — p.207).
        dice: DiceSpec,
        /// Floor MOVE is clamped to (1 — p.207).
        minimum_stat: u8,
    },

    /// **Skunk** (p.207): "Until this Program is Derezzed, an enemy
    /// Netrunner hit by this Effect makes all Slide Checks at a -2.
    /// Each Skunk Black ICE can only affect a single Netrunner at a
    /// time, but the effects of multiple Skunks can stack."
    SlideCheckPenalty {
        /// Penalty applied to all Slide Checks (-2 — p.207). Stored as
        /// a positive `u8`; the engine subtracts.
        penalty: u8,
        /// `true` iff multiple instances of this effect stack (Skunk —
        /// p.207).
        stacks: bool,
    },

    /// **Wisp** (p.207): "Does 1d6 damage direct to the enemy
    /// Netrunner's brain and lowers the amount of total NET Actions
    /// the Netrunner can accomplish on their next Turn by 1
    /// (minimum 2)."
    BrainDamageAndNetActionPenalty {
        /// Brain damage dealt (1d6 — p.207).
        dice: DiceSpec,
        /// NET Action reduction on the Netrunner's next Turn (1 —
        /// p.207).
        action_penalty: u8,
        /// Floor on the Netrunner's NET Actions (2 — p.207).
        minimum_actions: u8,
    },

    /// **Dragon / Killer / Sabertooth** (p.207): "Deals XdN damage to
    /// a Program. If this damage would be enough to Derezz the
    /// Program, it is instead Destroyed."
    ///
    /// Anti-Program Black ICE all share this effect shape, with
    /// different dice — Dragon and Sabertooth roll 6d6, Killer rolls
    /// 4d6 (p.207).
    ProgramDamageDestroyOnDerezz {
        /// Damage dealt to the targeted Program (6d6 / 4d6 — p.207).
        dice: DiceSpec,
    },
}

// ---------------------------------------------------------------------------
// BlackIce
// ---------------------------------------------------------------------------

/// A row in the canonical Black ICE catalog
/// (`content/catalogs/black_ice.ron`).
///
/// One entry per Black ICE program on pp.206–207. The stat block columns
/// (`per` / `spd` / `atk` / `def` / `rez`) are read directly off the
/// Black ICE Program Table; see p.206 for the column definitions.
///
/// All numeric fields are `u8` — every printed value on pp.206–207 fits
/// in a byte (the largest is REZ 30 for Kraken / Dragon).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlackIce {
    /// Catalog identifier — wraps the slug used as the `Catalog<T>` key.
    pub id: BlackIceId,
    /// Display name as printed in the rulebook (p.206 onward), preserving
    /// the book's capitalisation (e.g. "Hellhound").
    pub display_name: String,
    /// Black ICE class (Anti-Personnel / Anti-Program / Demon — p.205).
    /// All catalog rows are `AntiPersonnel` or `AntiProgram`; Demons are
    /// in WP-210.
    pub class: BlackIceClass,
    /// Perception — how hard the Black ICE is to Slide away from
    /// (p.206).
    pub per: u8,
    /// Speed — how fast the Black ICE can react (p.206).
    pub spd: u8,
    /// Attack — added to the Black ICE's attack roll (p.206).
    pub atk: u8,
    /// Defense — added to the Black ICE's defence roll (p.206).
    pub def: u8,
    /// REZ — the program's hit points (p.206).
    pub rez: u8,
    /// Effect inflicted on a successful hit (p.206 column "Effect").
    pub effect: BlackIceEffect,
    /// Cosmetic icon description ("Icon: …" footnote in the rulebook
    /// table; pp.206–207). The narrator uses this for flavour; the
    /// engine itself ignores the text.
    pub icon: String,
    /// Price tier (the `(Premium)` / `(Costly)` / etc. footnote in the
    /// `Cost` column; pp.206–207).
    pub price: PriceTier,
    /// Cash cost (the eurobuck number on the `Cost` column; pp.206–207).
    /// Stored alongside `price` so the catalog round-trips the printed
    /// number rather than re-deriving it from [`PriceTier::canonical_cost`]
    /// (the loader enforces agreement).
    pub price_eb: Eurobucks,
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Schema for the on-disk RON file `content/catalogs/black_ice.ron`.
///
/// Mirrors the [`crate::catalog::skills::load_skills_catalog`] envelope —
/// a `(black_ice: [ ... ])` flat list of rows, each carrying its own
/// `slug`. Decoupling the file schema from the in-memory `Catalog<T>`
/// keeps authored content readable.
#[derive(Debug, Deserialize)]
struct BlackIceFile {
    black_ice: Vec<BlackIceFileEntry>,
}

/// One row in the on-disk Black ICE catalog file.
///
/// `slug` is the lookup key inside the resulting `Catalog<BlackIce>`.
/// All other fields populate the [`BlackIce`] directly.
#[derive(Debug, Deserialize)]
struct BlackIceFileEntry {
    slug: String,
    id: BlackIceId,
    display_name: String,
    class: BlackIceClass,
    per: u8,
    spd: u8,
    atk: u8,
    def: u8,
    rez: u8,
    effect: BlackIceEffect,
    icon: String,
    price: PriceTier,
    price_eb: Eurobucks,
}

/// Load the Black ICE catalog from a RON file at `path`.
///
/// On success returns a [`Catalog<BlackIce>`] keyed by slug. On failure
/// returns [`RulesError::CatalogLoadFailed`] carrying the file path and
/// a stringified description of the underlying I/O / parse / invariant
/// error.
///
/// The loader enforces three invariants:
/// 1. **Slug uniqueness:** a duplicate slug fails the load.
/// 2. **Id agrees with slug:** every row's `id.0` must equal its `slug`
///    (the wrapped string and the catalog key are the same handle).
/// 3. **Price agrees with tier:** every row's `price_eb` must equal
///    `price.canonical_cost()`. Catches typos in the cost column at
///    parse time. Verified against pp.206–207.
///
/// No `class == Demon` rows are accepted — Demons are catalogued
/// separately by WP-210. (RAW p.205 lists Demon as a Black ICE class,
/// but its stat block does not match this catalog's columns.)
pub fn load_black_ice_catalog(path: &Path) -> Result<Catalog<BlackIce>, RulesError> {
    let bytes = std::fs::read_to_string(path).map_err(|e| RulesError::CatalogLoadFailed {
        path: path.to_path_buf(),
        source: format!("read failed: {e}"),
    })?;
    let parsed: BlackIceFile =
        ron::de::from_str(&bytes).map_err(|e| RulesError::CatalogLoadFailed {
            path: path.to_path_buf(),
            source: format!("parse failed: {e}"),
        })?;

    let mut entries: HashMap<String, BlackIce> = HashMap::with_capacity(parsed.black_ice.len());
    for row in parsed.black_ice {
        if row.id.0 != row.slug {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!("row '{}': id ({}) must equal slug", row.slug, row.id.0),
            });
        }
        if matches!(row.class, BlackIceClass::Demon) {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!(
                    "row '{}': Demon-class entries belong to WP-210, not the Black ICE catalog (see p.205)",
                    row.slug
                ),
            });
        }
        let canonical_eb = row.price.canonical_cost();
        if row.price_eb != canonical_eb {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!(
                    "row '{}': price_eb {:?} disagrees with PriceTier::{:?}.canonical_cost() = {:?}",
                    row.slug, row.price_eb, row.price, canonical_eb
                ),
            });
        }
        let entry = BlackIce {
            id: row.id,
            display_name: row.display_name,
            class: row.class,
            per: row.per,
            spd: row.spd,
            atk: row.atk,
            def: row.def,
            rez: row.rez,
            effect: row.effect,
            icon: row.icon,
            price: row.price,
            price_eb: row.price_eb,
        };
        if entries.insert(row.slug.clone(), entry).is_some() {
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

    /// Workspace-relative path to the canonical Black ICE catalog file.
    fn catalog_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules -> crates
        p.pop(); // crates -> repo root
        p.push("content");
        p.push("catalogs");
        p.push("black_ice.ron");
        p
    }

    /// Acceptance: every Black ICE listed on pp.206–207 must appear in
    /// the catalog, and the catalog must contain exactly that many
    /// entries.
    ///
    /// Count derivation (verified against pp.206–207):
    /// - Anti-Personnel (9): Asp, Giant, Hellhound, Kraken, Liche,
    ///   Raven, Scorpion, Skunk, Wisp.
    /// - Anti-Program (3): Dragon, Killer, Sabertooth.
    /// - Demon (0 in this catalog — WP-210 owns those).
    ///
    /// Total: 9 + 3 = **12**.
    #[test]
    fn test_black_ice_catalog_complete() {
        let cat = load_black_ice_catalog(&catalog_path()).expect("catalog must load");
        assert_eq!(
            cat.len(),
            12,
            "expected 12 Black ICE programs per pp.206-207 (got {}); verify the catalog RON file",
            cat.len()
        );

        // And spot-check every named entry is present.
        for slug in [
            "asp",
            "giant",
            "hellhound",
            "kraken",
            "liche",
            "raven",
            "scorpion",
            "skunk",
            "wisp",
            "dragon",
            "killer",
            "sabertooth",
        ] {
            assert!(
                cat.get(slug).is_some(),
                "missing canonical Black ICE: '{slug}' (pp.206-207)"
            );
        }
    }

    /// Acceptance (WP-209 §): Asp's effect destroys a random installed
    /// Program — see p.206 ("Destroys a single Program installed on
    /// the enemy Netrunner's Cyberdeck at random.").
    #[test]
    fn test_asp_destroys_program() {
        let cat = load_black_ice_catalog(&catalog_path()).expect("catalog must load");
        let asp = cat.get("asp").expect("'asp' must be in the catalog");
        assert_eq!(asp.class, BlackIceClass::AntiPersonnel);
        assert_eq!(asp.effect, BlackIceEffect::DestroyRandomProgram);
        // Stat block per p.206.
        assert_eq!(asp.per, 4);
        assert_eq!(asp.spd, 6);
        assert_eq!(asp.atk, 2);
        assert_eq!(asp.def, 2);
        assert_eq!(asp.rez, 15);
    }

    /// Acceptance (WP-209 §): Hellhound deals brain damage and ignites
    /// the Cyberdeck — see p.206 ("Does 2d6 damage direct to the
    /// Netrunner's brain. Unless insulated, their Cyberdeck catches
    /// fire... 2 damage to their HP whenever they end their Turn.
    /// Multiple instances of this effect cannot stack.").
    #[test]
    fn test_hellhound_brain_damage() {
        let cat = load_black_ice_catalog(&catalog_path()).expect("catalog must load");
        let hh = cat.get("hellhound").expect("'hellhound' must be present");
        assert_eq!(hh.class, BlackIceClass::AntiPersonnel);
        match &hh.effect {
            BlackIceEffect::BrainDamageAndCyberdeckFire {
                dice,
                burn_damage_per_turn_end,
                cannot_stack,
            } => {
                assert_eq!(
                    *dice,
                    DiceSpec {
                        n: 2,
                        die: DieKind::D6
                    }
                );
                assert_eq!(*burn_damage_per_turn_end, 2);
                assert!(
                    *cannot_stack,
                    "p.206: 'Multiple instances ... cannot stack'"
                );
            }
            other => panic!("Hellhound effect must be BrainDamageAndCyberdeckFire, got {other:?}"),
        }
        // Stat block per p.206.
        assert_eq!(hh.per, 6);
        assert_eq!(hh.spd, 6);
        assert_eq!(hh.atk, 6);
        assert_eq!(hh.def, 2);
        assert_eq!(hh.rez, 20);
    }

    /// Acceptance (WP-209 §): the catalog round-trips through RON —
    /// every loaded `BlackIce` re-serialises and re-deserialises to the
    /// same value.
    #[test]
    fn test_black_ice_round_trip_ron() {
        let cat = load_black_ice_catalog(&catalog_path()).expect("catalog must load");
        for (slug, entry) in cat.iter() {
            let serialised = ron::ser::to_string(entry)
                .unwrap_or_else(|e| panic!("{slug}: serialise failed: {e}"));
            let restored: BlackIce = ron::de::from_str(&serialised)
                .unwrap_or_else(|e| panic!("{slug}: deserialise failed: {e}"));
            assert_eq!(*entry, restored, "round-trip mismatch for '{slug}'");
        }
    }

    /// Regression: every catalog row's `id.0` matches its slug — the
    /// loader enforces this at parse time, but the test pins it in
    /// case a bypass is added.
    #[test]
    fn test_id_matches_slug() {
        let cat = load_black_ice_catalog(&catalog_path()).expect("catalog must load");
        for (slug, entry) in cat.iter() {
            assert_eq!(&entry.id.0, slug, "row '{slug}': id.0 must equal slug");
        }
    }

    /// Regression: every catalog row's `price_eb` matches
    /// `price.canonical_cost()` — pinned because the loader enforces
    /// it.
    #[test]
    fn test_price_eb_agrees_with_tier() {
        let cat = load_black_ice_catalog(&catalog_path()).expect("catalog must load");
        for (slug, entry) in cat.iter() {
            assert_eq!(
                entry.price_eb,
                entry.price.canonical_cost(),
                "row '{slug}': price_eb must equal PriceTier::canonical_cost()"
            );
        }
    }

    /// Spot-check a handful of stat blocks against pp.206–207. Catches
    /// transcription errors in the catalog file.
    #[test]
    fn test_canonical_stat_blocks() {
        let cat = load_black_ice_catalog(&catalog_path()).expect("catalog must load");

        // Giant: PER 2, SPD 2, ATK 8, DEF 4, REZ 25, V.Expensive (p.206).
        let giant = cat.get("giant").expect("'giant' present");
        assert_eq!(
            (giant.per, giant.spd, giant.atk, giant.def, giant.rez),
            (2, 2, 8, 4, 25)
        );
        assert_eq!(giant.price, PriceTier::VeryExpensive);

        // Killer: PER 4, SPD 8, ATK 6, DEF 2, REZ 20, Expensive (p.207).
        let killer = cat.get("killer").expect("'killer' present");
        assert_eq!(
            (killer.per, killer.spd, killer.atk, killer.def, killer.rez),
            (4, 8, 6, 2, 20)
        );
        assert_eq!(killer.class, BlackIceClass::AntiProgram);
        assert_eq!(killer.price, PriceTier::Expensive);

        // Sabertooth: PER 8, SPD 6, ATK 6, DEF 2, REZ 25 (p.207).
        let saber = cat.get("sabertooth").expect("'sabertooth' present");
        assert_eq!(
            (saber.per, saber.spd, saber.atk, saber.def, saber.rez),
            (8, 6, 6, 2, 25)
        );
        assert_eq!(saber.class, BlackIceClass::AntiProgram);
    }

    /// Spot-check Anti-Program Black ICE share the
    /// `ProgramDamageDestroyOnDerezz` effect shape with the dice
    /// printed on p.207.
    #[test]
    fn test_anti_program_dice() {
        let cat = load_black_ice_catalog(&catalog_path()).expect("catalog must load");
        let cases = [
            ("dragon", 6u8),     // 6d6 (p.207)
            ("killer", 4u8),     // 4d6 (p.207)
            ("sabertooth", 6u8), // 6d6 (p.207)
        ];
        for (slug, expected_n) in cases {
            let bi = cat.get(slug).unwrap_or_else(|| panic!("'{slug}' present"));
            assert_eq!(bi.class, BlackIceClass::AntiProgram);
            match &bi.effect {
                BlackIceEffect::ProgramDamageDestroyOnDerezz { dice } => {
                    assert_eq!(
                        *dice,
                        DiceSpec {
                            n: expected_n,
                            die: DieKind::D6
                        }
                    );
                }
                other => panic!("{slug}: expected ProgramDamageDestroyOnDerezz, got {other:?}"),
            }
        }
    }

    /// Spot-check Kraken's safe-jack-out lock matches p.207.
    #[test]
    fn test_kraken_locks_safe_jack_out() {
        let cat = load_black_ice_catalog(&catalog_path()).expect("catalog must load");
        let kraken = cat.get("kraken").expect("'kraken' present");
        match &kraken.effect {
            BlackIceEffect::BrainDamageAndJackOutLock {
                dice,
                jackout_consequence,
            } => {
                assert_eq!(
                    *dice,
                    DiceSpec {
                        n: 3,
                        die: DieKind::D6
                    }
                );
                assert_eq!(*jackout_consequence, JackOutKind::SafeJackOutBlocked);
            }
            other => panic!("Kraken effect must be BrainDamageAndJackOutLock, got {other:?}"),
        }
    }

    /// Spot-check Giant's forced-unsafe jack-out matches p.206 — and
    /// the "applies remaining Black ICE on exit" rider.
    #[test]
    fn test_giant_forces_unsafe_jack_out() {
        let cat = load_black_ice_catalog(&catalog_path()).expect("catalog must load");
        let giant = cat.get("giant").expect("'giant' present");
        match &giant.effect {
            BlackIceEffect::BrainDamageAndForceJackOut {
                dice,
                jackout_consequence,
                applies_remaining_black_ice_on_exit,
            } => {
                assert_eq!(
                    *dice,
                    DiceSpec {
                        n: 3,
                        die: DieKind::D6
                    }
                );
                assert_eq!(*jackout_consequence, JackOutKind::ForcedUnsafe);
                assert!(
                    *applies_remaining_black_ice_on_exit,
                    "p.206: Giant applies all remaining Rezzed Black ICE on exit"
                );
            }
            other => panic!("Giant effect must be BrainDamageAndForceJackOut, got {other:?}"),
        }
    }

    /// Spot-check Skunk's stacking flag — p.207 says Skunk effects
    /// stack, distinct from Hellhound's "cannot stack" rider on p.206.
    #[test]
    fn test_skunk_stacks() {
        let cat = load_black_ice_catalog(&catalog_path()).expect("catalog must load");
        let skunk = cat.get("skunk").expect("'skunk' present");
        match &skunk.effect {
            BlackIceEffect::SlideCheckPenalty { penalty, stacks } => {
                assert_eq!(*penalty, 2, "p.207: -2 to Slide Checks");
                assert!(*stacks, "p.207: 'effects of multiple Skunks can stack'");
            }
            other => panic!("Skunk effect must be SlideCheckPenalty, got {other:?}"),
        }
    }

    /// Loader-invariant: a Demon-class row is rejected.
    #[test]
    fn test_loader_rejects_demon_class() {
        let temp = std::env::temp_dir().join("wp209_demon_reject.ron");
        std::fs::write(
            &temp,
            r#"BlackIceFile(
                black_ice: [
                    (
                        slug: "imp",
                        id: BlackIceId("imp"),
                        display_name: "Imp",
                        class: Demon,
                        per: 0, spd: 0, atk: 0, def: 0, rez: 1,
                        effect: DestroyRandomProgram,
                        icon: "test",
                        price: Costly,
                        price_eb: (50),
                    ),
                ],
            )"#,
        )
        .unwrap();
        let err = load_black_ice_catalog(&temp).expect_err("must reject Demon-class row");
        match err {
            RulesError::CatalogLoadFailed { source, .. } => {
                assert!(
                    source.contains("Demon"),
                    "error must mention Demon: {source}"
                );
            }
            other => panic!("expected CatalogLoadFailed, got {other:?}"),
        }
        let _ = std::fs::remove_file(&temp);
    }

    /// Loader-invariant: a row whose `id.0` disagrees with `slug` is
    /// rejected.
    #[test]
    fn test_loader_rejects_id_slug_mismatch() {
        let temp = std::env::temp_dir().join("wp209_id_mismatch.ron");
        std::fs::write(
            &temp,
            r#"BlackIceFile(
                black_ice: [
                    (
                        slug: "asp",
                        id: BlackIceId("not_asp"),
                        display_name: "Asp",
                        class: AntiPersonnel,
                        per: 4, spd: 6, atk: 2, def: 2, rez: 15,
                        effect: DestroyRandomProgram,
                        icon: "test",
                        price: Premium,
                        price_eb: (100),
                    ),
                ],
            )"#,
        )
        .unwrap();
        let err = load_black_ice_catalog(&temp).expect_err("must reject id/slug mismatch");
        match err {
            RulesError::CatalogLoadFailed { source, .. } => {
                assert!(source.contains("slug"), "error must mention slug: {source}");
            }
            other => panic!("expected CatalogLoadFailed, got {other:?}"),
        }
        let _ = std::fs::remove_file(&temp);
    }

    /// Loader-invariant: `price_eb` must equal
    /// `PriceTier::canonical_cost()` — protects against typos in the
    /// authored cost column.
    #[test]
    fn test_loader_rejects_price_disagreement() {
        let temp = std::env::temp_dir().join("wp209_price_disagree.ron");
        std::fs::write(
            &temp,
            r#"BlackIceFile(
                black_ice: [
                    (
                        slug: "asp",
                        id: BlackIceId("asp"),
                        display_name: "Asp",
                        class: AntiPersonnel,
                        per: 4, spd: 6, atk: 2, def: 2, rez: 15,
                        effect: DestroyRandomProgram,
                        icon: "test",
                        price: Premium,
                        price_eb: (50),
                    ),
                ],
            )"#,
        )
        .unwrap();
        let err = load_black_ice_catalog(&temp).expect_err("must reject price disagreement");
        match err {
            RulesError::CatalogLoadFailed { source, .. } => {
                assert!(
                    source.contains("price"),
                    "error must mention price: {source}"
                );
            }
            other => panic!("expected CatalogLoadFailed, got {other:?}"),
        }
        let _ = std::fs::remove_file(&temp);
    }
}
