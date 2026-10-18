//! A simple example to show how to spawn an infinite grid.
//!
//! Infinite grids are useful as the ground plane in editor-like applications,
//! as they provide a consistent reference for the orientation of objects
//! and an evenly spaced grid to judge relative scale.

mod scene;
mod tilemap;

use bevy::{
    dev_tools::infinite_grid::{InfiniteGrid, InfiniteGridPlugin, InfiniteGridSettings},
    ecs::{lifecycle::HookContext, world::DeferredWorld},
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    math::NormedVectorSpace,
    picking::backend::{HitData, PointerHits, ray::RayMap},
    prelude::*,
    window::CursorOptions,
};
use tiny_bail::or_continue;

use crate::{
    scene::Scene,
    tilemap::{Tilemap, TilemapImage, TilemapRayHit, tilemap_coord_to_center},
};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(ImagePlugin::default_nearest()),
            InfiniteGridPlugin,
            // FpsOverlayPlugin {
            //     ..Default::default()
            // },
        ))
        .add_systems(Startup, setup_system)
        .add_systems(
            Update,
            (
                move_system,
                place_system,
                update_grid,
                load_tilemap,
                regenerate_dirty_tilemaps,
            ),
        )
        .init_resource::<MeshData>()
        .run();
}

fn update_hits(
    ray_map: Res<RayMap>,
    mut output_messages: MessageWriter<PointerHits>,
    mut camera: Query<(&Camera, &mut SelectionState)>,
    tilemap: Single<(Entity, &Tilemap)>,
) {
    let (tilemap_entity, tilemap) = tilemap.into_inner();
    for (&ray_id, &ray) in ray_map.iter() {
        let (camera, selection_state) = or_continue!(camera.get(ray_id.camera));

        let pick = match selection_state.active_plane.as_ref() {
            None => {
                if let Some(hit) = tilemap.ray_intersection(ray)
                    && let Some(e) = hit.entity
                {
                    (
                        e,
                        HitData {
                            camera: ray_id.camera,
                            depth: hit.time_of_impact,
                            position: Some(ray.get_point(hit.time_of_impact)),
                            normal: Some(hit.hit_normal),
                            extra: None,
                        },
                    )
                } else {
                    todo!()
                }
            }
            Some(_) => todo!(),
        };

        output_messages.write(PointerHits {
            pointer: ray_id.pointer,
            picks: vec![pick],
            order: camera.order as f32,
        });
    }
}

fn load_tilemap(
    mut tilemap_entity: Single<(Entity, &mut Tilemap)>,
    mut camera_entity: Single<&mut Transform, With<Camera>>,
    _mesh_assets: ResMut<Assets<Mesh>>,
    asset_server: Res<AssetServer>,
    input: Res<ButtonInput<KeyCode>>,
    tiles: Query<(Entity, &Tile3d)>,
    mut commands: Commands,
) {
    if input.just_pressed(KeyCode::KeyP) {
        println!("{} tiles", tiles.iter().count());
    }
    if input.pressed(KeyCode::ControlLeft) && input.just_pressed(KeyCode::KeyL) {
        let mut scene = Scene::load().unwrap();
        scene.tilemap_data.image = Some(TilemapImage {
            path: String::from("atlas.png"),
            image_width: 160,
            image_height: 110,
            tile_width: 16,
            tile_height: 11,
        });

        for (e, _tile) in tiles.iter() {
            commands.entity(e).despawn();
        }

        let img = match &scene.tilemap_data.image {
            Some(img) => asset_server.load::<Image>(&img.path),
            None => asset_server.load("PrototypeTextures/texture_08.png"),
        };

        let mat = asset_server.add(StandardMaterial::from(img));

        let mut tilemap_commands = commands.entity(tilemap_entity.0);

        tilemap_commands.insert(MeshMaterial3d(mat));

        tilemap_entity.1.data.image = scene.tilemap_data.image;

        for (tile, tile_data) in scene.tilemap_data.tiles.iter() {
            match tile_data.tile_type {
                tilemap::TileType::Cube(_) => {
                    tilemap_commands.queue(Tilemap::add_cube(*tile));
                }
            }
        }

        **camera_entity = Transform::from_isometry(scene.camera);
    }
    if input.pressed(KeyCode::ControlLeft) && input.just_pressed(KeyCode::KeyS) {
        let tilemap_data = tilemap_entity.1.data.clone();
        let camera = camera_entity.to_isometry();
        let scene = Scene {
            tilemap_data,
            camera,
        };
        let result = scene.save();
        if let Err(err) = result {
            eprintln!("{err}");
            eprintln!("{}", err.backtrace());
        }
    }
}

