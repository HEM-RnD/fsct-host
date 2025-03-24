use fsct_service::platform;

use eframe::egui;
use env_logger;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct PlayerState {
    current_track: Option<platform::Track>,
    timeline: Option<platform::TimelineInfo>,
}

struct PlayerApp {
    platform: Box<dyn platform::PlatformBehavior>,
    control: Arc<dyn platform::PlaybackControlProvider>,
    state: Arc<Mutex<PlayerState>>,
    _runtime_handle: tokio::runtime::Handle,
}

impl PlayerApp {
    fn new(
        platform: Box<dyn platform::PlatformBehavior>,
        context: platform::PlatformContext,
        runtime_handle: tokio::runtime::Handle,
    ) -> Self {
        let state = Arc::new(Mutex::new(PlayerState::default()));
        
        let control = context.control.clone();
        let info = context.info.clone();
        
        let state_clone = state.clone();
        runtime_handle.spawn(async move {
            loop {
                let mut state : PlayerState = PlayerState::default();
                
                // Aktualizacja informacji o utworze
                if let Ok(track) = info.get_current_track().await {
                    state.current_track = Some(track);
                }

                // Aktualizacja informacji o odtwarzaniu
                if let Ok(timeline) = info.get_timeline_info().await {
                    state.timeline = Some(timeline);
                }
                
                *state_clone.lock().unwrap() = state;
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        });

        Self {
            platform,
            control,
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

                    if let Some(duration) = timeline.duration {
                        let time_diff = timeline.update_time.elapsed().unwrap_or_default().as_secs_f64() * timeline.rate as f64;

                        let current_pos = timeline.position + time_diff;

                        let progress = current_pos / duration;
                        let progress_bar = egui::ProgressBar::new(progress as f32)
                            .show_percentage()
                            .animate(timeline.is_playing);
                        ui.add(progress_bar);

                        ui.label(format!(
                            "{:02}:{:02} / {:02}:{:02}",
                            (current_pos / 60.0) as i32,
                            (current_pos % 60.0) as i32,
                            (duration / 60.0) as i32,
                            (duration % 60.0) as i32
                        ));
                    }

                    ui.add_space(10.0);
                    ui.label(if timeline.is_playing {
                        "▶ Playing"
                    } else {
                        "⏸ Paused"
                    });

                    let runtime_handle = self._runtime_handle.clone();
                    ui.horizontal(|ui| {
                        if ui.button("⏮").clicked() {
                            let control = self.control.clone();
                            runtime_handle.spawn(async move {
                                let _ = control.previous_track().await;
                            });
                        }
                        if timeline.is_playing {
                            if ui.button("⏸").clicked() {
                                let control = self.control.clone();
                                runtime_handle.spawn(async move {
                                    let _ = control.pause().await;
                                });
                            }
                        } else {
                            if ui.button("▶").clicked() {
                                let control = self.control.clone();
                                runtime_handle.spawn(async move {
                                    let _ = control.play().await;
                                });
                            }
                        }
                        if ui.button("⏭").clicked() {
                            let control = self.control.clone();
                            runtime_handle.spawn(async move {
                                let _ = control.next_track().await;
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
    let context = platform.initialize().await?;
    
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
            Ok(Box::new(PlayerApp::new(platform, context, runtime_handle)))
        })
    ).map_err(|e| e.to_string())
}
