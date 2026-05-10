//! Encounter loader — load and instantiate combat encounters from RON.
//!
//! ## Tile format convention
//!
//! Grid tiles are encoded as a flat ASCII string of length `width * height`,
//! stored in row-major order (`y * width + x`). Characters map to
//! [`cpr_rules::combat::grid::TileKind`] as follows:
//!
//! | Char | TileKind   |
//! |------|------------|
//! | `'.'` | Open (floor, passable, clear terrain) |
//! | `'#'` | Wall (impassable, blocks LOS) |
//! | `'~'` | Difficult terrain (passable at normal cost; preserved for future houserule hooks) |
//! | `'w'` | Water (passable at normal cost; preserved for future houserule hooks) |
//!
//! Any other character is treated as `Open` and emits no error (forward-compat).
//!
//! ## Surprised initial state
//!
//! When an `EnemyPlacement` carries `EnemyInitialState::Surprised`, the
//! corresponding [`cpr_rules::combat::turn_engine::InitiativeEntry`] is
//! post-processed after [`cpr_rules::combat::turn_engine::CombatState::start`]:
//! both `action_used` and `move_used` are set to `true`, causing the entity to
//! skip its first turn entirely.
//!
//! **Rulebook reference — p.169 (Friday Night Firefight, "Hold Action"):**
//! A character that is surprised has no opportunity to react before combat
//! begins. RAW does not define a formal "Surprised condition" with a stat
//! modifier, but the intent (lose their first-turn actions) is clear from the
//! ambush and initiative context. This implementation encodes it as
//! `action_used = true, move_used = true` on their `InitiativeEntry` so the
//! first call to `CombatState::end_turn` advances past them without granting
//! any actions. This is flagged as a RAW-interpretation deviation in the PR.

#![forbid(unsafe_code)]

use crate::npc::entity::{NpcTemplate, NpcTemplateId};
use crate::npc::instantiate::{instantiate_npc, CatalogBundle};
use crate::{EncounterId, GmError, Rng, World};
use cpr_rules::combat::grid::{CoverInstance, Grid, TileKind};
use cpr_rules::combat::turn_engine::CombatState;
use cpr_rules::effects::{ActiveEffect, EffectDuration, EffectSource, EnvironmentalKind};
use cpr_rules::types::{EffectInstanceId, EntityId, DV};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// GridDef
// ---------------------------------------------------------------------------

/// Author-time definition of a combat grid.
///
/// `tiles` is a flat ASCII string of length `width * height`, in row-major
/// order (`y * width + x`). See module-level docs for the tile encoding.
///
/// `cover` is a list of `(x, y, material_slug)` tuples placing cover objects
/// on the grid. Slugs reference entries in the cover catalog (WP-207).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridDef {
    /// Number of columns (squares along X).
    pub width: u16,
    /// Number of rows (squares along Y).
    pub height: u16,
    /// Tile string, row-major. Length must equal `width * height`.
    pub tiles: String,
    /// Cover placements: `(x, y, material_slug)`.
    pub cover: Vec<(u16, u16, String)>,
}

// ---------------------------------------------------------------------------
// EnemyInitialState
// ---------------------------------------------------------------------------

/// The state an enemy NPC is in when the encounter begins.
///
/// Used by [`instantiate_encounter`] to apply any per-entity combat
/// adjustments at encounter start. Only `Surprised` has a mechanical effect
/// right now; the others are preserved for future AI-behaviour WPs.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnemyInitialState {
    /// Enemy is walking a patrol route — not yet alert.
    Patrolling,
    /// Enemy is stationed and watching — heightened awareness but no bonus.
    OnGuard,
    /// Enemy is asleep — future WPs may apply a Perception penalty.
    Asleep,
    /// Enemy is surprised and will skip their first turn.
    ///
    /// See module-level docs and p.169 for the RAW rationale.
    Surprised,
    /// Enemy is already engaged with another combatant at encounter start.
    Engaged,
}

// ---------------------------------------------------------------------------
// EnemyPlacement
// ---------------------------------------------------------------------------

