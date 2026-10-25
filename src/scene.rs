use std::{
    collections::VecDeque,
    fs::File,
    io::{BufWriter, ErrorKind},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use bevy::{
    app::{Plugin, PostStartup, Update},
    asset::AssetServer,
    camera::Camera,
    ecs::prelude::*,
    image::Image,
    input::{ButtonInput, keyboard::KeyCode},
    math::Isometry3d,
    pbr::{MeshMaterial3d, StandardMaterial},
    tasks::AsyncComputeTaskPool,
    transform::components::Transform,
};
use ron::{extensions::Extensions, ser::PrettyConfig};
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Whatever, whatever};
use tiny_bail::or_return;

use crate::tilemap::{self, Tilemap, TilemapData, TilemapImage};

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut bevy::app::App) {
        let save_data = match std::fs::read_to_string("editor.toml") {
            Ok(file) => toml::from_str::<EditorSaveData>(&file).unwrap(),
            Err(err) if err.kind() == ErrorKind::NotFound => {
                let save_data = EditorSaveData::default();

                let file = toml::to_string_pretty(&save_data).unwrap();
                std::fs::write("editor.toml", &file).unwrap();

                save_data
            }
            Err(err) => panic!("error reading save data: {err}"),
        };

        app.insert_resource(EditorData {
            save_data,
            current_scene_path: None,
        })
        .add_systems(PostStartup, Self::load_latest_scene)
        .add_systems(Update, (Self::load_keybind, Self::save_keybind));
    }
}

impl ScenePlugin {
    fn load_latest_scene(
        commands: Commands,
        mut editor_data: ResMut<EditorData>,
        tilemap_entity: Single<(Entity, &mut Tilemap)>,
        camera_entity: Single<&mut Transform, With<Camera>>,
        asset_server: Res<AssetServer>,
        history: ResMut<History>,
    ) {
        let path = or_return!(editor_data.save_data.recent_scenes.front()).clone();
        editor_data.current_scene_path = Some(path);

        Self::load(
            commands,
            editor_data,
            tilemap_entity,
            camera_entity,
            asset_server,
            history,
        )
    }

    fn load_keybind(
        commands: Commands,
        mut editor_data: ResMut<EditorData>,
        tilemap_entity: Single<(Entity, &mut Tilemap)>,
        camera_entity: Single<&mut Transform, With<Camera>>,
        asset_server: Res<AssetServer>,
        input: Res<ButtonInput<KeyCode>>,
        history: ResMut<History>,
        mut selection: Local<Option<Arc<Mutex<SelectionState>>>>,
    ) {
        if let Some(path) = selection.as_mut() {
            let mut reset = false;

            if let SelectionState::Ready(path) = &mut *or_return!(path.lock()) {
                if let Some(path) = path.take() {
                    editor_data.set_scene_path(path);
                    Self::load(
                        commands,
                        editor_data,
                        tilemap_entity,
                        camera_entity,
                        asset_server,
                        history,
                    );
                }

                reset = true;
            }

            if reset {
                *selection = None;
            }
        } else {
            if input.pressed(KeyCode::ControlLeft) && input.just_pressed(KeyCode::KeyO) {
                let select = Arc::new(Mutex::new(SelectionState::Waiting));
                *selection = Some(select.clone());
                AsyncComputeTaskPool::get()
                    .spawn(async move {
                        let file = rfd::AsyncFileDialog::new()
                            .add_filter("Scene (.ron)", &["ron"])
                            .set_directory("assets")
                            .pick_file()
                            .await;

                        *select.lock().unwrap() =
                            SelectionState::Ready(file.map(|handle| handle.path().to_path_buf()));
                    })
                    .detach();
                // if editor_data.current_scene_path.is_some() && !input.pressed(KeyCode::ShiftLeft) {
                //     Self::load(commands, editor_data, tilemap_entity, camera_entity, asset_server, history);
                // } else {
                // }
            }
        }
    }

