use std::{collections::BTreeMap, f32, fs::{self, File}, io::{BufWriter, Write}};

use bevy::{asset::{Assets, RenderAssetUsages}, ecs::{component::Component, entity::Entity, system::{Commands, ResMut}}, math::{Dir3, IVec3, Ray3d, Vec2, Vec3, primitives::Plane3d}, mesh::{Indices, Mesh, Mesh3d, MeshBuilder, Meshable}, platform::collections::HashMap};

use gltf_json::{self as gltf, validation::{Checked, USize64}};
use parry3d::{math::{Pose3, Vec3 as Vec3P, Vector}, partitioning::Bvh, query::Ray, shape::{Cuboid, Shape}};
use serde::{Serialize, Deserialize};
use snafu::{OptionExt, ResultExt, Whatever};
use sscanf::sscanf;


pub fn tilemap_coord_to_center(coord: IVec3) -> Vec3 {
    coord.as_vec3() + Vec3::splat(0.5)
}

pub fn tilemap_coord_to_pose(coord: IVec3) -> Pose3 {
    Pose3::from_translation(Vec3P::from_array(tilemap_coord_to_center(coord).to_array()))
}

#[derive(Default, Component)]
pub struct TilemapCollision {
    bvh: Bvh,
    tile_to_node: HashMap<IVec3, u32>,
    node_to_shape: HashMap<u32, (Box<dyn Shape>, IVec3)>,
    node_idx: u32,
}

impl<'a> From<&'a TilemapData> for TilemapCollision {
    fn from(value: &'a TilemapData) -> Self {
        let mut out = TilemapCollision::default();

        for (pos, tile) in value.tiles.iter() {
            match tile.tile_type {
                TileType::Cube => {
                    out.add_cube(*pos);
                },
            }
        }

        out
    }
}

impl TilemapCollision {
    pub fn add_cube(&mut self, pos: IVec3) {
        self.remove_at(pos);
        let transform = tilemap_coord_to_pose(pos);

        let cube = Cuboid::new(Vector::splat(0.5));
        let aabb = cube.aabb(&transform);

        self.bvh.insert(aabb, self.node_idx);
        self.tile_to_node.insert(pos, self.node_idx);
        self.node_to_shape.insert(self.node_idx, (Box::new(cube), pos));
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

        let (node, _hit_time) = self.bvh.cast_ray(&ray_parry, f32::INFINITY, |node, best_hit| {
            let (shape, pos) = &self.node_to_shape[&node];
            let pose = tilemap_coord_to_pose(*pos);
            shape.cast_ray(&pose, &ray_parry, best_hit, true)
        })?;

        let (shape, pos) = &self.node_to_shape[&node];
        let pose = tilemap_coord_to_pose(*pos);
        let hit = shape.cast_ray_and_get_normal(&pose, &ray_parry, f32::INFINITY, true).unwrap();

        Some(TilemapRayHit {
            time_of_impact: hit.time_of_impact,
            hit_normal: Vec3::from_array(hit.normal.to_array()),
            tile: *pos,
        })
    }
}

#[derive(Component, Default)]
pub struct Tilemap {
    pub data: TilemapData,
    pub collision: TilemapCollision,
}

impl Tilemap {
    pub fn add_cube(&mut self, tilemap_entity: Entity, pos: IVec3, commands: &mut Commands, mesh_assets: &mut ResMut<Assets<Mesh>>) {
        self.data.add_cube(tilemap_entity, pos, commands, mesh_assets);
        self.collision.add_cube(pos);
    }

    pub fn remove_at(&mut self, tilemap_entity: Entity, pos: IVec3, commands: &mut Commands, mesh_assets: &mut ResMut<Assets<Mesh>>) {
        self.collision.remove_at(pos);
        self.data.remove_at(tilemap_entity, pos, commands, mesh_assets);
    }

    pub fn ray_intersection(&self, ray: Ray3d) -> Option<TilemapRayHit> {
        self.collision.ray_intersection(ray)
    }
}

pub struct TilemapRayHit {
    pub time_of_impact: f32,
    pub hit_normal: Vec3,
    pub tile: IVec3,
}

#[derive(Clone, Default)]
pub struct TilemapData {
    pub tiles: HashMap<IVec3, TileData>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct TilemapSaved {
    pub tiles: HashMap<String, TileData>,
}

impl From<TilemapData> for TilemapSaved {
    fn from(value: TilemapData) -> Self {
        Self {
            tiles: value.tiles.into_iter().map(|(pos, tile)| (pos.to_string(), tile)).collect()
        }
    }
}

impl TryFrom<TilemapSaved> for TilemapData {
    type Error = Whatever;