/// An enemy NPC placed on the grid at encounter start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnemyPlacement {
    /// Which NPC template to instantiate. See [`crate::npc::entity::NpcTemplate`].
    pub template: NpcTemplateId,
    /// Grid coordinates `(x, y)` where the enemy starts. Must satisfy
    /// `x < grid.width && y < grid.height`.
    pub position: (u16, u16),
    /// Behavioural / mechanical state at encounter start.
    pub initial_state: EnemyInitialState,
}

// ---------------------------------------------------------------------------
// Encounter
// ---------------------------------------------------------------------------

/// Author-time combat encounter definition, loaded from `content/encounters/`.
///
/// An `Encounter` holds the static description of a fight: the grid, the
/// enemies and their positions, any environmental effects, and an optional
/// stealth-approach DV that determines whether the players can surprise the
/// enemies before initiative is rolled.
///
/// Call [`load_encounter`] to parse a RON file into this struct, then
/// [`instantiate_encounter`] to produce a live [`CombatState`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Encounter {
    /// Stable slug identifier for this encounter.
    pub id: EncounterId,
    /// Human-readable name shown in the UI.
    pub display_name: String,
    /// Grid map for the fight. See [`GridDef`].
    pub grid: GridDef,
    /// Enemy placements. Each entry becomes one live [`crate::npc::entity::ActiveNpc`].
    pub enemies: Vec<EnemyPlacement>,
    /// Environmental modifiers active throughout the encounter.
    ///
    /// Each variant is applied as a [`cpr_rules::effects::EnvironmentalKind`]
    /// effect on the [`World`] at instantiation time.
    pub environment: Vec<EnvironmentalKind>,
    /// DV for a stealth approach check before initiative is rolled.
    ///
    /// When `Some(dv)`, the caller should run a COOL + Stealth check against
    /// this DV before calling [`instantiate_encounter`]. If the check succeeds,
    /// set the relevant enemies to [`EnemyInitialState::Surprised`] before
    /// calling `instantiate_encounter`. This WP does not perform the check —
    /// that is the caller's responsibility.
    pub stealth_approach_dv: Option<DV>,
    /// Whether the player may hire allies to join this encounter.
    pub allies_can_join: bool,
}

// ---------------------------------------------------------------------------
// load_encounter
// ---------------------------------------------------------------------------

/// Parse a single encounter RON file into an [`Encounter`].
///
/// # Errors
///
/// - [`GmError::EncounterLoadFailed`] — if the file cannot be read or if the
///   RON cannot be parsed.
pub fn load_encounter(path: &Path) -> Result<Encounter, GmError> {
    let source = std::fs::read_to_string(path).map_err(|e| GmError::EncounterLoadFailed {
        path: path.to_path_buf(),
        detail: format!("I/O error: {e}"),
    })?;

    ron::from_str::<Encounter>(&source).map_err(|e| GmError::EncounterLoadFailed {
        path: path.to_path_buf(),
        detail: format!("RON parse error: {e}"),
    })
}

// ---------------------------------------------------------------------------
// instantiate_encounter
// ---------------------------------------------------------------------------

