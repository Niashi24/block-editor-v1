use std::path::PathBuf;

use bevy::{app::Plugin, ecs::prelude::*};
use crossbeam_channel::{Receiver, Sender};

pub struct FileDialogPlugin;

impl Plugin for FileDialogPlugin {
    fn build(&self, app: &mut bevy::app::App) {

    }
}

enum Finished {
    OpenFile(Option<PathBuf>),
}

#[derive(Resource)]
struct FileChannel {
    finished_sender: Sender<Finished>,
    finished_receiver: Receiver<Finished>,

}
