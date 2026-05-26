use std::time::Duration;

use bevy::prelude::*;
use bevy_tweening::lens::TextColorLens;
use bevy_tweening::{CycleCompletedEvent, Delay, Tween, TweenAnim};

#[derive(Component)]
pub struct InfoText;

#[derive(Message)]
pub struct SpawnToast {
    pub text: String,
    pub delay: Duration,
    pub duration: Duration,
}

#[derive(Resource, Default)]
struct HudState {
    current_toast: Option<Entity>,
}

fn spawn_toast(
    mut commands: Commands,
    mut state: ResMut<HudState>,
    mut reader: MessageReader<SpawnToast>,
) {
    for msg in reader.read() {
        if let Some(toast) = state.current_toast {
            commands.entity(toast).despawn();
        }
        let tween0 = Tween::new(
            EaseFunction::QuadraticInOut,
            Duration::from_secs(1),
            TextColorLens {
                start: Color::srgba(0., 0., 0., 0.),
                end: Color::WHITE,
            },
        );
        let tween = Tween::new(
            EaseFunction::QuadraticInOut,
            Duration::from_secs(1),
            TextColorLens {
                start: Color::WHITE,
                end: Color::srgba(0., 0., 0., 0.),
            },
        )
        .with_cycle_completed_event(true);

        let delayed = Delay::new(msg.delay)
            .then(tween0)
            .then(Delay::new(msg.duration))
            .then(tween);

        let entity = commands.spawn((
            Node {
                //width: Val::Px(400.0),
                position_type: PositionType::Absolute,
                bottom: Val::Px(0.0),
                right: Val::Px(0.0),
                margin: UiRect::all(Val::Px(60.0)),
                ..default()
            },
            Text::new(&msg.text),
            InfoText,
            TextFont {
                font_size: 48.0,
                ..default()
            },
            TextColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
            TextLayout {
                justify: Justify::Right,
                linebreak: LineBreak::WordBoundary,
            },
            TweenAnim::new(delayed),
        ));
        state.current_toast = Some(entity.id());
    }
}

fn handle_tween_done(mut commands: Commands, mut reader: MessageReader<CycleCompletedEvent>) {
    for msg in reader.read() {
        info!("DESPAWN");
        commands.entity(msg.anim_entity).despawn();
    }
}

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SpawnToast>()
            .insert_resource(HudState::default())
            .add_systems(
                Update,
                (
                    spawn_toast.run_if(on_message::<SpawnToast>),
                    handle_tween_done.run_if(on_message::<CycleCompletedEvent>),
                ),
            );
    }
}
