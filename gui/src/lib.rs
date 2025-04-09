use fsct_core::{definitions, run_player_watch, NoopPlayerEventListener};

use eframe::egui;
use std::sync::{Arc, Mutex};
use fsct_core::player::{Player, PlayerInterface, PlayerState};

struct PlayerApp {
    player: Player,
    state: Arc<Mutex<PlayerState>>,
    _runtime_handle: tokio::runtime::Handle,
}

impl PlayerApp {
    fn new(
        player: Player,
        state: Arc<Mutex<PlayerState>>,
        runtime_handle: tokio::runtime::Handle,
    ) -> Self {
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

                if let Some(title) = &state.texts.title {
                    ui.add_space(20.0);
                    ui.heading(title);
                }

                if let Some(artist) = &state.texts.artist {
                    ui.add_space(10.0);
                    ui.label(artist);
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
                    ui.label(if state.status == definitions::FsctStatus::Playing {
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
                        if state.status == definitions::FsctStatus::Playing {
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

pub async fn run_gui(player: Player) -> Result<(), String> {
    let player_state = Arc::new(Mutex::new(PlayerState::default()));


    run_player_watch(player.clone(), NoopPlayerEventListener::new(), player_state.clone()).await?;

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
            Ok(Box::new(PlayerApp::new(player, player_state, runtime_handle)))
        }),
    ).map_err(|e| e.to_string())
}
