//! A simple example to show how to spawn an infinite grid.
//!
//! Infinite grids are useful as the ground plane in editor-like applications,
//! as they provide a consistent reference for the orientation of objects
//! and an evenly spaced grid to judge relative scale.

mod scene;
mod tilemap;

use std::sync::Arc;

use bevy::{
    camera::{RenderTarget, Viewport, visibility::RenderLayers}, color::palettes, dev_tools::infinite_grid::{InfiniteGrid, InfiniteGridPlugin, InfiniteGridSettings}, feathers::{FeathersPlugins, controls::FeathersButton, dark_theme::create_dark_theme, display::caption, theme::UiTheme}, input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll}, math::NormedVectorSpace, picking::backend::{
        HitData, HitDataExtra, PointerHits,
        ray::{RayId, RayMap},
    }, platform::collections::{HashMap, HashSet, hash_map::Entry}, prelude::*, ui::{debug::print_ui_layout_tree, ui_surface::UiSurface}, ui_widgets::{Activate, Button, ButtonPlugin}, window::{CursorIcon, CursorOptions},
};
use serde::{Deserialize, Serialize};
use tiny_bail::{or_continue, or_continue_quiet};
use wgpu_types::TextureFormat;
use bevy_inspector_egui::{quick::WorldInspectorPlugin};
use bevy_egui::EguiPlugin;

use crate::{
    TileFace::InfinitePlane, scene::{History, NewScene, Redo, ScenePlugin, Undo}, tilemap::{FaceData, TileData, TileType, Tilemap, tilemap_coord_to_center},
};

const VIEWPORT_RENDER_LAYER: RenderLayers = RenderLayers::layer(0);
const TILESET_RENDER_LAYER: RenderLayers = RenderLayers::layer(1);

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(ImagePlugin::default_nearest()),
            InfiniteGridPlugin,
            ScenePlugin,
            EguiPlugin::default(),
            WorldInspectorPlugin::new(),
            FeathersPlugins,
            // FpsOverlayPlugin {
            //     ..Default::default()
            // },
            //
        ))
        .add_systems(Startup, setup_system)
        .add_systems(
            Update,
            (
                mouse_move_system,
                // place_system,
                update_grid,
                keyboard_funcs,
                regenerate_dirty_tilemaps,
                update_hits,
                draw_selection_gizmos,
            ),
        )
        .insert_resource(UiTheme(create_dark_theme()))
        .init_resource::<MeshData>()
        .init_resource::<History>()
        .insert_gizmo_config(SelectionGizmosGroup, GizmoConfig {
            render_layers: VIEWPORT_RENDER_LAYER,
            ..default()
        })
        .run();
}

// We can create our own gizmo config group!
#[derive(Default, Reflect, GizmoConfigGroup)]
struct SelectionGizmosGroup;

