use std::{
    collections::BTreeMap,
    fs::{self},
    io::Write,
};

use bevy::{
    asset::{Assets, RenderAssetUsages},
    ecs::{
        component::Component, entity::Entity, hierarchy::ChildOf, system::EntityCommand,
        world::EntityWorldMut,
    },
    math::{Dir3, IVec3, Quat, Vec2, Vec3},
    mesh::{Indices, Mesh, Mesh3d},
    platform::collections::HashMap,
    shape::{Plane3d, Ray3d},
    transform::components::Transform,
};

use gltf_json::{
    self as gltf,
    material::{EmissiveFactor, PbrMetallicRoughness},
    texture::Info,
    validation::{Checked, USize64},
};
use parry3d::{
    math::{Pose3, Vec3 as Vec3P, Vector},
    partitioning::Bvh,
    query::Ray,
    shape::{Cuboid, Shape},
};
use serde::{Deserialize, Serialize};
use wgpu_types::PrimitiveTopology;

use crate::{Tile3d, TileKind, observe_draw_hit_gizmos};

pub fn tilemap_coord_to_center(coord: IVec3) -> Vec3 {
    coord.as_vec3() + Vec3::splat(0.5)
}

pub fn tilemap_coord_to_pose(coord: IVec3) -> Pose3 {
    Pose3::from_translation(Vec3P::from_array(tilemap_coord_to_center(coord).to_array()))
}

#[derive(Default)]
pub struct TilemapCollision {
    bvh: Bvh,
    tile_to_node: HashMap<IVec3, u32>,
    node_to_shape: HashMap<u32, (Box<dyn Shape>, IVec3, Entity)>,
    node_idx: u32,
}

// impl<'a> From<&'a TilemapData> for TilemapCollision {
//     fn from(value: &'a TilemapData) -> Self {
//         let mut out = TilemapCollision::default();

//         for (pos, tile) in value.tiles.iter() {
//             match tile.tile_type {
//                 TileType::Cube(_) => {
//                     out.add_cube(*pos);
//                 },
//             }
//         }

//         out
//     }
// }

impl TilemapCollision {
    pub fn add_cube(&mut self, pos: IVec3, entity: Entity) {
        self.remove_at(pos);
        let transform = tilemap_coord_to_pose(pos);

        let cube = Cuboid::new(Vector::splat(0.5));
        let aabb = cube.aabb(&transform);

        self.bvh.insert(aabb, self.node_idx);
        self.tile_to_node.insert(pos, self.node_idx);
        self.node_to_shape
            .insert(self.node_idx, (Box::new(cube), pos, entity));
        self.node_idx += 1;
    }

    pub fn remove_at(&mut self, pos: IVec3) {
        let Some(node) = self.tile_to_node.get(&pos) else {
            return;
        };

        self.bvh.remove(*node);
        self.node_to_shape.remove(node);
        self.tile_to_node.remove(&pos);
    }

    pub fn ray_intersection(&self, ray: Ray3d) -> Option<TilemapRayHit> {
        let ray_parry = Ray {
            origin: Vec3P::from_array(ray.origin.to_array()),
            dir: Vec3P::from_array(ray.direction.to_array()),
        };

        let (node, _hit_time) =
            self.bvh
                .cast_ray(&ray_parry, f32::INFINITY, |node, best_hit| {
                    let (shape, pos, _entity) = &self.node_to_shape[&node];
                    let pose = tilemap_coord_to_pose(*pos);
                    shape.cast_ray(&pose, &ray_parry, best_hit, true)
                })?;

        let (shape, pos, entity) = &self.node_to_shape[&node];
        let pose = tilemap_coord_to_pose(*pos);
        let hit = shape
            .cast_ray_and_get_normal(&pose, &ray_parry, f32::INFINITY, true)
            .unwrap();

        Some(TilemapRayHit {
            time_of_impact: hit.time_of_impact,
            hit_normal: Vec3::from_array(hit.normal.to_array()),
            tile: *pos,
            entity: Some(*entity),
        })
    }
}

#[derive(Component, Default)]
#[require(Transform, Mesh3d)]
pub struct Tilemap {
    pub data: TilemapData,
    pub collision: TilemapCollision,
    pub dirty: bool,
}

impl Tilemap {
    pub fn get_entity(&self, pos: IVec3) -> Option<Entity> {
        let id = self.collision.tile_to_node.get(&pos)?;
        Some(self.collision.node_to_shape.get(id).unwrap().2)
    }

