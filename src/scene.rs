use std::{fs::File, io::BufWriter};

use bevy::{math::{IVec3, Isometry3d}, platform::collections::HashMap};
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt, Whatever, whatever};
use sscanf::sscanf;

use crate::tilemap::{TileData, TilemapData};


#[derive(Clone)]
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

        let file = File::create("assets/scenes/scene.json").whatever_context("create file")?;
        let mut file = BufWriter::new(file);
        let scene = SceneSaved::from(self.clone());
        serde_json::ser::to_writer(&mut file, &scene).with_whatever_context(|err| format!("write json: {err}"))?;

        println!("Successfully saved.");

        Ok(())
    }

    pub fn load() -> Result<Self, Whatever> {
        let file = std::fs::File::open("assets/scenes/scene.json");
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

        let scene_saved = serde_json::from_reader::<_, SceneSaved>(file).with_whatever_context(|err| format!("deserialize json: {err}"))?;
        let scene = Scene::try_from(scene_saved).with_whatever_context(|err| format!("deserialize saved: {err}"))?;

        Ok(scene)
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct SceneSaved {
    tilemap_data: TilemapSaved,
    camera: Isometry3d,
}

impl From<Scene> for SceneSaved {
    fn from(value: Scene) -> Self {
        let tilemap_data = TilemapSaved::from(value.tilemap_data);
        Self {
            tilemap_data,
            camera: value.camera
        }
    }
}

impl TryFrom<SceneSaved> for Scene {
    type Error = Whatever;

    fn try_from(value: SceneSaved) -> Result<Self, Self::Error> {
        let tilemap_data = TilemapData::try_from(value.tilemap_data)?;

        Ok(Self {
            tilemap_data,
            camera: value.camera,
        })
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct TilemapSaved {
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
