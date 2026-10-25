//! A simple example to show how to spawn an infinite grid.
//!
//! Infinite grids are useful as the ground plane in editor-like applications,
//! as they provide a consistent reference for the orientation of objects
//! and an evenly spaced grid to judge relative scale.

mod scene;
mod tilemap;
mod fd;

use std::{ffi::OsString, sync::Arc};

use bevy::{
    dev_tools::infinite_grid::{InfiniteGrid, InfiniteGridPlugin, InfiniteGridSettings}, ecs::{lifecycle::HookContext, world::DeferredWorld}, input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll}, log::tracing_subscriber::filter::combinator::And, math::NormedVectorSpace, picking::backend::{
        HitData, HitDataExtra, PointerHits,
        ray::{RayId, RayMap},
    }, platform::collections::{HashMap, HashSet, hash_map::Entry}, prelude::*, window::CursorOptions,
};
use serde::{Deserialize, Serialize};
use tiny_bail::or_continue;

use crate::{
    TileFace::InfinitePlane, scene::{History, Redo, Scene, ScenePlugin, Undo}, tilemap::{TileData, TileType, Tilemap, TilemapImage, TilemapRayHit, tilemap_coord_to_center},
};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(ImagePlugin::default_nearest()),
            InfiniteGridPlugin,
            ScenePlugin,
            // FpsOverlayPlugin {
            //     ..Default::default()
            // },
            //
        ))
        .add_systems(Startup, setup_system)
        .add_systems(
            Update,
            (
                move_system,
                // place_system,
                update_grid,
                keyboard_funcs,
                regenerate_dirty_tilemaps,
                update_hits,
                draw_selection_gizmos,
            ),
        )
        .init_resource::<MeshData>()
        .init_resource::<History>()
        .run();
}

fn observe_draw_hit_gizmos(event: On<PointerMove>, mut gizmos: Query<&mut SelectionGizmos>) {
    let PointerMove {
        entity: _,
        pointer: _,
        delta: _,
        hit,
    } = event.event();

    let tile_hit = *hit.extra_as::<TileHitData>().unwrap();

    let mut gizmos = gizmos.get_mut(hit.camera).unwrap();

    *gizmos = SelectionGizmos {
        circle_iso: Isometry3d::new(
            hit.position.unwrap(),
            Transform::default()
                .looking_to(hit.normal.unwrap(), Vec3::Y)
                .rotation,
        ),
        tile_pos: tile_hit.tile,
        new_tile_pos: tile_hit.tile + tile_hit.face.new_tile_offset(),
    };
}

#[derive(Debug, Component, Default)]
struct SelectionGizmos {
    circle_iso: Isometry3d,
    tile_pos: IVec3,
    new_tile_pos: IVec3,
}

