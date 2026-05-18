use std::{collections::BTreeMap, fs, io::Write};

use bevy::{asset::{Assets, RenderAssetUsages}, ecs::{component::Component, entity::Entity, system::{Commands, ResMut}}, math::{Dir3, IVec3, Vec2, Vec3, primitives::Plane3d}, mesh::{Indices, Mesh, Mesh3d, MeshBuilder, Meshable}, pbr::MeshMaterial3d, platform::collections::HashMap, transform::components::Transform};

use gltf_json::{self as gltf, Accessor, validation::{Checked, USize64}};

use crate::MeshData;

#[derive(Component, Default)]
pub struct Tilemap {
    pub tiles: HashMap<IVec3, TileData>,
}

impl Tilemap {
    pub fn add_cube(&mut self, tilemap_entity: Entity, pos: IVec3, commands: &mut Commands, mesh_assets: &mut ResMut<Assets<Mesh>>) {
        if self.tiles.contains_key(&pos) { return; }

        self.tiles.insert(pos, TileData {
            tile_type: TileType::Cube,
        });

        let mesh = self.create_mesh();
        let mesh = mesh_assets.add(mesh);

        commands.entity(tilemap_entity).insert(Mesh3d(mesh));
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
                            let translation = pos.as_vec3() + Vec3::splat(0.5) + 0.5 * dir.as_dir3();
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

pub struct TilemapMesh {
    planes: Vec<Transform>,
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

pub struct TileData {
    pub tile_type: TileType,
}

impl TileData {
    pub fn covers_dir(&self, dir: &Dir) -> bool {
        match self.tile_type {
            TileType::Cube => {
                true
            },
        }
    }
}

pub enum TileType {
    Cube,
}