    // pub fn add_cube(&mut self, tilemap_entity: Entity, pos: IVec3, commands: &mut Commands, mesh_assets: &mut ResMut<Assets<Mesh>>) {
    //     self.data.add_cube(tilemap_entity, pos, commands, mesh_assets);
    //     self.collision.add_cube(pos);
    // }

    // pub fn remove_at(&mut self, tilemap_entity: Entity, pos: IVec3, commands: &mut Commands, mesh_assets: &mut ResMut<Assets<Mesh>>) {
    //     self.collision.remove_at(pos);
    //     self.data.remove_at(tilemap_entity, pos, commands, mesh_assets);
    // }

    pub fn ray_intersection(&self, ray: Ray3d) -> Option<TilemapRayHit> {
        self.collision.ray_intersection(ray)
    }

    pub fn add_cube(pos: IVec3) -> impl EntityCommand {
        move |world: EntityWorldMut| {
            let tile_entity = world.get::<Tilemap>().unwrap().get_entity(pos);

            let tilemap_id = world.id();
            let world = world.into_world_mut();
            if let Some(tile_entity) = tile_entity {
                world.entity_mut(tile_entity).insert(Tile3d {
                    position: pos,
                    tile_kind: TileKind::Cube,
                });
            } else {
                world
                    .spawn((
                        Tile3d {
                            position: pos,
                            tile_kind: TileKind::Cube,
                        },
                        ChildOf(tilemap_id),
                    ))
                    .observe(observe_draw_hit_gizmos);
            }
        }
    }

    pub fn remove_at(pos: IVec3) -> impl EntityCommand {
        move |world: EntityWorldMut| {
            let tile_entity = world.get::<Tilemap>().unwrap().get_entity(pos);

            if let Some(tile_entity) = tile_entity {
                world.into_world_mut().despawn(tile_entity);
            }
        }
    }

    fn regenerate_mesh(mut world: EntityWorldMut) {
        let tilemap = world.get::<Tilemap>().unwrap();
        let mesh = tilemap.data.create_mesh();
        let mesh =
            world.resource_scope::<Assets<Mesh>, _>(|_, mut mesh_assets| mesh_assets.add(mesh));

        world.insert((Mesh3d(mesh),));
    }
}

pub struct TilemapRayHit {
    pub time_of_impact: f32,
    pub hit_normal: Vec3,
    pub tile: IVec3,
    /// If none, hit the infinite plane instead
    pub entity: Option<Entity>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct TilemapData {
    pub tiles: HashMap<IVec3, TileData>,
    pub image: Option<TilemapImage>,
}

impl TilemapData {
    pub fn add_cube(&mut self, pos: IVec3) {
        if self.tiles.contains_key(&pos) {
            return;
        }

        self.tiles.insert(
            pos,
            TileData {
                tile_type: TileType::default_cube(),
            },
        );
    }

    pub fn get(&self, pos: IVec3) -> Option<&TileData> {
        self.tiles.get(&pos)
    }

    pub fn remove_at(&mut self, pos: IVec3) {
        self.tiles.remove(&pos);
    }