fn draw_selection_gizmos(mut gizmos: Gizmos, selection_gizmos: Query<&SelectionGizmos>) {
    for selection in selection_gizmos.iter() {
        gizmos.circle(selection.circle_iso, 0.125, Color::WHITE);

        gizmos.cube(
            Transform::from_translation(tilemap_coord_to_center(selection.tile_pos)),
            Color::linear_rgb(1.0, 0.0, 0.0),
        );

        gizmos.cube(
            Transform::from_translation(tilemap_coord_to_center(selection.new_tile_pos)),
            Color::WHITE,
        );
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TileFace {
    Cube(CubeTileFace),
    // TODO: Replace with better plane normal
    InfinitePlane(IVec3),
    // Slope
    // Corner
}

impl TileFace {
    pub fn new_tile_offset(&self) -> IVec3 {
        match self {
            Self::Cube(cube) => cube.new_tile_offset(),
            Self::InfinitePlane(normal) => *normal,
        }
    }

    fn active_plane(&self, tile: IVec3) -> ActivePlane {
        match *self {
            TileFace::Cube(cube_tile_face) => {
                let normal = cube_tile_face.new_tile_offset();
                let depth = tile.dot(normal) + 1;
                ActivePlane { depth, normal }
            }
            InfinitePlane(normal) => {
                let depth = tile.dot(normal) + 1;
                ActivePlane { depth, normal }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CubeTileFace {
    Top,
    Bottom,
    Left,
    Right,
    Front,
    Back,
}

impl CubeTileFace {
    pub fn new_tile_offset(&self) -> IVec3 {
        match self {
            CubeTileFace::Top => IVec3::Y,
            CubeTileFace::Bottom => IVec3::NEG_Y,
            CubeTileFace::Left => IVec3::NEG_X,
            CubeTileFace::Right => IVec3::X,
            CubeTileFace::Front => IVec3::Z,
            CubeTileFace::Back => IVec3::NEG_Z,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TileHitData {
    pub tile: IVec3,
    pub face: TileFace,
}
impl TileHitData {
    fn active_plane(&self) -> ActivePlane {
        self.face.active_plane(self.tile)
    }
}

impl From<TileHitData> for Option<Arc<dyn HitDataExtra>> {
    fn from(value: TileHitData) -> Self {
        Some(Arc::new(value))
    }
}

fn update_hits(
    ray_map: Res<RayMap>,
    mut output_messages: MessageWriter<PointerHits>,
    camera: Query<(&Camera, &mut SelectionState)>,
    tilemap: Single<(Entity, &Tilemap)>,
    grid: Single<Entity, (With<InfiniteGrid>, Without<ActiveGrid>)>,
    active_grid: Single<Entity, (With<InfiniteGrid>, With<ActiveGrid>)>,
) {
    let (tilemap_entity, tilemap) = tilemap.into_inner();
    for (&ray_id, &ray) in ray_map.iter() {
        let (camera, selection_state) = or_continue!(camera.get(ray_id.camera));

        let pick = match &*selection_state {
            SelectionState::None => {
                if let Some(hit) = tilemap.ray_intersection(ray)
                {
                    Some((
                        tilemap_entity,
                        HitData {
                            camera: ray_id.camera,
                            depth: hit.time_of_impact,
                            position: Some(ray.get_point(hit.time_of_impact)),
                            normal: Some(hit.hit_normal),
                            extra: TileHitData {
                                tile: hit.tile,
                                face: hit.face,
                            }
                            .into(),
                        },
                    ))
                } else {
                    ray.intersect_plane(Vec3::ZERO, InfinitePlane3d { normal: Dir3::Y })
                        .map(|time_of_impact| {
                            let tile = (ray.get_point(time_of_impact) - Dir3::Y * 0.01)
                                .floor()
                                .as_ivec3();
                            (
                                *grid,
                                HitData {
                                    camera: ray_id.camera,
                                    depth: time_of_impact,
                                    position: Some(ray.get_point(time_of_impact)),
                                    normal: Some(Vec3::Y),
                                    extra: TileHitData {
                                        tile,
                                        face: TileFace::InfinitePlane(IVec3::Y),
                                    }
                                    .into(),
                                },
                            )
                        })
                }
            }
            SelectionState::Adding { active_plane, .. } | SelectionState::Removing {active_plane, .. } => {
                active_plane
                    .ray_intersection(ray, ray_id)
                    .map(|hit| (*active_grid, hit))
            }
        };

        output_messages.write(PointerHits {
            pointer: ray_id.pointer,
            picks: Vec::from(pick.as_slice()),
            order: camera.order as f32,
        });
    }
}

fn keyboard_funcs(
    mut tilemap_entity: Single<(Entity, &mut Tilemap)>,
    mut camera_entity: Single<&mut Transform, With<Camera>>,
    _mesh_assets: ResMut<Assets<Mesh>>,
    asset_server: Res<AssetServer>,
    input: Res<ButtonInput<KeyCode>>,
    // tiles: Query<(Entity, &Tile3d)>,
    mut history: ResMut<History>,
    mut commands: Commands,
) {

    if input.pressed(KeyCode::ControlLeft) && input.just_pressed(KeyCode::KeyE) {
        tilemap_entity.1.data.export();
    }

    if input.pressed(KeyCode::ControlLeft) && input.just_pressed(KeyCode::KeyZ) {
        if !input.pressed(KeyCode::ShiftLeft) {
            commands.entity(tilemap_entity.0).queue(History::undo());
        } else {
            commands.entity(tilemap_entity.0).queue(History::redo());
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ActivePlane {
    pub depth: i32,
    pub normal: IVec3,
}

impl ActivePlane {
    pub fn ray_intersection(&self, ray: Ray3d, ray_id: RayId) -> Option<HitData> {
        let origin = self.depth * self.normal;

        ray.intersect_plane(
            origin.as_vec3(),
            InfinitePlane3d {
                normal: Dir3::new(self.normal.as_vec3()).unwrap(),
            },
        )
        .map(|time_of_impact| {
            let point = ray.get_point(time_of_impact) - self.normal.as_vec3() * 0.01;

            let tile = point.floor().as_ivec3();
            HitData {
                camera: ray_id.camera,
                depth: time_of_impact,
                position: Some(ray.get_point(time_of_impact)),
                normal: Some(self.normal.as_vec3()),
                extra: TileHitData {
                    tile,
                    face: InfinitePlane(self.normal),
                }
                .into(),
            }
        })
    }
}

#[derive(Component, Default, Debug)]
enum SelectionState {
    #[default]
    None,
    Adding {
        active_plane: ActivePlane,
        diff: AddingDiff,
    },
    Removing {
        active_plane: ActivePlane,
        diff: RemovingDiff,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AddingDiff {
    tile: TileData,
    added: HashSet<IVec3>,
    inserted: HashMap<IVec3, TileData>,
}

#[typetag::serde]
impl Undo for AddingDiff {
    fn undo(self: Box<Self>, mut tilemap_world: EntityWorldMut) -> Box<dyn Redo>  {
        let tilemap = &mut *tilemap_world.get_mut::<Tilemap>().unwrap();

        for &pos in self.added.iter() {
            let _ = tilemap.remove_at(pos);
        }

        for (&pos, &tile) in self.inserted.iter() {
            let _ = tilemap.insert(pos, tile);
        }

        self
    }

    fn clone_dyn(&self) -> Box<dyn Undo>  {
        Box::new(self.clone())
    }
}

#[typetag::serde]
impl Redo for AddingDiff {
    fn redo(self: Box<Self>, mut tilemap_world: EntityWorldMut) -> Box<dyn Undo>  {
        let tilemap = &mut *tilemap_world.get_mut::<Tilemap>().unwrap();

        for &pos in self.added.iter() {
            tilemap.insert(pos, self.tile);
        }

        for (&pos, _) in self.inserted.iter() {
            tilemap.insert(pos, self.tile);
        }

        self
    }

    fn clone_dyn(&self) -> Box<dyn Redo>  {
        Box::new(self.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemovingDiff(HashMap<IVec3, TileData>);

#[typetag::serde]
impl Undo for RemovingDiff {
    fn undo(self: Box<Self>, mut tilemap_world: EntityWorldMut) -> Box<dyn Redo>  {
        let tilemap = &mut *tilemap_world.get_mut::<Tilemap>().unwrap();

        for (&pos, &tile) in self.0.iter() {
            tilemap.insert(pos, tile);
        }

        self
    }

    fn clone_dyn(&self) -> Box<dyn Undo>  {
        Box::new(self.clone())
    }
}

#[typetag::serde]
impl Redo for RemovingDiff {
    fn redo(self: Box<Self>, mut tilemap_world: EntityWorldMut) -> Box<dyn Undo>  {
        let tilemap = &mut *tilemap_world.get_mut::<Tilemap>().unwrap();

        for (&pos, _) in self.0.iter() {
            tilemap.remove_at(pos);
        }

        self
    }

    fn clone_dyn(&self) -> Box<dyn Redo>  {
        Box::new(self.clone())
    }
}



#[derive(Component)]
struct ActiveGrid;

fn on_drag_add_remove_tile(
    e: On<PointerMove>,
    tilemap: Single<(Entity, &mut Tilemap)>,
    mut selection_state: Query<&mut SelectionState>,
) {
    let PointerMove {
        entity: _,
        pointer: _,
        hit,
        delta: _,
    } = &*e;

    let tile_hit = *hit.extra_as::<TileHitData>().unwrap();
    let (tilemap_entity, mut tilemap) = tilemap.into_inner();

    let mut selection_state = selection_state.get_mut(hit.camera).unwrap();

    match &mut *selection_state {
        SelectionState::None => {}
        SelectionState::Adding {
            active_plane: _,
            diff,
        } => {
            let new_tile_pos = tile_hit.tile + tile_hit.face.new_tile_offset();

            if diff.added.contains(&new_tile_pos) { return; }

            let Entry::Vacant(entry) = diff.inserted.entry(new_tile_pos) else {
                return;
            };

            let old_tile = tilemap.insert(new_tile_pos, TileData {
                tile_type: TileType::Cube([None; 6]),
            });

            if let Some(old_tile) = old_tile {
                entry.insert(old_tile);
            } else {
                diff.added.insert(new_tile_pos);
            }
        }
        SelectionState::Removing {
            active_plane: _,
            diff,
        } => {
            if diff.0.contains_key(&tile_hit.tile) { return; }

            if let Some(tile) = tilemap.remove_at(tile_hit.tile) {
                diff.0.insert(tile_hit.tile, tile);
            }
        }
    }
}

fn on_press_add_remove(
    e: On<PointerPress>,
    tilemap: Single<(Entity, &mut Tilemap)>,
    mut commands: Commands,
    mut selection_state: Query<&mut SelectionState>,
) {
    let PointerPress {
        entity: _,
        pointer: _,
        button,
        hit,
        count: _,
    } = &*e;

    let tile_hit = *hit.extra_as::<TileHitData>().unwrap();
    let (tilemap_entity, mut tilemap) = tilemap.into_inner();

    let mut selection_state = selection_state.get_mut(hit.camera).unwrap();

    let active_plane = tile_hit.active_plane();

    match button {
        PointerButton::Primary => {
            let new_tile = tile_hit.tile + tile_hit.face.new_tile_offset();

            let tile_data = TileData { tile_type: TileType::Cube([None; 6])};

            let old_tile = tilemap.insert(new_tile, tile_data);

            let diff = if let Some(old_tile) = old_tile {
                AddingDiff {
                    tile: tile_data,
                    added: HashSet::new(),
                    inserted: HashMap::from([(new_tile, old_tile)])
                }
            } else {
                AddingDiff {
                    tile: tile_data,
                    added: HashSet::from([new_tile]),
                    inserted: HashMap::new(),
                }
            };

            *selection_state = SelectionState::Adding {
                active_plane,
                diff,
            };

            info!("Adding new tile and transition to Add: {:?}", active_plane);
        }
        PointerButton::Secondary => {
            let old_tile = tilemap.remove_at(tile_hit.tile)
                .map(|tile| (tile_hit.tile, tile));
            // commands
            //     .entity(tilemap_entity)
            //     .queue(Tilemap::remove_at(tile_hit.tile));

            *selection_state = SelectionState::Removing {
                active_plane,
                diff: RemovingDiff(old_tile.into_iter().collect()),
            };

            info!("Remove tile and transition to Remove: {:?}", active_plane);
        }
        PointerButton::Middle => {}
    }
}

fn grid_on_release(
    e: On<PointerRelease>,
    mut selection_state: Query<&mut SelectionState>,
    mut history: ResMut<History>,
) {
    let PointerRelease {
        entity: _,
        pointer: _,
        button: _,
        hit,
    } = &*e;

    let mut selection_state = selection_state.get_mut(hit.camera).unwrap();

    let previous_state = core::mem::replace(&mut *selection_state, SelectionState::None);

    match previous_state {
        SelectionState::None => {
            return;
        }
        SelectionState::Adding { active_plane: _, diff } => {
            history.queue(Box::new(diff));
        },
        SelectionState::Removing { active_plane: _, diff } => {
            history.queue(Box::new(diff));
        },
    }

    println!("Released mouse and going to None state");
}

fn setup_system(mut commands: Commands, mesh_data: Res<MeshData>, asset_server: Res<AssetServer>) {
    commands
        .spawn((
            // You need to spawn an entity with this component
            InfiniteGrid,
            // Optional component you can use to configure the grid
            InfiniteGridSettings::default(),
        ))
        .observe(observe_draw_hit_gizmos)
        .observe(on_press_add_remove)
        .observe(grid_on_release);

    commands
        .spawn((
            InfiniteGrid,
            ActiveGrid,
            InfiniteGridSettings {
                x_axis_color: Color::srgb(0.2, 0.2, 0.2),
                z_axis_color: Color::srgb(0.2, 0.2, 0.2),
                ..Default::default()
            },
            Visibility::Hidden,
        ))
        .observe(observe_draw_hit_gizmos)
        .observe(on_drag_add_remove_tile)
        .observe(grid_on_release);

    commands.spawn((
        Tilemap::default(),
        MeshMaterial3d(mesh_data.cube_material.clone()),
    ))
        .observe(observe_draw_hit_gizmos)
        .observe(on_press_add_remove);

    let camera_transform = Transform::from_xyz(-12.5, 5.0, 10.0)
        .with_rotation(Quat::from_rotation_x(-45.0f32.to_radians()));
    commands.spawn((
        Camera3d::default(),
        SelectionState::default(),
        SelectionGizmos::default(),
        // CameraMovement {
        //     translation: camera_transform.translation,
        //     rotation: camera_transform.rotation,
        // },
        camera_transform,
    ));

    commands.spawn((
        WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset("CharacterTest.glb"))),
        Transform::IDENTITY,
    ));

    // illuminace value determined from testing to match original textures
    commands.spawn((
        DirectionalLight {
            illuminance: 4500.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(
            EulerRot::YXZ,
            -30.0f32.to_radians(),
            -45.0f32.to_radians(),
            0.0,
        )),
    ));
}

pub fn intersect_plane_full(
    ray: &Ray3d,
    plane_origin: Vec3,
    plane: InfinitePlane3d,
) -> Option<f32> {
    let denominator = plane.normal.dot(*ray.direction);
    if ops::abs(denominator) <= f32::EPSILON {
        return None;
    }
    let distance = (plane_origin - ray.origin).dot(*plane.normal) / denominator;
    return Some(distance);
}

fn move_system(
    camera: Single<(&mut Transform, &Camera, &GlobalTransform)>,
    window: Single<&Window>,
    key_input: Res<ButtonInput<KeyCode>>,
    mouse_input: Res<ButtonInput<MouseButton>>,
    accumulated_mouse_motion: Res<AccumulatedMouseMotion>,
    accumulated_mouse_scroll: Res<AccumulatedMouseScroll>,
    _cursor_options: Single<&mut CursorOptions>,
    time: Res<Time>,
    _gizmos: Gizmos,
) {
    let (mut camera_transform, _camera, _camera_gb_transform) = camera.into_inner();

    let mut move_speed = 0.02;
    let mut rotate_speed = 2.0;
    let mut scroll_speed = 4.0;
    if key_input.pressed(KeyCode::ShiftLeft) {
        move_speed *= 2.0;
        rotate_speed *= 2.0;
        scroll_speed *= 2.0;
    }

    let forward = camera_transform
        .forward()
        .as_vec3()
        .with_y(0.0)
        .normalize_or_zero();
    let right = camera_transform
        .right()
        .as_vec3()
        .with_y(0.0)
        .normalize_or_zero();

    // let mut translation = Vec3::ZERO;
    // if key_input.pressed(KeyCode::KeyA) {
    //     translation -= right;
    // }
    // if key_input.pressed(KeyCode::KeyD) {
    //     translation += right;
    // }
    // if key_input.pressed(KeyCode::KeyW) {
    //     translation += forward;
    // }
    // if key_input.pressed(KeyCode::KeyS) {
    //     translation -= forward;
    // }

    // camera.translation += move_speed * time.delta_secs() * translation.normalize_or_zero();

    // if mouse_input.pressed(MouseButton::Middle) {
    //     camera_transform.translation += 0.01 * (accumulated_mouse_motion.delta.y * forward - accumulated_mouse_motion.delta.x * right);
    // }

    if key_input.pressed(KeyCode::Numpad4) {
        camera_transform.rotate_y(-rotate_speed * time.delta_secs());
    }
    if key_input.pressed(KeyCode::Numpad5) {
        camera_transform.rotate_y(rotate_speed * time.delta_secs());
    }

    // Zoom
    {
        let delta = accumulated_mouse_scroll.delta;
        let forward = camera_transform.forward();
        camera_transform.translation += forward * delta.y * scroll_speed;
        camera_transform.translation.y = camera_transform.translation.y.max(5.0);
    }

    if let Some(_cursor_position) = window.cursor_position() {
        // let previous = cursor_position - accumulated_mouse_motion.delta;
        // let previous_point = get_world_point(previous, camera, camera_gb_transform).unwrap();
        // let new_point = get_world_point(cursor_position, camera, camera_gb_transform).unwrap();

        // gizmos.circle(
        //     Isometry3d::new(
        //         new_point,
        //         Quat::from_rotation_arc(Vec3::Z, Vec3::Y),
        //     ),
        //     0.2,
        //     Color::WHITE,
        // );
        // gizmos.circle(
        //     Isometry3d::new(
        //         previous_point,
        //         Quat::from_rotation_arc(Vec3::Z, Vec3::Y),
        //     ),
        //     0.2,
        //     Color::WHITE,
        // );

        // let delta = new_point - previous_point;

        // dbg!(delta);

        // camera_transform.translation += (delta.y * forward - delta.x * right);
        //

        if mouse_input.pressed(MouseButton::Middle) {
            camera_transform.translation += move_speed
                * (accumulated_mouse_motion.delta.y * forward
                    - accumulated_mouse_motion.delta.x * right);
        }
    }

    // if mouse_input.just_pressed(MouseButton::Middle) {
    //     cursor_options.visible = false;
    //     cursor_options.grab_mode = bevy::window::CursorGrabMode::Locked;
    // }
    // if mouse_input.just_released(MouseButton::Middle) {
    //     cursor_options.visible = true;
    //     cursor_options.grab_mode = bevy::window::CursorGrabMode::None;
    // }
    // TODO: Rotate around center of screen

    if key_input.just_pressed(KeyCode::Numpad2) {
        camera_transform.rotation = Quat::from_rotation_x(-45.0f32.to_radians());
    } else if key_input.just_pressed(KeyCode::Numpad1) {
        camera_transform.rotation = Quat::from_euler(
            EulerRot::YXZ,
            45.0f32.to_radians(),
            -45.0f32.to_radians(),
            0.0,
        );
    } else if key_input.just_pressed(KeyCode::Numpad3) {
        camera_transform.rotation = Quat::from_euler(
            EulerRot::YXZ,
            -45.0f32.to_radians(),
            -45.0f32.to_radians(),
            0.0,
        );
    }
}

#[derive(Resource, Debug, Clone)]
pub struct MeshData {
    pub cube_mesh: Handle<Mesh>,
    pub cube_material: Handle<StandardMaterial>,
}

impl FromWorld for MeshData {
    fn from_world(world: &mut World) -> Self {
        let cube_mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::new(1.0, 1.0, 1.0));
        let asset_server = world.resource_mut::<AssetServer>();
        let image = asset_server.load::<Image>("atlas.png");
        let mut material = StandardMaterial::from(image);

        material.perceptual_roughness = 1.0;
        let cube_material = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(material);

        Self {
            cube_mesh,
            cube_material,
        }
    }
}

fn update_grid(
    mut active_grid: Single<(&mut Transform, &mut Visibility), With<ActiveGrid>>,
    selection_state: Single<&SelectionState>,
) {
    let active_plane = match &*selection_state {
        SelectionState::None => None,
        SelectionState::Adding { active_plane, .. } => Some(*active_plane),
        SelectionState::Removing { active_plane, .. } => Some(*active_plane),
    };

    match active_plane {
        None => {
            *active_grid.1 = Visibility::Hidden;
        }
        Some(ActivePlane { depth, normal }) => {
            *active_grid.1 = Visibility::Inherited;
            *active_grid.0 = Transform::from_translation((depth * normal).as_vec3())
                .looking_to(Vec3::Y, normal.as_vec3());
            if (Vec3::Y - normal.as_vec3()).norm_squared() < 1e-3 {
                active_grid.0.rotation = Quat::IDENTITY;
            }
        }
    }
}

fn regenerate_dirty_tilemaps(
    mut tilemaps: Query<(&mut Tilemap, &mut Mesh3d)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for (mut tilemap, mut mesh_3d) in tilemaps.iter_mut() {
        if !tilemap.dirty {
            continue;
        }

        let mesh = tilemap.data.create_mesh();
        let mesh = meshes.add(mesh);
        mesh_3d.0 = mesh;

        tilemap.dirty = false;
    }
}