    fn try_from(value: TilemapSaved) -> Result<Self, Self::Error> {
        let tiles = value.tiles.into_iter()
            .map(|(pos_str, tile)| {
            let (x, y, z) = sscanf!(pos_str, "[{i32}, {i32}, {i32}]").with_whatever_context(|| format!("parsing: {:?}", pos_str))?;
                Ok((IVec3::new(x, y, z), tile))
            })
            .collect::<Result<_, _>>()?;

        Ok(Self {
            tiles
        })
    }
}

impl TilemapData {
    pub fn add_cube(&mut self, tilemap_entity: Entity, pos: IVec3, commands: &mut Commands, mesh_assets: &mut ResMut<Assets<Mesh>>) {
        if self.tiles.contains_key(&pos) { return; }

        self.tiles.insert(pos, TileData {
            tile_type: TileType::Cube,
        });

        self.refresh_mesh(tilemap_entity, commands, mesh_assets);
    }

    pub fn remove_at(&mut self, tilemap_entity: Entity, pos: IVec3, commands: &mut Commands, mesh_assets: &mut ResMut<Assets<Mesh>>) {
        if self.tiles.remove(&pos).is_some() {
            self.refresh_mesh(tilemap_entity, commands, mesh_assets);
        }
    }

    pub fn refresh_mesh(&self, tilemap_entity: Entity, commands: &mut Commands, mesh_assets: &mut ResMut<Assets<Mesh>>) {
        let mesh = self.create_mesh();
        let mesh = mesh_assets.add(mesh);

        commands.entity(tilemap_entity).insert(Mesh3d(mesh));
    }

    pub fn save(&self) -> Result<(), Whatever> {
        std::fs::create_dir_all("assets/tilemap").whatever_context("create dir")?;

        let file = File::create("assets/tilemap/tilemap.json").whatever_context("create file")?;
        let mut file = BufWriter::new(file);
        let saveable = TilemapSaved::from(self.clone());
        serde_json::ser::to_writer(&mut file, &saveable).with_whatever_context(|err| format!("write json: {err}"))?;

        println!("Successfully saved.");

        Ok(())
    }

    pub fn export(&self) {

        let mut root = gltf::Root::default();

        let meshes = [
            self.create_mesh()
        ];

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
                        }
                ),
                byte_offset: Some(USize64::from(view.indices_offset)),
                byte_stride: None,
                name: None,
                target: Some(Checked::Valid(
                    gltf::buffer::Target::ElementArrayBuffer
                )),
                extensions: Default::default(),
                extras: Default::default(),
            });
            let indices = root.push(gltf::Accessor {
                buffer_view: Some(indices_buffer_view),
                byte_offset: Some(USize64(0)),
                count: USize64::from(view.index_count),
                component_type: Checked::Valid(gltf_json::accessor::GenericComponentType(view.indices_component_type)),
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
                material: None,
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
        let mut mesh = Mesh::new(wgpu_types::PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_indices(Indices::U32(vec![]))
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<Vec3>::new())
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, Vec::<[f32; 3]>::new())
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, Vec::<[f32; 2]>::new());

        for (pos, tile) in self.tiles.iter() {
            match tile.tile_type {
                TileType::Cube => {

                    for dir in Dir::DIRS {
                        let new_pos = *pos + dir.as_ivec3();
                        if self.tiles.get(&new_pos).is_none_or(|tile| !tile.covers_dir(&dir.inv())) {
                            let translation = tilemap_coord_to_center(*pos) + 0.5 * dir.as_dir3();
                            let plane = Plane3d {
                                normal: dir.as_dir3(),
                                half_size: Vec2::splat(0.5),
                            };

                            let mut plane_mesh = plane.mesh().build();
                            plane_mesh = plane_mesh.translated_by(translation);
                            mesh.merge(&plane_mesh).unwrap();
                            // meshes.push(plane_mesh);
                        }
                    }
                },
            }
        }

        mesh
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
    pub const DIRS: [Dir; 6] = [
        Dir::Z,
        Dir::NegZ,
        Dir::X,
        Dir::NegX,
        Dir::Y,
        Dir::NegY,
    ];

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

#[derive(Clone, Serialize, Deserialize)]
pub struct TileData {
    pub tile_type: TileType,
}

impl TileData {
    pub fn covers_dir(&self, _dir: &Dir) -> bool {
        match self.tile_type {
            TileType::Cube => {
                true
            },
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum TileType {
    Cube,
}
