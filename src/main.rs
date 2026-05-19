//! A simple example to show how to spawn an infinite grid.
//!
//! Infinite grids are useful as the ground plane in editor-like applications,
//! as they provide a consistent reference for the orientation of objects
//! and an evenly spaced grid to judge relative scale.

mod tilemap;
mod scene;


use bevy::{
    // camera_controller::,
    dev_tools::infinite_grid::{InfiniteGrid, InfiniteGridPlugin, InfiniteGridSettings}, input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll}, prelude::*, window::CursorOptions
};



use snafu::{ResultExt, Whatever, whatever};

use crate::{scene::Scene, tilemap::{Tilemap, TilemapCollision, TilemapData, TilemapRayHit, tilemap_coord_to_center}};

fn main() {

    App::new()
        .add_plugins((
            DefaultPlugins,
            InfiniteGridPlugin,
            // DebugPi
            // FpsOverlayPlugin {
            //     ..Default::default()
            // },
        ))
        .add_systems(Startup, (setup_system))
        .add_systems(Update, (move_system, place_system, load_tilemap))
        .init_resource::<MeshData>()
        .run();
}

fn load_tilemap(
    mut tilemap_entity: Single<(&mut Tilemap, &mut Mesh3d)>,
    mut camera_entity: Single<&mut Transform, With<Camera>>,
    mut mesh_assets: ResMut<Assets<Mesh>>,
    input: Res<ButtonInput<KeyCode>>,
) {
    if input.pressed(KeyCode::ControlLeft) && input.just_pressed(KeyCode::KeyL) {
        let scene = Scene::load().unwrap();
        let collision = TilemapCollision::from(&scene.tilemap_data);
        let tilemap = Tilemap { data: scene.tilemap_data, collision };

        let tilemap_mesh = tilemap.data.create_mesh();

        *tilemap_entity.0 = tilemap;
        *tilemap_entity.1 = Mesh3d(mesh_assets.add(tilemap_mesh));

        **camera_entity = Transform::from_isometry(scene.camera);

        // commands.spawn((
        //     tilemap,
        //     Transform::default(),
        //     Visibility::default(),
        //     MeshMaterial3d(mesh_data.cube_material.clone()),
        //     Mesh3d(mesh_assets.add(tilemap_mesh))
        // ));
    }
    if input.pressed(KeyCode::ControlLeft) && input.just_pressed(KeyCode::KeyS) {
        let tilemap_data = tilemap_entity.0.data.clone();
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

// fn load_scene() -> Result<Scene, Whatever> {
//     let file = std::fs::File::open("assets/tilemap/tilemap.json");
//     let file = match file {
//         Ok(file) => file,
//         Err(err) => {
//             if err.kind() == std::io::ErrorKind::NotFound {
//                 println!("tilemap not found. creating new tilemap");
//                 return Ok(Tilemap::default());
//             }
//             return whatever!(Err(err), "");
//         }
//     };

//     let tilemap_saved = serde_json::from_reader::<_, TilemapSaved>(file).whatever_context("")?;
//     let tilemap_data = TilemapData::try_from(tilemap_saved).with_whatever_context(|err| format!("deserialized saved: {err}"))?;
//     let tilemap_collision = TilemapCollision::from(&tilemap_data);

//     Ok(Tilemap {
//         data: tilemap_data,
//         collision: tilemap_collision,
//     })
// }

#[derive(Component, Default)]
struct SelectionState {
    active_plane: Option<(Vec3, Dir3)>,
}

fn setup_system(
    mut commands: Commands,
    mesh_data: Res<MeshData>,
    // asset_server: Res<AssetServer>,
) {
    commands.spawn((
        // You need to spawn an entity with this component
        InfiniteGrid,
        // Optional component you can use to configure the grid
        InfiniteGridSettings::default(),
    ));

    commands.spawn((
        Tilemap::default(),
        MeshMaterial3d(mesh_data.cube_material.clone())
    ));

    let camera_transform = Transform::from_xyz(-12.5, 5.0, 10.0).with_rotation(Quat::from_rotation_x(-45.0f32.to_radians()));
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
        DirectionalLight { ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, 0.0, -45.0f32.to_radians(), -15.0f32.to_radians())),
    ));
}

pub fn intersect_plane_full(ray: &Ray3d, plane_origin: Vec3, plane: InfinitePlane3d) -> Option<f32> {
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

    let forward = camera_transform.forward().as_vec3().with_y(0.0).normalize_or_zero();
    let right = camera_transform.right().as_vec3().with_y(0.0).normalize_or_zero();

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



    if let Some(_cursor_position) = window.cursor_position()
    {
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
            camera_transform.translation += move_speed * (accumulated_mouse_motion.delta.y * forward - accumulated_mouse_motion.delta.x * right);
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
        camera_transform.rotation = Quat::from_euler(EulerRot::YXZ, 45.0f32.to_radians(), -45.0f32.to_radians(), 0.0);
    } else if key_input.just_pressed(KeyCode::Numpad3) {
        camera_transform.rotation = Quat::from_euler(EulerRot::YXZ, -45.0f32.to_radians(), -45.0f32.to_radians(), 0.0);
    }
}

