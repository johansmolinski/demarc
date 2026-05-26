#![allow(dead_code, clippy::too_many_arguments, clippy::type_complexity)]
use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResource;
use bevy_tweening::TweeningPlugin;
use clap::Parser;

#[allow(warnings)]
mod libretro;

mod hud;
mod post_process;
mod retro;
mod retro_emu;

use hud::HudPlugin;
use post_process::{BorderMode, PostProcessPlugin, ScaleMode};
use retro::RetroPlugin;

#[derive(Parser, Debug, Resource, Clone)]
#[command(name = "rupix", about = "Bevy + libretro front-end")]
struct Args {
    /// Path to the program/ROM to load
    games: Vec<PathBuf>,

    /// How to map the low-res render target onto the window.
    #[arg(long, value_enum, default_value_t = ScaleModeArg::Fit)]
    scale: ScaleModeArg,

    /// How to fill the border outside the image (letterbox/pillarbox bars).
    #[arg(long, value_enum, default_value_t = BorderModeArg::Black)]
    border: BorderModeArg,

    /// Shuffle the list of games into a random order.
    #[arg(long)]
    shuffle: bool,
}

#[derive(Copy, Clone, Debug, clap::ValueEnum)]
enum ScaleModeArg {
    /// Fill the window, distorting the aspect ratio.
    Stretch,
    /// Preserve aspect ratio, adding letterbox/pillarbox bars.
    Fit,
    /// Preserve aspect ratio, cropping top/bottom or left/right to fill.
    Zoom,
}

impl From<ScaleModeArg> for ScaleMode {
    fn from(s: ScaleModeArg) -> Self {
        match s {
            ScaleModeArg::Stretch => ScaleMode::Stretch,
            ScaleModeArg::Fit => ScaleMode::Fit,
            ScaleModeArg::Zoom => ScaleMode::Zoom,
        }
    }
}

#[derive(Copy, Clone, Debug, clap::ValueEnum)]
enum BorderModeArg {
    /// Stretch the edge pixels outward into the border.
    Stretch,
    /// Fill the border with black.
    Black,
}

impl From<BorderModeArg> for BorderMode {
    fn from(b: BorderModeArg) -> Self {
        match b {
            BorderModeArg::Stretch => BorderMode::Stretch,
            BorderModeArg::Black => BorderMode::Black,
        }
    }
}

#[derive(Resource, Clone, ExtractResource)]
struct AppSettings {
    border_mode: BorderMode,
    scale_mode: ScaleMode,
}

fn main() {
    let mut args = Args::parse();

    if args.shuffle {
        use rand::seq::SliceRandom;
        args.games.shuffle(&mut rand::rng());
    }

    tracing_subscriber::fmt().with_target(true).compact().init();
    let primary_window = Some(Window {
        title: "Rupix".into(),
        //mode: WindowMode::BorderlessFullscreen(MonitorSelection::Current),
        resolution: (366 * 3, 280 * 3).into(),
        resizable: false,
        ..Default::default()
    });

    let settings = AppSettings {
        border_mode: args.border.into(),
        scale_mode: args.scale.into(),
    };

    App::new()
        .insert_resource(args)
        .insert_resource(settings)
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window,
                ..Default::default()
            }),
            RetroPlugin {},
            PostProcessPlugin,
            TweeningPlugin,
            HudPlugin,
        ))
        .run();
}