#[derive(Component, Default)]
struct SelectionState {
    active_plane: Option<(Vec3, Dir3)>,
}

#[derive(Component)]
struct ActiveGrid;

fn setup_system(mut commands: Commands, mesh_data: Res<MeshData>, asset_server: Res<AssetServer>) {
    commands.spawn((
        // You need to spawn an entity with this component
        InfiniteGrid,
        // Optional component you can use to configure the grid
        InfiniteGridSettings::default(),
    ));

    commands.spawn((
        InfiniteGrid,
        ActiveGrid,
        InfiniteGridSettings {
            x_axis_color: Color::srgb(0.2, 0.2, 0.2),
            z_axis_color: Color::srgb(0.2, 0.2, 0.2),
            ..Default::default()
        },
        Visibility::Hidden,
    ));

    commands.spawn((
        Tilemap::default(),
        MeshMaterial3d(mesh_data.cube_material.clone()),
    ));

    let camera_transform = Transform::from_xyz(-12.5, 5.0, 10.0)
        .with_rotation(Quat::from_rotation_x(-45.0f32.to_radians()));
    commands.spawn((
        Camera3d::default(),
        SelectionState::default(),
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
    match selection_state.active_plane.as_ref() {
        None => {
            *active_grid.1 = Visibility::Hidden;
            *active_grid.0 = Transform::from_translation(Vec3::Y * -100000.0);
        }
        Some(&(origin, normal)) => {
            *active_grid.1 = Visibility::Inherited;
            *active_grid.0 =
                Transform::from_translation(origin.floor()).looking_to(Vec3::Y, normal);
            if (Vec3::Y - normal.as_vec3()).norm_squared() < 1e-3 {
                active_grid.0.rotation = Quat::IDENTITY;
            }
        }
    }
}

fn place_system(
    tilemap: Single<(Entity, &Tilemap)>,
    camera: Single<(&Camera, &GlobalTransform, &mut SelectionState)>,
    window: Single<&Window>,
    mouse_input: Res<ButtonInput<MouseButton>>,
    key_input: Res<ButtonInput<KeyCode>>,
    _mesh_data: Res<MeshData>,
    _mesh_assets: ResMut<Assets<Mesh>>,
    mut gizmos: Gizmos,
    mut commands: Commands,
) {
    let (camera, transform, mut selection_state) = camera.into_inner();
    let (tilemap_entity, tilemap) = tilemap.into_inner();

    if let Some(cursor_position) = window.cursor_position() {
        let ray = camera
            .viewport_to_world(transform, cursor_position)
            .unwrap();

        match selection_state.active_plane.as_ref() {
            None => {
                let ray_hit = {
                    if let Some(hit) = tilemap.ray_intersection(ray) {
                        hit
                    } else {
                        let time_of_impact = ray
                            .intersect_plane(Vec3::ZERO, InfinitePlane3d { normal: Dir3::Y })
                            .unwrap();
                        let tile = (ray.get_point(time_of_impact) + Dir3::Y * 0.01)
                            .floor()
                            .as_ivec3();
                        TilemapRayHit {
                            time_of_impact,
                            hit_normal: Vec3::Y,
                            tile,
                            entity: None,
                        }
                    }
                };

                gizmos.circle(
                    Isometry3d::new(
                        ray.get_point(ray_hit.time_of_impact),
                        Transform::default()
                            .looking_to(ray_hit.hit_normal, Vec3::Y)
                            .rotation,
                    ),
                    0.125,
                    Color::WHITE,
                );

                gizmos.cube(
                    Transform::from_translation(tilemap_coord_to_center(ray_hit.tile)),
                    Color::WHITE,
                );

                if mouse_input.any_pressed([MouseButton::Left, MouseButton::Right]) {
                    selection_state.active_plane = Some((
                        ray.get_point(ray_hit.time_of_impact),
                        Dir3::new(ray_hit.hit_normal).unwrap(),
                    ));
                }
            }
            Some(&(origin, normal)) => {
                if !mouse_input.pressed(MouseButton::Left)
                    && !mouse_input.pressed(MouseButton::Right)
                {
                    selection_state.active_plane = None;
                } else {
                    let ray_hit = {
                        let time_of_impact = tiny_bail::or_return!(
                            ray.intersect_plane(origin, InfinitePlane3d { normal })
                        );
                        let mut point = ray.get_point(time_of_impact) + normal * 0.01;
                        if !mouse_input.pressed(MouseButton::Left) {
                            point -= *normal;
                        }

                        let tile = point.floor().as_ivec3();
                        TilemapRayHit {
                            time_of_impact,
                            hit_normal: Vec3::Y,
                            tile,
                            entity: None,
                        }
                    };

                    gizmos.circle(
                        Isometry3d::new(
                            ray.get_point(ray_hit.time_of_impact),
                            Transform::default()
                                .looking_to(ray_hit.hit_normal, Vec3::Y)
                                .rotation,
                        ),
                        0.125,
                        Color::WHITE,
                    );

                    gizmos.cube(
                        Transform::from_translation(tilemap_coord_to_center(ray_hit.tile)),
                        Color::WHITE,
                    );

                    if mouse_input.pressed(MouseButton::Left) {
                        commands
                            .entity(tilemap_entity)
                            .queue(Tilemap::add_cube(ray_hit.tile));
                    } else {
                        commands
                            .entity(tilemap_entity)
                            .queue(Tilemap::remove_at(ray_hit.tile));
                    }
                }
            }
        }
    }

    if key_input.pressed(KeyCode::ControlLeft) && key_input.just_pressed(KeyCode::KeyE) {
        tilemap.data.export();
    }
}

fn tile_3d_picking_backend(
    ray_map: Res<RayMap>,
    camera: Query<&Camera>,
    tilemaps: Query<&Tilemap>,
    mut pointer_hits: MessageWriter<PointerHits>,
) {
    for (&ray_id, &ray) in ray_map.iter() {
        let Ok(camera) = camera.get(ray_id.camera) else {
            continue;
        };

        let mut picks = Vec::new();

        for tilemap in tilemaps.iter() {
            let Some(hit) = tilemap.collision.ray_intersection(ray) else {
                continue;
            };

            let entity = hit.entity.expect("hit plane");

            picks.push((
                entity,
                HitData {
                    camera: ray_id.camera,
                    depth: hit.time_of_impact,
                    position: Some(ray.get_point(hit.time_of_impact)),
                    normal: Some(hit.hit_normal),
                    extra: None,
                },
            ))
        }

        pointer_hits.write(PointerHits::new(ray_id.pointer, picks, camera.order as f32));
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

#[derive(Component, Clone, Debug)]
#[component(immutable, on_insert, on_discard)]
pub struct Tile3d {
    pub position: IVec3,
    pub tile_kind: TileKind,
}

#[derive(Clone, Debug)]
pub enum TileKind {
    Cube,
}

impl Tile3d {
    fn on_insert(mut world: DeferredWorld, hook_context: HookContext) {
        let tile = world.get::<Tile3d>(hook_context.entity).unwrap().clone();
        let parent = world.get::<ChildOf>(hook_context.entity).unwrap().0;
        let mut tilemap = world.get_mut::<Tilemap>(parent).unwrap();
        tilemap
            .collision
            .add_cube(tile.position, hook_context.entity);
        tilemap.data.add_cube(tile.position);
        tilemap.dirty = true;
    }

    fn on_discard(mut world: DeferredWorld, hook_context: HookContext) {
        let tile = world.get::<Tile3d>(hook_context.entity).unwrap().clone();
        let parent = world.get::<ChildOf>(hook_context.entity).unwrap().0;
        let mut tilemap = world.get_mut::<Tilemap>(parent).unwrap();
        tilemap.collision.remove_at(tile.position);
        tilemap.data.remove_at(tile.position);
        tilemap.dirty = true;
    }
}

#[derive(Component)]
#[component(immutable)]
struct Tile2d {
    tile: IVec3,
    idx: u32,
}
