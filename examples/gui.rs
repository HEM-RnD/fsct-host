use dac_player_integration::{definitions, platform, player};

use eframe::egui;
use env_logger;
use std::sync::{Arc, Mutex};
use dac_player_integration::player::{Player, PlayerInterface};

#[derive(Default)]
struct PlayerState {
    current_track: Option<player::Track>,
    timeline: Option<definitions::TimelineInfo>,
    is_playing: bool,
}

struct PlayerApp {
    player: Player,
    state: Arc<Mutex<PlayerState>>,
    _runtime_handle: tokio::runtime::Handle,
}

impl PlayerApp {
    fn new(
        player: Player,
        runtime_handle: tokio::runtime::Handle,
    ) -> Self {
        let state = Arc::new(Mutex::new(PlayerState::default()));

        let player_clone = player.clone();
        let state_clone = state.clone();
        runtime_handle.spawn(async move {
            let player = player_clone;
            let state = state_clone;
            loop {
                let mut state_local: PlayerState = PlayerState::default();

                if let Ok(track) = player.get_current_track().await {
                    state_local.current_track = Some(track);
                }

                if let Ok(timeline) = player.get_timeline_info().await {
                    state_local.timeline = timeline;
                }

                state_local.is_playing = player.is_playing().await.unwrap_or(false);

                *state.lock().unwrap() = state_local;
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        });

        Self {
            player,
            state,
            _runtime_handle: runtime_handle,
        }
    }
}

impl eframe::App for PlayerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let state = self.state.lock().unwrap();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Music Player");

                if let Some(track) = &state.current_track {
                    ui.add_space(20.0);
                    ui.heading(&track.title);
                    ui.label(&track.artist);
                }

                if let Some(timeline) = &state.timeline {
                    ui.add_space(10.0);

                    let time_diff = timeline.update_time.elapsed().unwrap_or_default().as_secs_f64() * timeline.rate as f64;

                    let current_pos = timeline.position + time_diff;

                    let progress = current_pos / timeline.duration;
                    let progress_bar = egui::ProgressBar::new(progress as f32)
                        .show_percentage()
                        .animate(timeline.rate > 0.0);
                    ui.add(progress_bar);

                    ui.label(format!(
                        "{:02}:{:02} / {:02}:{:02}",
                        (current_pos / 60.0) as i32,
                        (current_pos % 60.0) as i32,
                        (timeline.duration / 60.0) as i32,
                        (timeline.duration % 60.0) as i32
                    ));


                    ui.add_space(10.0);
                    ui.label(if state.is_playing {
                        "▶ Playing"
                    } else {
                        "⏸ Paused"
                    });

                    let runtime_handle = self._runtime_handle.clone();
                    ui.horizontal(|ui| {
                        if ui.button("⏮").clicked() {
                            let player = self.player.clone();
                            runtime_handle.spawn(async move {
                                let _ = player.previous_track().await;
                            });
                        }
                        if state.is_playing {
                            if ui.button("⏸").clicked() {
                                let player = self.player.clone();
                                runtime_handle.spawn(async move {
                                    let _ = player.pause().await;
                                });
                            }
                        } else {
                            if ui.button("▶").clicked() {
                                let player = self.player.clone();
                                runtime_handle.spawn(async move {
                                    let _ = player.play().await;
                                });
                            }
                        }
                        if ui.button("⏭").clicked() {
                            let player = self.player.clone();
                            runtime_handle.spawn(async move {
                                let _ = player.next_track().await;
                            });
                        }
                    });
                }
            });
        });

        ctx.request_repaint();
    }
}

#[tokio::main]
async fn main() -> Result<(), String> {
    env_logger::init();

    // Inicjalizacja platformy
    let platform = platform::get_platform();
    let player = platform.initialize().await?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([300.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Music Player",
        options,
        Box::new(|_cc| {
            let runtime_handle = tokio::runtime::Handle::current();
            Ok(Box::new(PlayerApp::new(player, runtime_handle)))
        }),
    ).map_err(|e| e.to_string())
}
