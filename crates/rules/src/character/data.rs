//! Concrete data types that compose [`super::Character`].
//!
//! Every type here is plain data: no methods that compute current values,
//! no validation hooks. Validation belongs to the GM layer / progression
//! actions; query-time derivation belongs to a later WP.
//!
//! `Lifepath` is a placeholder following WP-003's precedent — a stub that
//! compiles today and will be replaced by a closed structure once WP-214
//! lands. `ArmorKind` was one such placeholder; WP-203 has since replaced
//! it with the closed enum re-exported from [`crate::catalog::armor`].
//!
//! [`WeaponId`] is **not** a placeholder: WP-202 keeps it as a string
//! newtype on purpose because the weapon catalog is open-ended (brand
//! variants, exotic weapons of GM's choice — see pp.342, 347), so the
//! lookup key has to stay a free-form slug into the catalog RON.

pub use crate::catalog::armor::ArmorKind;
use crate::effects::{CyberwareId, SkillId, WoundState};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Base stats for a character. See pp.72–73 for definitions.
///
/// Field names track the rulebook's lowercase conventions. `r#ref` and
/// `r#move` are raw identifiers because `ref` and `move` are Rust keywords.
/// Stats are stored as `u8`: in-book stat ranges are 1–10 at character
/// creation and capped at 10 (or 11 with cyberware) — `u8` has plenty of
/// headroom.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatBlock {
    /// INT — Intelligence. See p.72.
    pub int: u8,
    /// REF — Reflexes. See p.72.
    pub r#ref: u8,
    /// DEX — Dexterity. See p.72.
    pub dex: u8,
    /// TECH — Technique. See p.72.
    pub tech: u8,
    /// COOL — Cool. See p.72.
    pub cool: u8,
    /// WILL — Willpower. See p.73.
    pub will: u8,
    /// LUCK — Luck. See p.73.
    pub luck: u8,
    /// MOVE — Movement. See p.73.
    pub r#move: u8,
    /// BODY — Body. See p.73.
    pub body: u8,
    /// EMP — Empathy. See p.73.
    pub emp: u8,
}

/// Base skill ranks. See pp.81–90.
///
/// Stored as `HashMap<SkillId, u8>` — absent entries mean rank 0
/// (untrained). Ranks max at 10 in book RAW; `u8` is the right width.
/// Effects can shift the *effective* rank at query time but never mutate
/// these base values.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSet {
    /// Map of skill identifier to base rank.
    pub ranks: HashMap<SkillId, u8>,
}

/// The ten Cyberpunk RED roles. See pp.36–69.
///
/// Each role drives a Role Ability that scales with `Character::role_rank`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    /// Charismatic Impresario — see p.40.
    Rockerboy,
    /// Combat Specialist — see p.44.
    Solo,
    /// NET Architect & Hacker — see p.48.
    Netrunner,
    /// Maker & Repair Expert — see p.52.
    Tech,
    /// Field Doctor — see p.56.
    Medtech,
    /// Investigative Journalist — see p.60.
    Media,
    /// Street Cop / Detective / Corporate Cop — see p.62.
    Lawman,
    /// Corporate Operative — see p.64.
    Exec,
    /// Broker / Information Trader — see p.66.
    Fixer,
    /// Wanderer / Family Member — see p.68.
    Nomad,
}

/// Hit-point and wound bookkeeping. See pp.79–80, p.186.
///
/// `current_hp` is signed to accommodate intermediate damage application —
/// an attack that brings a character below 0 HP can briefly land them at,
/// say, `-3`, before the combat engine clamps and resolves the kill check.
/// Wound state lookup itself is a query layer concern (a later WP).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wounds {
    /// Current HP. May be negative momentarily during damage application.
    pub current_hp: i16,
    /// Max HP. See p.79: HP = 10 + 5 × ((BODY + WILL) / 2 round up).
    pub max_hp: u16,
    /// Threshold at or below which the character is Seriously Wounded. Per
    /// p.186 this is "Less than 1/2 HP (round up)".
    pub seriously_wounded_threshold: u16,
    /// Base value for the Death Save check — equal to base BODY. See p.186.
    pub death_save_base: u8,
    /// Cumulative penalty applied to Death Saves from prior critical
    /// injuries this scene. See p.186 / p.187.
    pub death_save_penalty: u8,
    /// Cached current wound state. See p.186. The combat engine updates
    /// this whenever `current_hp` crosses a threshold.
    pub current_state: WoundState,
}

/// Worn armor. See p.184. Cyberpunk RED resolves armor in two body
/// locations: the head (only struck on an Aimed Shot) and the body.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WornArmor {
    /// Helmet / head armor, if any.
    pub head: Option<ArmorPiece>,
    /// Body armor, if any.
    pub body: Option<ArmorPiece>,
}

