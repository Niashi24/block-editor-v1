use std::{collections::BTreeMap, fs, io::Write};

use bevy::{asset::{Handle, RenderAssetUsages}, ecs::{component::Component, entity::Entity, system::Commands}, math::{Dir3, IVec3, Quat, Vec2, Vec3, primitives::Plane3d}, mesh::{Mesh, Mesh3d, MeshBuilder, MeshVertexAttributeId, Meshable}, pbr::{MeshMaterial3d, StandardMaterial}, platform::collections::HashMap, transform::components::Transform};

use gltf_json::{self as gltf, Accessor, Index, validation::{Checked, USize64}};
use wgpu_types::WriteOnly;

use crate::MeshData;

#[derive(Component, Default)]
pub struct Tilemap {
    pub tiles: HashMap<IVec3, TileData>,
}

impl Tilemap {
    pub fn add_cube(&mut self, tilemap_entity: Entity, pos: IVec3, commands: &mut Commands, mesh_data: &MeshData) {
        if self.tiles.contains_key(&pos) { return; }

        let entity = commands.entity(tilemap_entity).with_child((
            Mesh3d(mesh_data.cube_mesh.clone()),
            MeshMaterial3d(mesh_data.cube_material.clone()),
            Transform::from_translation(pos.as_vec3() + Vec3::ONE * 0.5),
        )).id();

        self.tiles.insert(pos, TileData {
            entity,
            tile_type: TileType::Cube,
        });
    }

    pub fn save(&self) {

        let mesh = self.to_mesh();

        let mut root = gltf::Root::default();

        let meshes = [
            Plane3d::new(Vec3::Y, Vec2::splat(0.5)).mesh().build()
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
            // let position = mesh.attribute(Mesh::ATTRIBUTE_POSITION.id).unwrap();
            // let pos_start = buffer.len();
            // buffer.extend_from_slice(position.get_bytes());
            // pad_to_4(&mut buffer);

            // let normal = mesh.attribute(Mesh::ATTRIBUTE_NORMAL.id).unwrap();
            // let normal_start = buffer.len();
            // buffer.extend_from_slice(normal.get_bytes());
            // pad_to_4(&mut buffer);

            // let uv = mesh.attribute(Mesh::ATTRIBUTE_UV_0.id).unwrap();
            // let uv_start = buffer.len();
            // buffer.extend_from_slice(uv.get_bytes());
            // pad_to_4(&mut buffer);
            // mesh.create_packed_vertex_buffer_data()
            //
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
            // indices.

            mesh_buffers.push(MeshBuffer {
                // position: (pos_start, normal_start - pos_start),
                // normal: (normal_start, uv_start - normal_start),
                // uv: (uv_start, buffer.len() - uv_start),

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

        struct Accessors {
            position: Index<Accessor>,
            normal: Index<Accessor>,
            uv: Index<Accessor>,
            indices: Index<Accessor>,
        }

        let mut accessors = Vec::with_capacity(mesh_buffers.len());

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

            accessors.push(Accessors {
                position,
                normal,
                uv,
                indices,
            })
        }

        let plane_primitive = gltf::mesh::Primitive {
            attributes: {
                let mut map = BTreeMap::new();
                map.insert(Checked::Valid(gltf::mesh::Semantic::Positions), accessors[0].position);
                map.insert(Checked::Valid(gltf::mesh::Semantic::Normals), accessors[0].normal);
                map.insert(Checked::Valid(gltf::mesh::Semantic::TexCoords(0)), accessors[0].uv);
                map
            },
            extensions: Default::default(),
            extras: Default::default(),
            indices: Some(accessors[0].indices),
            material: None,
            mode: Checked::Valid(gltf::mesh::Mode::Triangles),
            targets: None,
        };

        let plane_mesh = root.push(gltf::Mesh {
            extensions: Default::default(),
            extras: Default::default(),
            name: None,
            primitives: vec![plane_primitive],
            weights: None,
        });

        let mut nodes = Vec::new();
        for transform in mesh.planes {
            let node = root.push(gltf::Node {
                mesh: Some(plane_mesh),
                rotation: Some(gltf::scene::UnitQuaternion(transform.rotation.to_array())),
                scale: Some(transform.scale.to_array()),
                translation: Some(transform.translation.to_array()),
                ..Default::default()
            });
            nodes.push(node);
        }
        root.push(gltf::Scene {
            extensions: Default::default(),
            extras: Default::default(),
            name: None,
            nodes,
        });

        let _ = fs::create_dir("tilemap");
        let writer = fs::File::create("tilemap/tilemap.gltf").unwrap();
        gltf::serialize::to_writer_pretty(writer, &root).unwrap();

        let mut writer = fs::File::create("tilemap/buffer0.bin").unwrap();
        writer.write_all(&buf).unwrap();
        // let buffer_view


        // root.push

    }

    pub fn to_mesh(&self) -> TilemapMesh {
        let mut planes = Vec::new();

        for (pos, tile) in self.tiles.iter() {
            match tile.tile_type {
                TileType::Cube => {

                    for dir in Dir::DIRS {
                        let new_pos = *pos + dir.as_ivec3();
                        if self.tiles.get(&new_pos).is_none_or(|tile| !tile.covers_dir(&dir.inv())) {
                            let rotation = Quat::from_rotation_arc(Vec3::Y, *dir.as_dir3());
                            let translation = pos.as_vec3() + Vec3::splat(0.5) + 0.5 * dir.as_dir3();

                            let transform = Transform {
                                translation,
                                rotation,
                                ..Default::default()
                            };

                            planes.push(transform);

                            // let plane = Plane3d {
                            //     normal: dir.as_dir3(),
                            //     half_size: Vec2::splat(0.5),
                            // };

                            // let mut plane_mesh = plane.mesh().build();
                            // plane_mesh = plane_mesh.translated_by(pos.as_vec3() + Vec3::splat(0.5));
                            // meshes.push(plane_mesh);
                        }
                    }
                },
            }
        }

        TilemapMesh { planes }
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
enum Dir {
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
    pub entity: Entity,
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