    pub fn export(&self) {
        let mut root = gltf::Root::default();

        let image_path = self
            .image
            .as_ref()
            .map(|img| img.path.clone())
            .unwrap_or_else(|| "PrototypeTextures.png".to_string());
        let image_path = format!("../assets/{}", image_path);

        let image = root.push(gltf::Image {
            buffer_view: None,
            mime_type: None,
            name: None,
            uri: Some(image_path),
            extensions: Default::default(),
            extras: Default::default(),
        });

        let sampler = root.push(gltf::texture::Sampler {
            mag_filter: Some(Checked::Valid(gltf_json::texture::MagFilter::Nearest)),
            min_filter: None,
            name: None,
            wrap_s: Checked::Valid(gltf_json::texture::WrappingMode::Repeat),
            wrap_t: Checked::Valid(gltf_json::texture::WrappingMode::Repeat),
            extensions: Default::default(),
            extras: Default::default(),
        });

        let texture = root.push(gltf::Texture {
            name: None,
            sampler: Some(sampler),
            source: image,
            extensions: Default::default(),
            extras: Default::default(),
        });

        let material = root.push(gltf::Material {
            alpha_cutoff: None,
            alpha_mode: Checked::Valid(gltf::material::AlphaMode::Blend),
            double_sided: false,
            name: None,
            pbr_metallic_roughness: PbrMetallicRoughness {
                base_color_texture: Some(Info {
                    index: texture,
                    tex_coord: 0,
                    extensions: Default::default(),
                    extras: Default::default(),
                }),
                ..Default::default()
            },
            normal_texture: None,
            occlusion_texture: None,
            emissive_texture: None,
            emissive_factor: EmissiveFactor::default(),
            extensions: Default::default(),
            extras: Default::default(),
        });

        let meshes = [self.create_mesh()];

        let mut buf = Vec::new();
        struct MeshBuffer {
            min_position: [f32; 3],
            max_position: [f32; 3],
            offset: usize,

            num_vertices: usize,
            indices_offset: usize,
            index_count: usize,
            indices_component_type: gltf::accessor::ComponentType,
        }
        let mut mesh_buffers = Vec::with_capacity(meshes.len());
        for mesh in meshes {
            let position = mesh.attribute(Mesh::ATTRIBUTE_POSITION.id).unwrap();
            let mut min = [f32::MAX; 3];
            let mut max = [f32::MIN; 3];
            for &[x, y, z] in position.as_float3().unwrap() {
                min[0] = min[0].min(x);
                max[0] = max[0].max(x);
                min[1] = min[1].min(y);
                max[1] = max[1].max(y);
                min[2] = min[2].min(z);
                max[2] = max[2].max(z);
            }

            let start_index = buf.len();
            let packed = mesh.create_packed_vertex_buffer_data();
            // mesh.write_packed_vertex_buffer_data(WriteOnly::from_mut(&mut buf));
            buf.extend_from_slice(&packed);
            pad_to_4(&mut buf);

            let indices_offset = buf.len();
            let (index_count, indices_component_type) = match mesh.indices().unwrap() {
                bevy::mesh::Indices::U16(indices) => {
                    for i in indices {
                        buf.extend_from_slice(&i.to_le_bytes());
                    }
                    (indices.len(), gltf::accessor::ComponentType::U16)
                }
                bevy::mesh::Indices::U32(indices) => {
                    for i in indices {
                        buf.extend_from_slice(&i.to_le_bytes());
                    }
                    (indices.len(), gltf::accessor::ComponentType::U32)
                }
            };

            pad_to_4(&mut buf);

            mesh_buffers.push(MeshBuffer {
                min_position: min,
                max_position: max,
                offset: start_index,
                num_vertices: mesh.count_vertices(),
                indices_offset,
                index_count,
                indices_component_type,
            })
        }

        let buffer = root.push(gltf::Buffer {
            byte_length: USize64::from(buf.len()),
            extensions: Default::default(),
            extras: Default::default(),
            name: None,
            uri: Some("buffer0.bin".into()),
        });

        const VERTEX_SIZE: usize = 32; // 12 + 12 + 8

        let mut primitives = Vec::new();

        for view in mesh_buffers {
            let buffer_view = root.push(gltf::buffer::View {
                buffer,
                byte_length: USize64::from(view.num_vertices * VERTEX_SIZE),
                byte_offset: Some(USize64::from(view.offset)),
                byte_stride: Some(gltf::buffer::Stride(VERTEX_SIZE)),
                name: None,
                target: Some(Checked::Valid(gltf::buffer::Target::ArrayBuffer)),
                extensions: Default::default(),
                extras: Default::default(),
            });

            let position = root.push(gltf::Accessor {
                buffer_view: Some(buffer_view),
                byte_offset: Some(USize64(0)),
                count: USize64::from(view.num_vertices),
                component_type: Checked::Valid(gltf::accessor::GenericComponentType(
                    gltf::accessor::ComponentType::F32,
                )),
                extensions: Default::default(),
                extras: Default::default(),
                type_: Checked::Valid(gltf::accessor::Type::Vec3),
                min: Some(gltf::Value::from(Vec::from(view.min_position))),
                max: Some(gltf::Value::from(Vec::from(view.max_position))),
                name: None,
                normalized: false,
                sparse: None,
            });

            let normal = root.push(gltf::Accessor {
                buffer_view: Some(buffer_view),
                byte_offset: Some(USize64::from(3 * size_of::<f32>())),
                count: USize64::from(view.num_vertices),
                component_type: Checked::Valid(gltf::accessor::GenericComponentType(
                    gltf::accessor::ComponentType::F32,
                )),
                extensions: Default::default(),
                extras: Default::default(),
                type_: Checked::Valid(gltf::accessor::Type::Vec3),
                min: None,
                max: None,
                name: None,
                normalized: false,
                sparse: None,
            });

            let uv = root.push(gltf::Accessor {
                buffer_view: Some(buffer_view),
                byte_offset: Some(USize64::from(6 * size_of::<f32>())),
                count: USize64::from(view.num_vertices),
                component_type: Checked::Valid(gltf::accessor::GenericComponentType(
                    gltf::accessor::ComponentType::F32,
                )),
                extensions: Default::default(),
                extras: Default::default(),
                type_: Checked::Valid(gltf::accessor::Type::Vec2),
                min: None,
                max: None,
                name: None,
                normalized: false,
                sparse: None,
            });

            let indices_buffer_view = root.push(gltf::buffer::View {
                buffer,
                byte_length: USize64::from(
                    view.index_count
                        * match view.indices_component_type {
                            gltf::accessor::ComponentType::U16 => 2,
                            gltf::accessor::ComponentType::U32 => 4,
                            _ => unreachable!(),
                        },
                ),
                byte_offset: Some(USize64::from(view.indices_offset)),
                byte_stride: None,
                name: None,
                target: Some(Checked::Valid(gltf::buffer::Target::ElementArrayBuffer)),
                extensions: Default::default(),
                extras: Default::default(),
            });
            let indices = root.push(gltf::Accessor {
                buffer_view: Some(indices_buffer_view),
                byte_offset: Some(USize64(0)),
                count: USize64::from(view.index_count),
                component_type: Checked::Valid(gltf_json::accessor::GenericComponentType(
                    view.indices_component_type,
                )),
                extensions: Default::default(),
                extras: Default::default(),
                type_: Checked::Valid(gltf::accessor::Type::Scalar),
                min: None,
                max: None,
                name: None,
                normalized: false,
                sparse: None,
            });

            let primitive = gltf::mesh::Primitive {
                attributes: {
                    let mut map = BTreeMap::new();
                    map.insert(Checked::Valid(gltf::mesh::Semantic::Positions), position);
                    map.insert(Checked::Valid(gltf::mesh::Semantic::Normals), normal);
                    map.insert(Checked::Valid(gltf::mesh::Semantic::TexCoords(0)), uv);
                    map
                },
                extensions: Default::default(),
                extras: Default::default(),
                indices: Some(indices),
                material: Some(material),
                mode: Checked::Valid(gltf::mesh::Mode::Triangles),
                targets: None,
            };

            primitives.push(primitive);
        }

        let mesh = root.push(gltf::Mesh {
            extensions: Default::default(),
            extras: Default::default(),
            name: None,
            primitives,
            weights: None,
        });

        let node = root.push(gltf::Node {
            mesh: Some(mesh),
            ..Default::default()
        });

        root.push(gltf::Scene {
            extensions: Default::default(),
            extras: Default::default(),
            name: None,
            nodes: vec![node],
        });

        let _ = fs::create_dir("tilemap");
        let writer = fs::File::create("tilemap/tilemap.gltf").unwrap();
        gltf::serialize::to_writer_pretty(writer, &root).unwrap();

        let mut writer = fs::File::create("tilemap/buffer0.bin").unwrap();
        writer.write_all(&buf).unwrap();

        println!("Successfully exported.");
    }