/// One worn armor piece. See p.184.
///
/// `current_sp` and `max_sp` track Stopping Power and ablation: every
/// damaging hit that lands ablates by 1 until the armor is repaired.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArmorPiece {
    /// Closed-enum catalog kind. See [`crate::catalog::armor::ArmorKind`]
    /// (WP-203, rulebook p.185).
    pub kind: ArmorKind,
    /// Current Stopping Power, after any ablation. See p.184.
    pub current_sp: u8,
    /// Stopping Power when freshly repaired. See p.184.
    pub max_sp: u8,
}

/// One installed piece of cyberware. See p.94.
///
/// `options` lists the option-slot fillings on this cyberware (e.g. a
/// Cyberarm with a Big Knuckles option installed). A real catalog model
/// arrives in WP-204; today this is the shape every other crate can rely
/// on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledCyberware {
    /// The cyberware's catalog id.
    pub id: CyberwareId,
    /// Option-slot installs, as catalog ids.
    pub options: Vec<CyberwareId>,
}

/// Owned items not currently worn or wielded.
///
/// A flat list of [`ItemStack`]s — no slotting, no weight tracking. The
/// catalog WPs will refine `ItemKind`'s variants but the shape stays
/// stable.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    /// All carried stacks.
    pub items: Vec<ItemStack>,
}

/// One stack of items in the inventory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemStack {
    /// What the stack contains.
    pub kind: ItemKind,
    /// How many of `kind` are in this stack.
    pub quantity: u32,
}

/// Discriminator for inventory items. Catalog WPs (WP-203, WP-207) will
/// expand the `Misc` shape, but `Weapon` and `Ammo` are stable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    /// A weapon, identified by catalog slug. Real catalog in WP-207.
    Weapon(WeaponId),
    /// Loose ammunition: ammo kind plus round count. See p.344.
    Ammo(AmmoKind, u32),
    /// Anything else — gear, prepak, mission items. Free-form for now.
    Misc(String),
}

/// Weapon catalog slug — the lookup key into the weapon catalog
/// (WP-202, `crates/rules/src/catalog/weapons.rs`).
///
/// Unlike [`crate::catalog::SkillId`], which is a closed enum, `WeaponId`
/// stays a string newtype on purpose: the catalog is open-ended (brand
/// variants per p.342, exotic weapons of the GM's choice per p.347, future
/// DLC additions). The slug is the canonical handle — the loader enforces
/// uniqueness and the `Catalog<Weapon>` lookup converts a `WeaponId` back
/// into the full [`crate::catalog::weapons::Weapon`].
///
/// `Hash` is included so downstream code can key sets / maps on weapons.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct WeaponId(pub String);

/// Ammunition kind. See p.344 (Ammunition section, Night Market appendix).
///
/// Per p.344 RAW: "Ammunition comes in many varieties: Bullet (Medium,
/// Heavy, & Very Heavy Pistol, Slug, or Rifle), Shotgun Shell, Arrow,
/// Grenade, and Rocket". The variants here correspond 1:1 to the book's
/// list, with the bullet sub-types broken out (Medium/Heavy/Very Heavy
/// Pistol caliber chambers feed different magazines per p.171). Shotgun
/// shells are encoded by the weapon's `WeaponFeature::ShotgunShell`
/// alt-fire mode rather than by ammo kind: a shotgun's loaded ammunition
/// is `Slug` per p.171, and shells are an alt-fire option (p.174).
///
/// See [`crate::catalog::weapons::Weapon::magazine`] for how a weapon
/// references its ammo type.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmmoKind {
    /// Medium Pistol caliber. Used by Medium Pistol and SMG (p.171).
    MPistol,
    /// Heavy Pistol caliber. Used by Heavy Pistol and Heavy SMG (p.171).
    HPistol,
    /// Very Heavy Pistol caliber. Used by Very Heavy Pistol (p.171).
    VHPistol,
    /// Shotgun slug round — the default ammunition for a Shotgun's
    /// magazine (p.171). Shotgun shells are loaded as a `ShotgunShell`
    /// alt-fire feature (p.174).
    Slug,
    /// Rifle caliber. Used by Assault Rifle and Sniper Rifle (p.171).
    Rifle,
    /// Arrow — for Bows and Crossbows (p.171). Per p.174, basic arrows can
    /// always be retrieved after firing, so a bow "never needs to Reload".
    Arrow,
    /// Grenade — Grenade Launcher munition (p.171, p.344). Sold per round.
    Grenade,
    /// Rocket — Rocket Launcher munition (p.171, p.344). Sold per round.
    Rocket,
}

/// Lifepath record.
///
/// **Stub.** WP-214 will replace this with the full Streetrat / Complete
/// Lifepath structure (cultural origin, family, what shaped you, etc.,
/// pp.230–254). Today it carries a single placeholder string so a
/// `Character` can be constructed end-to-end.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lifepath {
    /// Free-form slug. Replaced by WP-214's structured fields.
    pub placeholder: String,
}