#[derive(Resource, Debug, Clone)]
pub struct MeshData {
    pub cube_mesh: Handle<Mesh>,
    pub cube_material: Handle<StandardMaterial>,
}

impl FromWorld for MeshData {
    fn from_world(world: &mut World) -> Self {
        let cube_mesh = world.resource_mut::<Assets<Mesh>>().add(Cuboid::new(1.0, 1.0, 1.0));
        let asset_server = world.resource_mut::<AssetServer>();
        let image = asset_server.load::<Image>("PrototypeTextures/texture_08.png");
        let mut material = StandardMaterial::from(image);
        material.perceptual_roughness = 1.0;
        let cube_material = world.resource_mut::<Assets<StandardMaterial>>().add(material);

        Self {
            cube_mesh,
            cube_material,
        }
    }
}

fn place_system(
    tilemap: Single<(Entity, &mut Tilemap)>,
    camera: Single<(&Camera, &GlobalTransform, &mut SelectionState)>,
    window: Single<&Window>,
    mouse_input: Res<ButtonInput<MouseButton>>,
    key_input: Res<ButtonInput<KeyCode>>,
    _mesh_data: Res<MeshData>,
    mut mesh_assets: ResMut<Assets<Mesh>>,
    mut gizmos: Gizmos,
    mut commands: Commands,
) {
    let (camera, transform, mut selection_state) = camera.into_inner();
    let (tilemap_entity, mut tilemap) = tilemap.into_inner();

    if let Some(cursor_position) = window.cursor_position() {
        let ray = camera.viewport_to_world(transform, cursor_position).unwrap();

        match selection_state.active_plane.as_ref() {
            None => {
                let ray_hit = {
                    if let Some(hit) = tilemap.ray_intersection(ray) {
                        hit
                    } else {
                        let time_of_impact = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d { normal: Dir3::Y }).unwrap();
                        let tile = (ray.get_point(time_of_impact) + Dir3::Y * 0.01).floor().as_ivec3();
                        TilemapRayHit {
                            time_of_impact,
                            hit_normal: Vec3::Y,
                            tile,
                        }
                    }
                };

                gizmos.circle(Isometry3d::new(ray.get_point(ray_hit.time_of_impact), Transform::default().looking_to(ray_hit.hit_normal, Vec3::Y).rotation), 0.125, Color::WHITE);


                gizmos.cube(Transform::from_translation(tilemap_coord_to_center(ray_hit.tile)), Color::WHITE);

                if mouse_input.any_pressed([MouseButton::Left, MouseButton::Right]) {
                    selection_state.active_plane = Some((ray.get_point(ray_hit.time_of_impact), Dir3::new(ray_hit.hit_normal).unwrap()));
                }
            },
            Some(&(origin, normal)) => {
                if !mouse_input.pressed(MouseButton::Left) && !mouse_input.pressed(MouseButton::Right) {
                    selection_state.active_plane = None;
                } else {
                    let ray_hit = {
                        let time_of_impact = ray.intersect_plane(origin, InfinitePlane3d { normal }).unwrap();
                        let mut point = ray.get_point(time_of_impact) + normal * 0.01;
                        if mouse_input.pressed(MouseButton::Right) {
                            point -= *normal;
                        }

                        let tile = point.floor().as_ivec3();
                        TilemapRayHit {
                            time_of_impact,
                            hit_normal: Vec3::Y,
                            tile,
                        }
                    };

                    gizmos.circle(Isometry3d::new(ray.get_point(ray_hit.time_of_impact), Transform::default().looking_to(ray_hit.hit_normal, Vec3::Y).rotation), 0.125, Color::WHITE);


                    gizmos.cube(Transform::from_translation(tilemap_coord_to_center(ray_hit.tile)), Color::WHITE);

                    if mouse_input.pressed(MouseButton::Left) {
                        tilemap.add_cube(tilemap_entity, ray_hit.tile, &mut commands, &mut mesh_assets);
                    } else {
                        tilemap.remove_at(tilemap_entity, ray_hit.tile, &mut commands, &mut mesh_assets);
                    }
                }
            },
        }
    }

    if key_input.pressed(KeyCode::ControlLeft) && key_input.just_pressed(KeyCode::KeyE) {
        tilemap.data.export();
    }
}