fn observe_draw_hit_gizmos(event: On<PointerMove>, mut gizmos: Query<&mut SelectionGizmos>) {
    let PointerMove {
        entity: _,
        pointer: _,
        delta: _,
        hit,
    } = event.event();

    let tile_hit = *hit.extra_as::<TileHitData>().unwrap();

    let mut gizmos = gizmos.get_mut(hit.camera).unwrap();

    *gizmos = SelectionGizmos::AddTerrain {
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
enum SelectionGizmos {
    #[default]
    None,
    AddTerrain {
        circle_iso: Isometry3d,
        tile_pos: IVec3,
        new_tile_pos: IVec3,
    },
    PaintTile {
        tile_pos: IVec3,
        face: TileFace,
    },
}

fn draw_selection_gizmos(mut gizmos: Gizmos<SelectionGizmosGroup>, selection_gizmos: Query<&SelectionGizmos>) {
    for selection in selection_gizmos.iter() {
        match *selection {
            SelectionGizmos::None => {}
            SelectionGizmos::AddTerrain { circle_iso, tile_pos, new_tile_pos } => {
                gizmos.circle(circle_iso, 0.125, Color::WHITE);

                gizmos.cube(
                    Transform::from_translation(tilemap_coord_to_center(tile_pos)),
                    Color::linear_rgb(1.0, 0.0, 0.0),
                );

                gizmos.cube(
                    Transform::from_translation(tilemap_coord_to_center(new_tile_pos)),
                    Color::WHITE,
                );
            }
            SelectionGizmos::PaintTile { tile_pos, face } => {
                let (plane, position) = face.to_primitive(tile_pos);
                gizmos
                    .primitive_3d(
                        &plane,
                        Isometry3d::from_translation(position),
                        palettes::css::GREEN,
                    );
            },
        }
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

    fn to_primitive(&self, tile: IVec3) -> (Plane3d, Vec3) {
        match *self {
            TileFace::Cube(cube_tile_face) => {
                let normal = cube_tile_face.new_tile_offset().as_vec3();
                let tile_position = tilemap_coord_to_center(tile) + normal * 0.5;
                (
                    Plane3d {
                        normal: Dir3::new_unchecked(normal),
                        half_size: Vec2::splat(0.5),
                    },
                    tile_position,
                )
            },
            InfinitePlane(ivec3) => {
                todo!()
            },
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
    // NOTE: must be unit vector
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
        let (camera, selection_state) = or_continue_quiet!(camera.get(ray_id.camera));

        let pick = match &*selection_state {
            SelectionState::None => {
                if let Some(hit) = tilemap.ray_intersection(ray) {
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
            SelectionState::Adding { active_plane, .. }
            | SelectionState::Removing { active_plane, .. } => active_plane
                .ray_intersection(ray, ray_id)
                .map(|hit| (*active_grid, hit)),
        };

        output_messages.write(PointerHits {
            pointer: ray_id.pointer,
            picks: Vec::from(pick.as_slice()),
            order: camera.order as f32,
        });
    }
}

fn keyboard_funcs(
    tilemap_entity: Single<(Entity, &Tilemap)>,
    input: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    ui_surface: Res<UiSurface>,
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

    if input.just_pressed(KeyCode::KeyP) {
        print_ui_layout_tree(&*&ui_surface);
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ActivePlane {
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

#[derive(Resource, Debug, Serialize, Deserialize)]
pub struct TileSelections {
    pub cube: [Option<FaceData>; 6],
    pub tile: u32,
    pub tool: ToolSelection,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TileSelection {
    One(u32),
    Two([u32; 2])
}

impl Default for TileSelections {
    fn default() -> Self {
        Self {
            cube: [Some(FaceData { tile_idx: 0 }); 6],
            tile: 0,
            tool: ToolSelection::Cube,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolSelection {
    Cube,
    TilePaint,
}

#[derive(Component, Default, Debug)]
pub enum SelectionState {
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
pub struct AddingDiff {
    tile: TileData,
    added: HashSet<IVec3>,
    inserted: HashMap<IVec3, TileData>,
}

#[typetag::serde]
impl Undo for AddingDiff {
    fn undo(self: Box<Self>, mut tilemap_world: EntityWorldMut) -> Box<dyn Redo> {
        let tilemap = &mut *tilemap_world.get_mut::<Tilemap>().unwrap();

        for &pos in self.added.iter() {
            let _ = tilemap.remove_at(pos);
        }

        for (&pos, &tile) in self.inserted.iter() {
            let _ = tilemap.insert(pos, tile);
        }

        self
    }

    fn clone_dyn(&self) -> Box<dyn Undo> {
        Box::new(self.clone())
    }
}

#[typetag::serde]
impl Redo for AddingDiff {
    fn redo(self: Box<Self>, mut tilemap_world: EntityWorldMut) -> Box<dyn Undo> {
        let tilemap = &mut *tilemap_world.get_mut::<Tilemap>().unwrap();

        for &pos in self.added.iter() {
            tilemap.insert(pos, self.tile);
        }

        for (&pos, _) in self.inserted.iter() {
            tilemap.insert(pos, self.tile);
        }

        self
    }

    fn clone_dyn(&self) -> Box<dyn Redo> {
        Box::new(self.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemovingDiff(HashMap<IVec3, TileData>);

#[typetag::serde]
impl Undo for RemovingDiff {
    fn undo(self: Box<Self>, mut tilemap_world: EntityWorldMut) -> Box<dyn Redo> {
        let tilemap = &mut *tilemap_world.get_mut::<Tilemap>().unwrap();

        for (&pos, &tile) in self.0.iter() {
            tilemap.insert(pos, tile);
        }

        self
    }

    fn clone_dyn(&self) -> Box<dyn Undo> {
        Box::new(self.clone())
    }
}

#[typetag::serde]
impl Redo for RemovingDiff {
    fn redo(self: Box<Self>, mut tilemap_world: EntityWorldMut) -> Box<dyn Undo> {
        let tilemap = &mut *tilemap_world.get_mut::<Tilemap>().unwrap();

        for (&pos, _) in self.0.iter() {
            tilemap.remove_at(pos);
        }

        self
    }

    fn clone_dyn(&self) -> Box<dyn Redo> {
        Box::new(self.clone())
    }
}

#[derive(Component)]
struct ActiveGrid;

fn on_drag_add_remove_tile(
    e: On<PointerMove>,
    mut tilemap: Single<&mut Tilemap>,
    mut selection_state: Query<&mut SelectionState>,
) {
    let PointerMove {
        entity: _,
        pointer: _,
        hit,
        delta: _,
    } = &*e;

    let tile_hit = *hit.extra_as::<TileHitData>().unwrap();

    let mut selection_state = selection_state.get_mut(hit.camera).unwrap();

    match &mut *selection_state {
        SelectionState::None => {}
        SelectionState::Adding {
            active_plane: _,
            diff,
        } => {
            let new_tile_pos = tile_hit.tile + tile_hit.face.new_tile_offset();

            if diff.added.contains(&new_tile_pos) {
                return;
            }

            let Entry::Vacant(entry) = diff.inserted.entry(new_tile_pos) else {
                return;
            };

            let old_tile = tilemap.insert(
                new_tile_pos,
                TileData {
                    tile_type: TileType::default_cube(),
                },
            );

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
            if diff.0.contains_key(&tile_hit.tile) {
                return;
            }

            if let Some(tile) = tilemap.remove_at(tile_hit.tile) {
                diff.0.insert(tile_hit.tile, tile);
            }
        }
    }
}

fn on_press_add_remove(
    e: On<PointerPress>,
    tilemap: Single<(Entity, &mut Tilemap)>,
    _commands: Commands,
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
    let (_tilemap_entity, mut tilemap) = tilemap.into_inner();

    let mut selection_state = selection_state.get_mut(hit.camera).unwrap();

    let active_plane = tile_hit.active_plane();

    match button {
        PointerButton::Primary => {
            let new_tile = tile_hit.tile + tile_hit.face.new_tile_offset();

            let tile_data = TileData {
                tile_type: TileType::default_cube(),
            };

            let old_tile = tilemap.insert(new_tile, tile_data);

            let diff = if let Some(old_tile) = old_tile {
                AddingDiff {
                    tile: tile_data,
                    added: HashSet::new(),
                    inserted: HashMap::from([(new_tile, old_tile)]),
                }
            } else {
                AddingDiff {
                    tile: tile_data,
                    added: HashSet::from([new_tile]),
                    inserted: HashMap::new(),
                }
            };

            *selection_state = SelectionState::Adding { active_plane, diff };

            info!("Adding new tile and transition to Add: {:?}", active_plane);
        }
        PointerButton::Secondary => {
            let old_tile = tilemap
                .remove_at(tile_hit.tile)
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
        SelectionState::Adding {
            active_plane: _,
            diff,
        } => {
            history.queue(Box::new(diff));
        }
        SelectionState::Removing {
            active_plane: _,
            diff,
        } => {
            history.queue(Box::new(diff));
        }
    }

    println!("Released mouse and going to None state");
}

#[derive(Component, Debug, Clone, Copy)]
pub struct TilemapCamera;

fn setup_system(mut commands: Commands, mesh_data: Res<MeshData>, asset_server: Res<AssetServer>, window: Single<Entity, With<Window>>, mut images: ResMut<Assets<Image>>) {


    // UI Camera
    commands.spawn(Camera3d::default());

    commands
        .spawn((
            // You need to spawn an entity with this component
            InfiniteGrid,
            // Optional component you can use to configure the grid
            InfiniteGridSettings::default(),
            VIEWPORT_RENDER_LAYER,
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
            VIEWPORT_RENDER_LAYER,
        ))
        .observe(observe_draw_hit_gizmos)
        .observe(on_drag_add_remove_tile)
        .observe(grid_on_release);

    commands
        .spawn((
            Tilemap::default(),
            MeshMaterial3d(mesh_data.cube_material.clone()),
            VIEWPORT_RENDER_LAYER,
        ))
        .observe(observe_draw_hit_gizmos)
        .observe(on_press_add_remove);

    let render_image_3d = Image::new_target_texture(0, 0, TextureFormat::Rgba8UnormSrgb, None);
    let render_image_3d_handle = images.add(render_image_3d);

    let camera_transform = Transform::from_xyz(0.0, 15.0, 15.0)
        .with_rotation(Quat::from_rotation_x(-45.0f32.to_radians()));
    let camera_3d = commands.spawn((
        Camera3d::default(),
        Camera {
            order: -1,
            ..Default::default()
        },
        RenderTarget::Image(render_image_3d_handle.clone().into()),
        VIEWPORT_RENDER_LAYER,
        SelectionState::default(),
        SelectionGizmos::default(),
        TilemapCamera,
        // CameraMovement {
        //     translation: camera_transform.translation,
        //     rotation: camera_transform.rotation,
        // },
        camera_transform,
    ))
    .id();

    commands.insert_resource(GlobalAmbientLight {
        brightness: 80.0,
        ..default()
    });

    // illuminace value determined from testing to match original textures
    commands.spawn((
        DirectionalLight {
            illuminance: 5000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(
            EulerRot::YXZ,
            -30.0f32.to_radians(),
            -45.0f32.to_radians(),
            0.0,
        )),
        VIEWPORT_RENDER_LAYER,
    ));

    commands.entity(*window)
        .insert(CursorIcon::default());

    let viewport_node = ViewportNode::new(camera_3d);


    // commands.spawn((
    //     Sprite::from_image(asset_server.load("atlas.png")),
    //     TILESET_RENDER_LAYER,
    // ));

    // let render_image_2d = Image::new_target_texture(0, 0, TextureFormat::Rgba8UnormSrgb, None);
    // let render_image_2d_handle = images.add(render_image_2d);

    // let tile_camera = commands.spawn((
    //     Camera2d,
    //     Camera {
    //         order: -1,
    //         ..Default::default()
    //     },
    //     RenderTarget::Image(render_image_2d_handle.into()),
    //     TILESET_RENDER_LAYER,
    // ))
    //     .observe(|e: On<PointerMove>| {
    //         dbg!(e.hit.position);
    //     })
    //     .id();

    // let tile_node = ViewportNode::new(tile_camera);

    fn nav_button(icon: &str) -> impl Scene {
        fn hover_border(
            e: On<PointerEnter>,
            mut colors: Query<(&mut BorderColor, &mut BackgroundColor)>,
        ) {
            let (mut border, mut bg_color) = colors.get_mut(e.entity).unwrap();

            *border = BorderColor::all(Color::srgb_u8(137, 137, 137));
            *bg_color = BackgroundColor(Color::srgba_u8(137, 137, 137, 128));
        }

        fn exit_border(
            e: On<PointerLeave>,
            mut colors: Query<(&mut BorderColor, &mut BackgroundColor)>,
        ) {
            let (mut border, mut bg_color) = colors.get_mut(e.entity).unwrap();

            *border = BorderColor::default();
            *bg_color = BackgroundColor::default();
        }

        let icon = icon.to_owned();
        bsn! {
            Button
            Node {
                height: px(32),
                width: px(32),
                padding: px(4),
                margin: px(2),
                border: px(2),
                box_sizing: BoxSizing::ContentBox,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
            }
            UiDebugOptions {
                enabled: true,
                // outline_border_box: true,
                outline_content_box: true,
                // outline_padding_box: true,
            }
            BorderColor
            // Outline {
            //     width: px(2),
            //     color: Color::srgb_u8(137, 137, 137),
            // }
            BackgroundColor
            Children [
                Node {
                    height: px(32),
                    width: px(32),
                }
                ImageNode {
                    image: icon,
                }
            ]
            on(hover_border)
            on(exit_border)
        }
    }

    #[derive(Component, Debug, Clone, FromTemplate)]
    struct TileHighlight {
        pub selected_outline: Entity,
        pub hovered_outline: Entity,
    }

    commands.queue_spawn_scene(bsn! {
        #Root
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            right: Val::Px(0.0),
            bottom: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
        }
        Children [
            #Header
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(48.0),
                padding: px(4.0),

                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Start,
                // column_gap: Val::Px(16.0),
            }
            BackgroundColor(Color::srgb_u8(76, 70, 66))
            Children [
                // @FeathersButton {
                //     @caption: bsn! { @caption("New") }
                // }
                // on(|_: On<Activate>, mut commands: Commands| {
                //     commands.trigger(NewScene);
                // })
                // --
                // @FeathersButton {
                //     @caption: bsn! { @caption("Open") }
                // }
                // on(|_: On<Activate>| {
                //     println!("Open");
                // })
                // --
                // @nav_button("icons/save.png")
                // --
                // @nav_button("icons/undo.png")
                // --
                // @nav_button("icons/redo.png")
            ]
            --
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Row,
            }
            Children [
                viewport_node
                Node {
                    height: Val::Percent(100.0),
                    flex_grow: 1.0,
                }
                --
                // tile_node
                Node {
                    height: Val::Percent(100.0),
                    max_width: percent(50.0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                }
                BackgroundColor(Color::WHITE)
                Children [
                    BackgroundColor(Color::srgb_u8(76, 70, 66))
                    Node {
                        width: percent(100.0),
                        height: px(440),
                    }
                    ImageNode {
                        image: "atlas.png"
                    }
                    TileHighlight {
                        selected_outline: #TileActiveOutline,
                        hovered_outline: #TileOutline,
                    }
                    on(|e: On<PointerMove>, tile: Query<&TileHighlight>, mut node: Query<&mut Node>| {
                        let highlight = tile.get(e.entity).unwrap();
                        dbg!(e.hit.camera);
                        let mut node = node.get_mut(highlight.hovered_outline).unwrap();

                        let uv = e.hit.position.unwrap().truncate() + Vec2::splat(0.5);
                        let uv_i = (uv * 10.0).as_ivec2();

                        node.top = percent((uv_i.y as f32) * 10.0);
                        node.left = percent((uv_i.x as f32) * 10.0);
                    })
                    on(|e: On<PointerPress>, tile: Query<&TileHighlight>, mut node: Query<&mut Node>| {
                        let highlight = tile.get(e.entity).unwrap();
                        dbg!(e.hit.camera);
                        let mut node = node.get_mut(highlight.hovered_outline).unwrap();

                        let uv = e.hit.position.unwrap().truncate() + Vec2::splat(0.5);
                        let uv_i = (uv * 10.0).as_ivec2();

                        node.top = percent((uv_i.y as f32) * 10.0);
                        node.left = percent((uv_i.x as f32) * 10.0);
                    })
                    Children [
                        #TileOutline
                        Node {
                            position_type: PositionType::Absolute,
                            width: percent(10.0),
                            height: percent(10.0),
                        }
                        Outline {
                            width: px(2),
                            color: Color::srgb_u8(255, 0, 0),
                        }
                        Pickable::IGNORE
                        --
                        #TileActiveOutline
                        Node {
                            position_type: PositionType::Absolute,
                            width: percent(10.0),
                            height: percent(10.0),
                        }
                        Outline {
                            width: px(2),
                            color: Color::srgb_u8(255, 0, 0),
                        }
                        Pickable::IGNORE
                    ]
                ]
            ]
            --
        ]
    });
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

// fn keyboard_move_system

fn mouse_move_system(
    camera: Single<(&mut Transform, &Camera, &GlobalTransform), With<SelectionState>>,
    window: Single<&Window>,
    key_input: Res<ButtonInput<KeyCode>>,
    mouse_input: Res<ButtonInput<MouseButton>>,
    accumulated_mouse_motion: Res<AccumulatedMouseMotion>,
    accumulated_mouse_scroll: Res<AccumulatedMouseScroll>,
    mut cursor_options: Single<&mut CursorIcon>,
    time: Res<Time>,
    _gizmos: Gizmos,
) {
    let (mut camera_transform, _camera, _camera_gb_transform) = camera.into_inner();

    if !key_input.pressed(KeyCode::Space) {
        // **cursor_options = CursorIcon::System(bevy::window::SystemCursorIcon::Default);
        return;
    }
    // **cursor_options = CursorIcon::System(bevy::window::SystemCursorIcon::AllScroll);

    // cursor_options.grab_mode = bevy::window::CursorGrabMode::None;

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

    let mut translation = Vec3::ZERO;
    if key_input.pressed(KeyCode::KeyA) {
        translation -= right;
    }
    if key_input.pressed(KeyCode::KeyD) {
        translation += right;
    }
    if key_input.pressed(KeyCode::KeyW) {
        translation += forward;
    }
    if key_input.pressed(KeyCode::KeyS) {
        translation -= forward;
    }

    camera_transform.translation += 20.0 * time.delta_secs() * translation.normalize_or_zero();

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
        let mut delta = accumulated_mouse_scroll.delta;
        if key_input.pressed(KeyCode::Equal) {
            delta.y += 10.0 * time.delta_secs();
        }
        if key_input.pressed(KeyCode::Minus) {
            delta.y -= 10.0 * time.delta_secs();
        }

        let forward = camera_transform.forward();
        camera_transform.translation += forward * delta.y * scroll_speed;
        camera_transform.translation.y = camera_transform.translation.y.max(5.0);
    }

    if mouse_input.pressed(MouseButton::Middle) {
        camera_transform.translation += move_speed
            * (accumulated_mouse_motion.delta.y * forward
                - accumulated_mouse_motion.delta.x * right);
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

    let mut rotation = 0.0;
    if key_input.pressed(KeyCode::KeyQ) {
        rotation += 90.0f32.to_radians() * time.delta_secs();
    }
    if key_input.pressed(KeyCode::KeyE) {
        rotation -= 90.0f32.to_radians() * time.delta_secs();
    }
    camera_transform.rotate_y(rotation);


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