    fn load(
        mut commands: Commands,
        mut editor_data: ResMut<EditorData>,
        tilemap_entity: Single<(Entity, &mut Tilemap)>,
        mut camera_entity: Single<&mut Transform, With<Camera>>,
        asset_server: Res<AssetServer>,
        mut history: ResMut<History>,
    ) {
        let path = or_return!(editor_data.current_scene_path.as_ref());

        let Scene {
            camera,
            history: saved_history,
            mut tilemap_data,
        } = or_return!(Scene::load(path));

        tilemap_data.image = Some(TilemapImage {
            path: String::from("atlas.png"),
            image_width: 160,
            image_height: 110,
            tile_width: 16,
            tile_height: 11,
        });

        let (tilemap_entity, mut tilemap) = tilemap_entity.into_inner();

        *tilemap = Tilemap::default();

        let img = match &tilemap_data.image {
            Some(img) => asset_server.load::<Image>(&img.path),
            None => asset_server.load("PrototypeTextures/texture_08.png"),
        };

        let mat = asset_server.add(StandardMaterial::from(img));

        let mut tilemap_commands = commands.entity(tilemap_entity);

        tilemap_commands.insert(MeshMaterial3d(mat));

        tilemap.data.image = tilemap_data.image;

        for (tile, tile_data) in tilemap_data.tiles.into_iter() {
            match tile_data.tile_type {
                tilemap::TileType::Cube(_) => {
                    let _ = tilemap.insert(tile, tile_data);
                }
            }
        }

        *history = saved_history;
        **camera_entity = Transform::from_isometry(camera);
        editor_data.current_scene_path = Some(path.to_path_buf());
    }

    fn save_keybind(
        mut editor_data: ResMut<EditorData>,
        tilemap_entity: Single<&Tilemap>,
        camera_entity: Single<&Transform, With<Camera>>,
        input: Res<ButtonInput<KeyCode>>,
        history: ResMut<History>,
        mut selection: Local<Option<Arc<Mutex<SelectionState>>>>,
    ) {
        if let Some(path) = selection.as_ref() {
            // borrow checking shenanigans...
            let mut reset = false;

            if let SelectionState::Ready(path) = &mut *or_return!(path.lock()) {
                if let Some(path) = path.take() {
                    editor_data.current_scene_path = Some(path.clone());
                    editor_data.save_data.recent_scenes.push_front(path);
                    editor_data.save_data.save();

                    Self::save(&*editor_data, &*tilemap_entity, &*camera_entity, &*history);
                }

                reset = true;
            }

            if reset {
                *selection = None;
            }
        } else {
            if input.pressed(KeyCode::ControlLeft) && input.just_pressed(KeyCode::KeyS) {
                if editor_data.current_scene_path.is_some() && !input.pressed(KeyCode::ShiftLeft) {
                    Self::save(&*editor_data, &*tilemap_entity, &*camera_entity, &*history);
                } else {
                    let select = Arc::new(Mutex::new(SelectionState::Waiting));
                    *selection = Some(select.clone());
                    AsyncComputeTaskPool::get()
                        .spawn(async move {
                            let file = rfd::AsyncFileDialog::new()
                                .add_filter("Scene (.ron)", &["ron"])
                                .set_directory("assets")
                                .save_file()
                                .await;

                            *select.lock().unwrap() = SelectionState::Ready(
                                file.map(|handle| handle.path().to_path_buf()),
                            );
                        })
                        .detach();
                }
            }
        }

        if input.pressed(KeyCode::ControlLeft) && input.just_pressed(KeyCode::KeyS) {}
    }

    fn save(
        editor_data: &EditorData,
        tilemap_entity: &Tilemap,
        camera_transform: &Transform,
        history: &History,
    ) {
        let path = or_return!(editor_data.current_scene_path.as_ref());
        let tilemap_data = tilemap_entity.data.clone();
        let camera = camera_transform.to_isometry();
        let scene = Scene {
            tilemap_data,
            camera,
            history: history.clone(),
        };
        let result = scene.save(path);
        if let Err(err) = result {
            eprintln!("{err}");
            eprintln!("{}", err.backtrace());
        }
    }
}

