#![forbid(unsafe_code)]

//! Cyberpunk RED rules engine — pure logic. Dice, checks, combat, netrunning,
//! character derivation. WASM- and native-compatible. Zero feature flags.
//!
//! See `IMPLEMENTATION_PLAN.md` §1.4 (single-source-of-truth) and §2 (conventions).

pub mod catalog;
pub mod character;
pub mod checks;
pub mod dice;
pub mod effects;
pub mod error;
pub mod movement;
pub mod resolution;
pub mod rng;
pub mod types;
pub mod world;

pub use catalog::armor::{Armor, ArmorId, ArmorKind, ArmorLocation, ArmorPenalty};
pub use catalog::critical_injuries::{
    CritTable, CriticalInjury, CriticalInjuryKind, HealMethod, QuickFix, Treatment,
};
pub use catalog::lifepath::{
    BeaconKind, CulturalOriginRow, ExecLifepath, FamilyBackground, FamilyBackgroundRow,
    FixerLifepath, LawmanLifepath, Lifepath, LifepathTable, MediaLifepath, MedtechLifepath,
    NetrunnerLifepath, NomadLifepath, RelationshipBeacon, RockerboyLifepath, RoleLifepath,
    RoleLifepathTables, SoloLifepath, TechLifepath,
};
pub use catalog::roles::{RoleAbilityKind, RoleDefinition};
pub use catalog::skills::{
    Instrument, LanguageKind, LocalArea, MartialArtsForm, ScienceField, SkillCategory,
    SkillDefinition, SkillId,
};
pub use catalog::weapons::{
    load_weapons_catalog, DamageDice, DieKind, Magazine, MeleeKind, RangeBand, RangedKind, Weapon,
    WeaponFeature, WeaponKind,
};
pub use catalog::Catalog;
pub use error::RulesError;
pub use rng::Rng;