/// Build a [`CombatState`] populated with all enemy NPCs at their declared
/// positions on a grid built from [`GridDef`].
///
/// Steps:
/// 1. Validate `GridDef.tiles.len() == width * height`.
/// 2. Build the [`Grid`] from the tile string and cover list.
/// 3. For each [`EnemyPlacement`]:
///    a. Validate the position is within grid bounds.
///    b. Look up the template in `npc_templates`.
///    c. Call [`instantiate_npc`] to resolve the live [`crate::npc::entity::ActiveNpc`].
///    d. Register the NPC's character in `world.npcs`.
///    e. Place the entity on the grid.
/// 4. Apply environmental effects from `enc.environment` as
///    [`ActiveEffect`]s on the world's PC entity (representing area-wide
///    conditions; individual entity effects are handled by the rules engine).
/// 5. Call [`CombatState::start`] with all enemy entity IDs to roll
///    initiative and build the queue.
/// 6. Post-process: for any enemy with [`EnemyInitialState::Surprised`],
///    set `action_used = true` and `move_used = true` on their
///    [`cpr_rules::combat::turn_engine::InitiativeEntry`], causing them
///    to skip their first turn (see module-level docs for RAW rationale).
///
/// # Errors
///
/// - [`GmError::GridDimensionMismatch`] — tiles string length ≠ width × height.
/// - [`GmError::EnemyPositionOutOfBounds`] — a position is outside the grid.
/// - [`GmError::NpcTemplateNotFound`] — a template slug is missing.
/// - Propagates [`GmError::MookArchetypeNotFound`] and [`GmError::LoadoutItemNotFound`]
///   from [`instantiate_npc`].
///
/// Rulebook reference: **p.169** (surprise / initiative queue).
pub fn instantiate_encounter(
    enc: &Encounter,
    npc_templates: &HashMap<NpcTemplateId, NpcTemplate>,
    catalog: &CatalogBundle<'_>,
    world: &mut World,
    rng: &mut Rng,
) -> Result<CombatState, GmError> {
    // ── Step 1: validate grid dimensions ────────────────────────────────────
    let expected_len = usize::from(enc.grid.width) * usize::from(enc.grid.height);
    if enc.grid.tiles.len() != expected_len {
        return Err(GmError::GridDimensionMismatch {
            encounter: enc.id.clone(),
        });
    }

    // ── Step 2: build the Grid ───────────────────────────────────────────────
    let mut grid = Grid::new(enc.grid.width, enc.grid.height);

    // Parse tile characters.
    for (idx, ch) in enc.grid.tiles.chars().enumerate() {
        let x = (idx % usize::from(enc.grid.width)) as u16;
        let y = (idx / usize::from(enc.grid.width)) as u16;
        let kind = char_to_tile(ch);
        grid.set_tile((x, y), kind);
    }

    // Place cover objects.
    for (cx, cy, ref material) in &enc.grid.cover {
        // Cover HP is looked up from the cover catalog at play time; here we
        // record the material slug with placeholder HP = 0, which signals the
        // combat engine to resolve HP from the catalog before the first attack.
        // This matches the pattern used by WP-302's existing tests.
        grid.cover_objects.insert(
            (*cx, *cy),
            CoverInstance {
                material: material.clone(),
                current_hp: 0,
                max_hp: 0,
            },
        );
    }

    // ── Step 3: instantiate enemies and populate world ───────────────────────
    let mut placements: Vec<(EntityId, EnemyInitialState)> = Vec::new();

    for placement in &enc.enemies {
        // Validate position bounds.
        let (x, y) = placement.position;
        if x >= enc.grid.width || y >= enc.grid.height {
            return Err(GmError::EnemyPositionOutOfBounds {
                encounter: enc.id.clone(),
                x,
                y,
                width: enc.grid.width,
                height: enc.grid.height,
            });
        }

        // Look up template.
        let template = npc_templates
            .get(&placement.template)
            .ok_or_else(|| GmError::NpcTemplateNotFound(placement.template.0.clone()))?;

        // Instantiate the NPC (this draws from `rng` to generate its EntityId).
        let active = instantiate_npc(template, catalog, rng)?;
        let entity_id = active.entity_id;

        // Register in world.npcs so that `world.entity(entity_id)` resolves
        // during `CombatState::start` initiative rolling.
        use cpr_rules::types::NpcId;
        world.npcs.insert(NpcId(entity_id.0), active.character);

        // Place on grid.
        grid.place(entity_id, placement.position);

        placements.push((entity_id, placement.initial_state));
    }

    // ── Step 4: apply environmental effects ─────────────────────────────────
    // Environmental effects are applied to the PC entity as area-wide
    // conditions. Individual enemy entities pick them up via the rules engine
    // during action resolution. Using deterministic EffectInstanceIds derived
    // from their index maintains replay safety.
    for (i, &env_kind) in enc.environment.iter().enumerate() {
        let effect_id = EffectInstanceId(Uuid::from_u128(i as u128 + 0xFF00_0000));
        let effect = ActiveEffect {
            id: effect_id,
            source: EffectSource::Environmental(env_kind),
            modifiers: vec![],
            duration: EffectDuration::UntilGigEnd,
        };
        world.pc.effects.add(effect);
    }

    // ── Step 5: build CombatState ────────────────────────────────────────────
    let entity_ids: Vec<EntityId> = placements.iter().map(|(eid, _)| *eid).collect();
    let mut combat = CombatState::start(entity_ids, world, rng);

    // Attach the grid we built (CombatState::start creates a default empty one).
    combat.grid = grid;

    // ── Step 6: apply Surprised initial state ────────────────────────────────
    // Per p.169 RAW: surprised characters lose their first turn.
    // We encode this by marking action_used = true and move_used = true
    // on their InitiativeEntry. See module-level docs for full rationale.
    for (entity_id, initial_state) in &placements {
        if *initial_state == EnemyInitialState::Surprised {
            if let Some(entry) = combat.queue.iter_mut().find(|e| &e.entity == entity_id) {
                entry.action_used = true;
                entry.move_used = true;
            }
        }
    }

    Ok(combat)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Map a tile character to a [`TileKind`].
///
/// Convention (see module-level docs):
/// - `'.'` → [`TileKind::Open`]
/// - `'#'` → [`TileKind::Wall`]
/// - `'~'` → [`TileKind::Difficult`]
/// - `'w'` → [`TileKind::Water`]
/// - anything else → [`TileKind::Open`] (forward-compatible)
fn char_to_tile(ch: char) -> TileKind {
    match ch {
        '.' => TileKind::Open,
        '#' => TileKind::Wall,
        '~' => TileKind::Difficult,
        'w' => TileKind::Water,
        _ => TileKind::Open,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::npc::entity::{Loadout, MookArchetype, NpcTemplate, NpcTemplateId, NpcTemplateKind};
    use crate::npc::instantiate::MookStatline;
    use cpr_rules::catalog::armor::Armor;
    use cpr_rules::catalog::cyberware::Cyberware;
    use cpr_rules::catalog::weapons::Weapon;
    use cpr_rules::character::data::{Inventory, Role, SkillSet, StatBlock, WornArmor, Wounds};
    use cpr_rules::character::Character;
    use cpr_rules::effects::EffectStack;
    use cpr_rules::types::{CharacterId, Eurobucks};
    use cpr_rules::{Catalog, Lifepath};
    use rand_core::SeedableRng;
    use std::collections::HashMap;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use uuid::Uuid;

    // ── Catalog helpers ───────────────────────────────────────────────────────

    fn empty_weapon_catalog() -> Catalog<Weapon> {
        Catalog::new(HashMap::new())
    }

    fn empty_armor_catalog() -> Catalog<Armor> {
        Catalog::new(HashMap::new())
    }

    fn empty_cyberware_catalog() -> Catalog<Cyberware> {
        Catalog::new(HashMap::new())
    }

    fn goon_statline() -> MookStatline {
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

    fn make_mook_catalog(statlines: Vec<MookStatline>) -> Catalog<MookStatline> {
        let entries: HashMap<String, MookStatline> = statlines
            .into_iter()
            .map(|s| (format!("{:?}", s.archetype), s))
            .collect();
        Catalog::new(entries)
    }

    fn make_catalog_bundle<'a>(
        weapons: &'a Catalog<Weapon>,
        armor: &'a Catalog<Armor>,
        cyberware: &'a Catalog<Cyberware>,
        mooks: &'a Catalog<MookStatline>,
    ) -> CatalogBundle<'a> {
        CatalogBundle {
            weapons,
            armor,
            cyberware,
            mooks,
        }
    }

    fn goon_template(id: &str) -> NpcTemplate {
        NpcTemplate {
            id: NpcTemplateId::from(id),
            display_name: id.to_string(),
            kind: NpcTemplateKind::Mook {
                archetype: MookArchetype::Goon,
                loadout: Loadout {
                    weapons: vec![],
                    armor: None,
                    cyberware: vec![],
                },
            },
            description: "Test goon".to_string(),
            initial_disposition: -3,
            voice_notes: String::new(),
        }
    }

    fn minimal_pc() -> Character {
        Character {
            id: CharacterId(Uuid::from_u128(0xBEEF)),
            name: "TestPC".to_string(),
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
            skills: SkillSet::default(),
            cyberware: vec![],
            armor: WornArmor::default(),
            wounds: Wounds {
                current_hp: 40,
                max_hp: 40,
                seriously_wounded_threshold: 20,
                death_save_base: 5,
                death_save_penalty: 0,
                current_state: cpr_rules::effects::WoundState::None,
            },
            effects: EffectStack::default(),
            inventory: Inventory::default(),
            money: Eurobucks(0),
            humanity: 50,
            luck_pool: 5,
            improvement_points: 0,
            complementary_bonuses: vec![],
            lifepath: Lifepath::default(),
        }
    }

    fn seeded_rng() -> Rng {
        Rng::seed_from_u64(42)
    }

    // ── test_load_encounter_round_trip ────────────────────────────────────────

    /// `test_load_encounter_round_trip` — write a sample Encounter to RON,
    /// parse it back via `load_encounter`, and assert equality.
    #[test]
    fn test_load_encounter_round_trip() {
        let enc = Encounter {
            id: EncounterId::from("warehouse_lobby"),
            display_name: "Warehouse Lobby".to_string(),
            grid: GridDef {
                width: 5,
                height: 4,
                tiles: ".....#...#.....#...#".to_string(),
                cover: vec![(1, 1, "concrete_barricade".to_string())],
            },
            enemies: vec![
                EnemyPlacement {
                    template: NpcTemplateId::from("goon_a"),
                    position: (2, 1),
                    initial_state: EnemyInitialState::OnGuard,
                },
                EnemyPlacement {
                    template: NpcTemplateId::from("goon_b"),
                    position: (3, 2),
                    initial_state: EnemyInitialState::Patrolling,
                },
            ],
            environment: vec![EnvironmentalKind::Darkness],
            stealth_approach_dv: Some(DV(15)),
            allies_can_join: true,
        };

        let ron_str = ron::to_string(&enc).expect("serialization must succeed");

        let mut tmp = NamedTempFile::new().expect("temp file");
        tmp.write_all(ron_str.as_bytes()).expect("write temp file");
        tmp.flush().expect("flush temp file");

        let loaded = load_encounter(tmp.path()).expect("load_encounter must succeed");

        assert_eq!(
            enc, loaded,
            "loaded Encounter must equal the original struct"
        );
        assert_eq!(loaded.id, EncounterId::from("warehouse_lobby"));
        assert_eq!(loaded.stealth_approach_dv, Some(DV(15)));
        assert_eq!(loaded.enemies.len(), 2);
        assert_eq!(loaded.grid.cover.len(), 1);
    }

    // ── test_instantiate_places_enemies ──────────────────────────────────────

    /// `test_instantiate_places_enemies` — instantiate a 2-enemy encounter
    /// and assert both ActiveNpcs appear on the grid at their declared positions.
    #[test]
    fn test_instantiate_places_enemies() {
        let mook_cat = make_mook_catalog(vec![goon_statline()]);
        let weapon_cat = empty_weapon_catalog();
        let armor_cat = empty_armor_catalog();
        let cw_cat = empty_cyberware_catalog();
        let catalog = make_catalog_bundle(&weapon_cat, &armor_cat, &cw_cat, &mook_cat);

        let mut templates = HashMap::new();
        templates.insert(NpcTemplateId::from("goon_1"), goon_template("goon_1"));
        templates.insert(NpcTemplateId::from("goon_2"), goon_template("goon_2"));

        let enc = Encounter {
            id: EncounterId::from("test_encounter"),
            display_name: "Test".to_string(),
            grid: GridDef {
                width: 10,
                height: 10,
                // 10×10 open grid
                tiles: ".".repeat(100),
                cover: vec![],
            },
            enemies: vec![
                EnemyPlacement {
                    template: NpcTemplateId::from("goon_1"),
                    position: (2, 3),
                    initial_state: EnemyInitialState::Patrolling,
                },
                EnemyPlacement {
                    template: NpcTemplateId::from("goon_2"),
                    position: (7, 5),
                    initial_state: EnemyInitialState::OnGuard,
                },
            ],
            environment: vec![],
            stealth_approach_dv: None,
            allies_can_join: false,
        };

        let mut world = World::new(minimal_pc());
        let mut rng = seeded_rng();

        let combat = instantiate_encounter(&enc, &templates, &catalog, &mut world, &mut rng)
            .expect("instantiate_encounter must succeed");

        // Both enemies must be participants.
        assert_eq!(
            combat.participants.len(),
            2,
            "both enemies must be in the participant set"
        );

        // Each participant entity must appear on the grid at the declared position.
        for entity_id in &combat.participants {
            let pos = combat
                .grid
                .position_of(*entity_id)
                .expect("enemy entity must be on grid");
            // The position must be one of the two declared positions.
            assert!(
                pos == (2, 3) || pos == (7, 5),
                "entity at unexpected position: {pos:?}"
            );
        }

        // The initiative queue must have exactly 2 entries.
        assert_eq!(
            combat.queue.len(),
            2,
            "initiative queue must have 2 entries"
        );
    }

    // ── test_stealth_approach_dv_threading ───────────────────────────────────

    /// `test_stealth_approach_dv_threading` — DV(15) round-trips through
    /// `load_encounter`, and a `Surprised` enemy has `action_used = true`
    /// and `move_used = true` in the initiative queue.
    #[test]
    fn test_stealth_approach_dv_threading() {
        // ── Part 1: DV round-trips through RON ──────────────────────────────
        let enc = Encounter {
            id: EncounterId::from("stealth_test"),
            display_name: "Stealth Test".to_string(),
            grid: GridDef {
                width: 5,
                height: 5,
                tiles: ".".repeat(25),
                cover: vec![],
            },
            enemies: vec![],
            environment: vec![],
            stealth_approach_dv: Some(DV(15)),
            allies_can_join: false,
        };

        let ron_str = ron::to_string(&enc).expect("serialize");
        let mut tmp = NamedTempFile::new().expect("temp file");
        tmp.write_all(ron_str.as_bytes()).expect("write");
        tmp.flush().expect("flush");

        let loaded = load_encounter(tmp.path()).expect("load");
        assert_eq!(
            loaded.stealth_approach_dv,
            Some(DV(15)),
            "stealth_approach_dv must survive round-trip"
        );

        // ── Part 2: Surprised enemy skips first turn ─────────────────────────
        let mook_cat = make_mook_catalog(vec![goon_statline()]);
        let weapon_cat = empty_weapon_catalog();
        let armor_cat = empty_armor_catalog();
        let cw_cat = empty_cyberware_catalog();
        let catalog = make_catalog_bundle(&weapon_cat, &armor_cat, &cw_cat, &mook_cat);

        let mut templates = HashMap::new();
        templates.insert(
            NpcTemplateId::from("surprised_goon"),
            goon_template("surprised_goon"),
        );

        let enc2 = Encounter {
            id: EncounterId::from("stealth_test_2"),
            display_name: "Stealth Test 2".to_string(),
            grid: GridDef {
                width: 5,
                height: 5,
                tiles: ".".repeat(25),
                cover: vec![],
            },
            enemies: vec![EnemyPlacement {
                template: NpcTemplateId::from("surprised_goon"),
                position: (2, 2),
                initial_state: EnemyInitialState::Surprised,
            }],
            environment: vec![],
            stealth_approach_dv: Some(DV(15)),
            allies_can_join: false,
        };

        let mut world = World::new(minimal_pc());
        let mut rng = seeded_rng();

        let combat = instantiate_encounter(&enc2, &templates, &catalog, &mut world, &mut rng)
            .expect("instantiate must succeed");

        // The surprised enemy's initiative entry must have both action flags set.
        assert_eq!(combat.queue.len(), 1, "one enemy in the initiative queue");
        let entry = &combat.queue[0];
        assert!(
            entry.action_used,
            "Surprised enemy must have action_used=true (skips first turn; p.169)"
        );
        assert!(
            entry.move_used,
            "Surprised enemy must have move_used=true (skips first turn; p.169)"
        );
    }

    // ── test_position_out_of_bounds_errors ───────────────────────────────────

    /// `test_position_out_of_bounds_errors` — enemy at (100,100) on a 10×10
    /// grid returns `EnemyPositionOutOfBounds`.
    #[test]
    fn test_position_out_of_bounds_errors() {
        let mook_cat = make_mook_catalog(vec![goon_statline()]);
        let weapon_cat = empty_weapon_catalog();
        let armor_cat = empty_armor_catalog();
        let cw_cat = empty_cyberware_catalog();
        let catalog = make_catalog_bundle(&weapon_cat, &armor_cat, &cw_cat, &mook_cat);

        let mut templates = HashMap::new();
        templates.insert(NpcTemplateId::from("oob_goon"), goon_template("oob_goon"));

        let enc = Encounter {
            id: EncounterId::from("oob_encounter"),
            display_name: "OOB".to_string(),
            grid: GridDef {
                width: 10,
                height: 10,
                tiles: ".".repeat(100),
                cover: vec![],
            },
            enemies: vec![EnemyPlacement {
                template: NpcTemplateId::from("oob_goon"),
                position: (100, 100),
                initial_state: EnemyInitialState::Patrolling,
            }],
            environment: vec![],
            stealth_approach_dv: None,
            allies_can_join: false,
        };

        let mut world = World::new(minimal_pc());
        let mut rng = seeded_rng();

        let result = instantiate_encounter(&enc, &templates, &catalog, &mut world, &mut rng);

        assert!(
            matches!(
                result,
                Err(GmError::EnemyPositionOutOfBounds { x: 100, y: 100, .. })
            ),
            "expected EnemyPositionOutOfBounds, got: {result:?}"
        );
    }

    // ── test_grid_dimension_mismatch_errors ───────────────────────────────────

    /// `test_grid_dimension_mismatch_errors` — tiles string with wrong length
    /// returns `GridDimensionMismatch`.
    #[test]
    fn test_grid_dimension_mismatch_errors() {
        let mook_cat = make_mook_catalog(vec![goon_statline()]);
        let weapon_cat = empty_weapon_catalog();
        let armor_cat = empty_armor_catalog();
        let cw_cat = empty_cyberware_catalog();
        let catalog = make_catalog_bundle(&weapon_cat, &armor_cat, &cw_cat, &mook_cat);

        let templates: HashMap<NpcTemplateId, NpcTemplate> = HashMap::new();

        // 10×10 grid but only 50 tile chars — wrong.
        let enc = Encounter {
            id: EncounterId::from("dim_mismatch"),
            display_name: "Dim Mismatch".to_string(),
            grid: GridDef {
                width: 10,
                height: 10,
                tiles: ".".repeat(50),
                cover: vec![],
            },
            enemies: vec![],
            environment: vec![],
            stealth_approach_dv: None,
            allies_can_join: false,
        };

        let mut world = World::new(minimal_pc());
        let mut rng = seeded_rng();

        let result = instantiate_encounter(&enc, &templates, &catalog, &mut world, &mut rng);

        assert!(
            matches!(result, Err(GmError::GridDimensionMismatch { .. })),
            "expected GridDimensionMismatch, got: {result:?}"
        );
    }

    // ── test_missing_npc_template_errors ─────────────────────────────────────

    /// `test_missing_npc_template_errors` — encounter referencing an unknown
    /// NpcTemplateId returns `NpcTemplateNotFound`.
    #[test]
    fn test_missing_npc_template_errors() {
        let mook_cat = make_mook_catalog(vec![goon_statline()]);
        let weapon_cat = empty_weapon_catalog();
        let armor_cat = empty_armor_catalog();
        let cw_cat = empty_cyberware_catalog();
        let catalog = make_catalog_bundle(&weapon_cat, &armor_cat, &cw_cat, &mook_cat);

        // Empty template map — slug "ghost" won't resolve.
        let templates: HashMap<NpcTemplateId, NpcTemplate> = HashMap::new();

        let enc = Encounter {
            id: EncounterId::from("missing_template"),
            display_name: "Missing Template".to_string(),
            grid: GridDef {
                width: 5,
                height: 5,
                tiles: ".".repeat(25),
                cover: vec![],
            },
            enemies: vec![EnemyPlacement {
                template: NpcTemplateId::from("ghost"),
                position: (1, 1),
                initial_state: EnemyInitialState::Patrolling,
            }],
            environment: vec![],
            stealth_approach_dv: None,
            allies_can_join: false,
        };

        let mut world = World::new(minimal_pc());
        let mut rng = seeded_rng();

        let result = instantiate_encounter(&enc, &templates, &catalog, &mut world, &mut rng);

        match result {
            Err(GmError::NpcTemplateNotFound(slug)) => {
                assert_eq!(slug, "ghost", "error must name the missing template slug");
            }
            other => panic!("expected NpcTemplateNotFound, got: {other:?}"),
        }
    }
}