enum SelectionState {
    Waiting,
    Ready(Option<PathBuf>),
}

#[derive(Clone, Debug, Resource)]
pub struct EditorData {
    save_data: EditorSaveData,
    current_scene_path: Option<PathBuf>,
}

impl EditorData {
    pub fn set_scene_path(&mut self, path: PathBuf) {
        let _ = self.current_scene_path.insert(path.clone());
        self.save_data.recent_scenes.retain(|x| x != &path);
        self.save_data.recent_scenes.push_front(path);
        self.save_data.save();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct EditorSaveData {
    pub recent_scenes: VecDeque<PathBuf>,
}
impl EditorSaveData {
    fn save(&self) {
        let file = toml::to_string_pretty(&self).unwrap();
        std::fs::write("editor.toml", &file).unwrap();
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Scene {
    pub camera: Isometry3d,
    pub history: History,
    pub tilemap_data: TilemapData,
}

#[derive(Default, Serialize, Deserialize, Resource)]
pub struct History {
    history: Vec<Box<dyn Undo>>,
    redo: Vec<Box<dyn Redo>>,
}

impl Clone for History {
    fn clone(&self) -> Self {
        let history = self.history.iter().map(|x| x.clone_dyn()).collect();

        let redo = self.redo.iter().map(|x| x.clone_dyn()).collect();

        Self { history, redo }
    }
}

impl History {
    pub fn queue(&mut self, action: Box<dyn Undo>) {
        self.history.push(action);
        self.redo.clear();
    }

    pub fn undo() -> impl EntityCommand {
        move |mut tilemap_entity: EntityWorldMut<'_>| {
            let to_undo = or_return!(tilemap_entity.resource_mut::<History>().history.pop());

            let redo = tilemap_entity.reborrow_scope(|tilemap_entity| to_undo.undo(tilemap_entity));

            tilemap_entity.resource_mut::<History>().redo.push(redo);
        }
    }

    pub fn redo() -> impl EntityCommand {
        move |mut tilemap_entity: EntityWorldMut<'_>| {
            let to_redo = or_return!(tilemap_entity.resource_mut::<History>().redo.pop());

            let undo = tilemap_entity.reborrow_scope(|tilemap_entity| to_redo.redo(tilemap_entity));

            tilemap_entity.resource_mut::<History>().history.push(undo);
        }
    }
}

#[typetag::serde]
pub trait Redo: Send + Sync + 'static {
    fn redo(self: Box<Self>, tilemap: EntityWorldMut) -> Box<dyn Undo>;
    fn clone_dyn(&self) -> Box<dyn Redo>;
}

#[typetag::serde]
pub trait Undo: Send + Sync + 'static {
    fn undo(self: Box<Self>, tilemap: EntityWorldMut) -> Box<dyn Redo>;
    fn clone_dyn(&self) -> Box<dyn Undo>;
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            tilemap_data: Default::default(),
            camera: Default::default(),
            history: History::default(),
        }
    }
}

impl Scene {
    pub fn save(&self, path: &Path) -> Result<(), Whatever> {
        let file = File::create(path).whatever_context("create file")?;
        let mut file = BufWriter::new(file);
        ron::Options::default()
            // .with_default_extension(Extensions::IMPLICIT_SOME)
            .to_io_writer_pretty(&mut file, &self, PrettyConfig::new())
            .with_whatever_context(|err| format!("write ron: {err}"))?;

        println!("Successfully saved.");

        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, Whatever> {
        let file = std::fs::File::open(path);
        let file = match file {
            Ok(file) => file,
            Err(err) => {
                // if err.kind() == std::io::ErrorKind::NotFound {
                //     println!("tilemap not found. creating new tilemap");
                //     return Ok(Scene::default());
                // }
                return whatever!(Err(err), "");
            }
        };

        let scene = ron::Options::default()
            .with_default_extension(Extensions::IMPLICIT_SOME)
            .from_reader::<_, Scene>(file)
            .with_whatever_context(|err| format!("deserialize ron: {err}"))?;

        Ok(scene)
    }
}
