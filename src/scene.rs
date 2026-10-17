use std::{fs::File, io::BufWriter};

use bevy::{math::{IVec3, Isometry3d}, platform::collections::HashMap};
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt, Whatever, whatever};
use sscanf::sscanf;

use crate::tilemap::{TileData, TilemapData, TilemapImage};


#[derive(Clone, Serialize, Deserialize)]
pub struct Scene {
    pub tilemap_data: TilemapData,
    pub camera: Isometry3d,
}

impl Default for Scene {
    fn default() -> Self {
        Self { tilemap_data: Default::default(), camera: Default::default() }
    }
}

impl Scene {

    pub fn save(&self) -> Result<(), Whatever> {
        std::fs::create_dir_all("assets/scenes").whatever_context("create dir")?;

        let file = File::create("assets/scenes/scene.ron").whatever_context("create file")?;
        let mut file = BufWriter::new(file);
        ron::Options::default().to_io_writer(&mut file, &self).with_whatever_context(|err| format!("write ron: {err}"))?;

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

        let scene = ron::Options::default().from_reader::<_, Scene>(file).with_whatever_context(|err| format!("deserialize ron: {err}"))?;

        Ok(scene)
    }
}