    pub fn create_mesh(&self) -> Mesh {
        let mut mesh = Mesh::new(
            wgpu_types::PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_indices(Indices::U32(vec![]))
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<Vec3>::new())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, Vec::<[f32; 3]>::new())
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, Vec::<[f32; 2]>::new());

        for (pos, tile) in self.tiles.iter() {
            match tile.tile_type {
                TileType::Cube(face_data) => {
                    for (i, dir) in Dir::DIRS.into_iter().enumerate() {
                        let new_pos = *pos + dir.as_ivec3();
                        if self
                            .tiles
                            .get(&new_pos)
                            .is_none_or(|tile| !tile.covers_dir(&dir.inv()))
                        {
                            let translation = tilemap_coord_to_center(*pos) + 0.5 * dir.as_dir3();
                            let _plane = Plane3d {
                                normal: dir.as_dir3(),
                                half_size: Vec2::splat(0.5),
                            };

                            let face = face_data[i];
                            let uv = face
                                .and_then(|face| {
                                    self.image.as_ref().map(|img| img.uv(face.tile_idx))
                                })
                                .unwrap_or([[0.0; 2], [1.0; 2]]);
                            // let uv = [[0.0; 2], [1.0; 2]];

                            let plane_mesh = construct_plane_mesh(translation, dir.as_dir3(), uv);

                            // let mut plane_mesh = plane.mesh().build();
                            // plane_mesh = plane_mesh.translated_by(translation);
                            mesh.merge(&plane_mesh).unwrap();
                            // meshes.push(plane_mesh);
                        }
                    }
                }
            }
        }

