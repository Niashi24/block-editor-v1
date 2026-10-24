use std::{fs::File, io::BufWriter};

use bevy::{ecs::{resource::Resource, system::EntityCommand, world::EntityWorldMut}, math::{IVec3, Isometry3d}};
use ron::{extensions::Extensions, ser::PrettyConfig};
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt, Whatever, whatever};
use tiny_bail::or_return;

use crate::tilemap::TilemapData;

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
        let history = self.history.iter()
            .map(|x| x.clone_dyn())
            .collect();

        let redo = self.redo.iter()
            .map(|x| x.clone_dyn())
            .collect();

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

#[typetag::serde(tag = "type")]
pub trait Redo: Send + Sync + 'static {
    fn redo(self: Box<Self>, tilemap: EntityWorldMut) -> Box<dyn Undo>;
    fn clone_dyn(&self) -> Box<dyn Redo>;
}

#[typetag::serde(tag = "type")]
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
    pub fn save(&self) -> Result<(), Whatever> {
        std::fs::create_dir_all("assets/scenes").whatever_context("create dir")?;

        let file = File::create("assets/scenes/scene.ron").whatever_context("create file")?;
        let mut file = BufWriter::new(file);
        ron::Options::default()
            .with_default_extension(Extensions::all().difference(Extensions::EXPLICIT_STRUCT_NAMES))
            .to_io_writer_pretty(&mut file, &self, PrettyConfig::new())
            .with_whatever_context(|err| format!("write ron: {err}"))?;

        println!("Successfully saved.");

        Ok(())
    }

    pub fn load() -> Result<Self, Whatever> {
        let file = std::fs::File::open("assets/scenes/scene.ron");
        let file = match file {
            Ok(file) => file,
            Err(err) => {
                if err.kind() == std::io::ErrorKind::NotFound {
                    println!("tilemap not found. creating new tilemap");
                    return Ok(Scene::default());
                }
                return whatever!(Err(err), "");
            }
        };

        let scene = ron::Options::default()
            .with_default_extension(Extensions::all().difference(Extensions::EXPLICIT_STRUCT_NAMES))
            .from_reader::<_, Scene>(file)
            .with_whatever_context(|err| format!("deserialize ron: {err}"))?;

        Ok(scene)
    }
}
