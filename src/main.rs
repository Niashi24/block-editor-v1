//! A simple example to show how to spawn an infinite grid.
//!
//! Infinite grids are useful as the ground plane in editor-like applications,
//! as they provide a consistent reference for the orientation of objects
//! and an evenly spaced grid to judge relative scale.

mod save;

use std::collections::{HashMap, HashSet};

use bevy::{
    // camera_controller::,
    dev_tools::infinite_grid::{InfiniteGrid, InfiniteGridPlugin, InfiniteGridSettings}, input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll}, math::VectorSpace, prelude::*, window::CursorOptions
};



use bevy::dev_tools::fps_overlay::*;
use snafu::{OptionExt, Whatever};

use crate::save::Tilemap;

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
        .add_systems(Startup, setup_system)
        .add_systems(Update, (move_system, place_system))
        .init_resource::<MeshData>()
        .run();
}

fn setup_system(
    mut commands: Commands,
    mesh_data: Res<MeshData>,
) {

    commands.spawn((
        // You need to spawn an entity with this component
        InfiniteGrid,
        // Optional component you can use to configure the grid
        InfiniteGridSettings::default(),
    ));

    let camera_transform = Transform::from_xyz(-12.5, 5.0, 10.0).with_rotation(Quat::from_rotation_x(-45.0f32.to_radians()));
    commands.spawn((
        Camera3d::default(),
        // CameraMovement {
        //     translation: camera_transform.translation,
        //     rotation: camera_transform.rotation,
        // },
        camera_transform,
    ));

    commands.spawn((
        DirectionalLight { ..default() },
        Transform::from_translation(Vec3::X * 15. + Vec3::Y * 20.).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((Tilemap::default(), Transform::default(), Visibility::default()));

}

fn get_world_point(
    cursor_position: Vec2,
    camera: &Camera,
    camera_transform: &GlobalTransform,
) -> Result<Vec3, Whatever> {
    let ray = camera.viewport_to_world(camera_transform, cursor_position).unwrap();
    let point = ray.plane_intersection_point(Vec3::ZERO, InfinitePlane3d { normal: Dir3::Y }).unwrap();
    Ok(point)
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
    mut camera: Single<(&mut Transform, &Camera, &GlobalTransform)>,
    window: Single<&Window>,
    key_input: Res<ButtonInput<KeyCode>>,
    mouse_input: Res<ButtonInput<MouseButton>>,
    accumulated_mouse_motion: Res<AccumulatedMouseMotion>,
    accumulated_mouse_scroll: Res<AccumulatedMouseScroll>,
    mut cursor_options: Single<&mut CursorOptions>,
    time: Res<Time>,
    mut gizmos: Gizmos,
) {
    let (mut camera_transform, camera, camera_gb_transform) = camera.into_inner();

    let mut move_speed = 20.0;
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

    if key_input.pressed(KeyCode::KeyE) {
        camera_transform.rotate_y(-rotate_speed * time.delta_secs());
    }
    if key_input.pressed(KeyCode::KeyQ) {
        camera_transform.rotate_y(rotate_speed * time.delta_secs());
    }

    // Zoom
    {
        let delta = accumulated_mouse_scroll.delta;
        let forward = camera_transform.forward();
        camera_transform.translation += forward * delta.y * scroll_speed;
    }



    if let Some(cursor_position) = window.cursor_position()
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
            camera_transform.translation += 0.01 * (accumulated_mouse_motion.delta.y * forward - accumulated_mouse_motion.delta.x * right);
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
        let mut material = StandardMaterial::from_color(Color::srgb_u8(
            153, 155, 156,
        ));
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
    camera: Single<(&Camera, &GlobalTransform)>,
    window: Single<&Window>,
    mouse_input: Res<ButtonInput<MouseButton>>,
    key_input: Res<ButtonInput<KeyCode>>,
    mesh_data: Res<MeshData>,
    mut gizmos: Gizmos,
    mut commands: Commands,
) {
    let (camera, transform) = camera.into_inner();
    let (tilemap_entity, mut tilemap) = tilemap.into_inner();

    if let Some(cursor_position) = window.cursor_position() {
        let ray = camera.viewport_to_world(transform, cursor_position).unwrap();

        let pos = ray.plane_intersection_point(Vec3::ZERO, InfinitePlane3d { normal: Dir3::Y }).unwrap().floor().as_ivec3().with_y(0);

        gizmos.cube(Transform::from_translation(pos.as_vec3() + Vec3::ONE * 0.5), Color::WHITE);

        if mouse_input.pressed(MouseButton::Left) {
            tilemap.add_cube(tilemap_entity, pos, &mut commands, &mesh_data);
        }
    }

    if key_input.pressed(KeyCode::ControlLeft) && key_input.just_pressed(KeyCode::KeyS) {
        tilemap.save();
    }
}