        mesh
    }
}

fn construct_plane_mesh(translation: Vec3, normal: Dir3, uv: [[f32; 2]; 2]) -> Mesh {
    let mut positions: Vec<Vec3> = Vec::with_capacity(4);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(4);
    // let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(4);

    let rotation = Quat::from_rotation_arc(Vec3::Y, *normal);

    for z in 0..2 {
        for x in 0..2 {
            let tx = x as f32;
            let tz = z as f32;
            let pos = rotation * Vec3::new(-0.5 + tx, 0.0, -0.5 + tz) + translation;
            positions.push(pos);
            normals.push(normal.to_array());
            // let uv_x = FloatExt::lerp(tx, uv[0][0], uv[1][0]);
            // let uv_z = FloatExt::lerp(tz, uv[0][1], uv[1][1]);
            // uvs.push([uv_x, uv_z]);
        }
    }

    let [min_uv, max_uv] = uv;
    let uvs = vec![
        min_uv,
        [max_uv[0], min_uv[1]],
        [min_uv[0], max_uv[1]],
        max_uv,
    ];
    let indices = vec![3, 1, 2, 0, 2, 1];

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_indices(Indices::U32(indices))
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TilemapImage {
    pub path: String,
    pub image_width: u32,
    pub image_height: u32,
    pub tile_width: u32,
    pub tile_height: u32,
}

impl TilemapImage {
    pub fn uv(&self, i: u32) -> [[f32; 2]; 2] {
        let tiles_wide = self.tiles_wide();
        let (tl_x, tl_y) = (i % tiles_wide, i / tiles_wide);
        let (tl_x, tl_y) = (tl_x * self.tile_width, tl_y * self.tile_height);
        let (br_x, br_y) = (tl_x + self.tile_width, tl_y + self.tile_height);

        [[tl_x, tl_y], [br_x, br_y]].map(|f| {
            [
                f[0] as f32 / self.image_width as f32,
                f[1] as f32 / self.image_height as f32,
            ]
        })
    }

    fn tiles_wide(&self) -> u32 {
        self.image_width / self.tile_width
    }
}

fn pad_to_4(buffer: &mut Vec<u8>) {
    while buffer.len() % 4 != 0 {
        buffer.push(0);
    }
}

#[derive(Copy, Clone)]
pub enum Dir {
    Z,
    NegZ,
    X,
    NegX,
    Y,
    NegY,
}

impl Dir {
    pub const DIRS: [Dir; 6] = [Dir::Z, Dir::NegZ, Dir::X, Dir::NegX, Dir::Y, Dir::NegY];

    fn inv(self) -> Self {
        match self {
            Dir::Z => Dir::NegZ,
            Dir::NegZ => Dir::Z,
            Dir::X => Dir::NegX,
            Dir::NegX => Dir::X,
            Dir::Y => Dir::NegY,
            Dir::NegY => Dir::Y,
        }
    }

    pub fn as_ivec3(&self) -> IVec3 {
        match self {
            Dir::Z => IVec3::Z,
            Dir::NegZ => IVec3::NEG_Z,
            Dir::X => IVec3::X,
            Dir::NegX => IVec3::NEG_X,
            Dir::Y => IVec3::Y,
            Dir::NegY => IVec3::NEG_Y,
        }
    }

    pub fn as_dir3(&self) -> Dir3 {
        match self {
            Dir::Z => Dir3::Z,
            Dir::NegZ => Dir3::NEG_Z,
            Dir::X => Dir3::X,
            Dir::NegX => Dir3::NEG_X,
            Dir::Y => Dir3::Y,
            Dir::NegY => Dir3::NEG_Y,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TileData {
    pub tile_type: TileType,
}

impl TileData {
    pub fn covers_dir(&self, _dir: &Dir) -> bool {
        match self.tile_type {
            TileType::Cube(_) => true,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum TileType {
    Cube([Option<FaceData>; 6]),
}

impl TileType {
    pub fn default_cube() -> Self {
        Self::Cube([Some(FaceData { tile_idx: 0 }); 6])
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct FaceData {
    tile_idx: u32,
}
