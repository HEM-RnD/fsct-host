use std::time::Duration;
use async_trait::async_trait;
use fsct_core::definitions::{FsctStatus, TimelineInfo};
use fsct_core::player::{PlayerError, PlayerInterface, TrackMetadata};
use reqwest::Url;

pub struct RestApiVolumioPlayer {
    url: Url,
}

impl RestApiVolumioPlayer {
    pub async fn new(url: Url) -> Result<Self, PlayerError> {
        Ok(RestApiVolumioPlayer { url })
    }

    async fn get_state(&self) -> Result<serde_json::Value, PlayerError>
    {
        let info_url = self.url.join("api/v1/getState").unwrap();
        let response = reqwest::get(info_url).await.map_err(|e| PlayerError::Other(e.into()))?;
        let response = response.error_for_status().map_err(|e| PlayerError::Other(e.into()))?;
        let response_text = response.text().await.map_err(|e| PlayerError::Other(e.into()))?;
        println!("Response: {}", response_text);
        let json_value = serde_json::from_str(&response_text).map_err(|e| PlayerError::Other(e.into()))?;
        Ok(json_value)
    }


    //
    async fn send_command(&self, command: &str) -> Result<(), PlayerError>
    {
        let info_url = self.url.join(format!("api/v1/commands/?cmd={command}").as_str()).unwrap();
        let response = reqwest::get(info_url).await.map_err(|e| PlayerError::Other(e.into()))?;
        let _response = response.error_for_status().map_err(|e| PlayerError::Other(e.into()))?;
        Ok(())
    }
}

fn get_current_track(state: &serde_json::Value) -> TrackMetadata {
    let mut texts = TrackMetadata::default();
    texts.title = state["title"].as_str().map(|s| s.to_string());
    texts.artist = state["artist"].as_str().map(|s| s.to_string());
    texts.album = state["album"].as_str().map(|s| s.to_string());

    texts
}

fn get_timeline_info(state: &serde_json::Value) -> Option<TimelineInfo> {
    let position = state["seek"].as_u64()?;
    let duration = state["duration"].as_u64()?;
    let status = state["status"].as_str().unwrap_or("stop");
    let rate = if status == "play" { 1.0 } else { 0.0 };
    Some(TimelineInfo {
        position: Duration::from_millis(position),
        update_time: std::time::SystemTime::now(),
        duration: Duration::from_secs(duration),
        rate,
    })
}

fn get_status(state: &serde_json::Value) -> FsctStatus {
    match state["status"].as_str().unwrap_or("stop") {
        "play" => FsctStatus::Playing,
        "pause" => FsctStatus::Paused,
        "stop" => FsctStatus::Stopped,
        _ => FsctStatus::Unknown,
    }
}
#[async_trait]
impl PlayerInterface for RestApiVolumioPlayer {
    async fn get_current_state(&self) -> Result<fsct_core::player::PlayerState, PlayerError> {
        let state = self.get_state().await?;
        let texts = get_current_track(&state);
        let timeline = get_timeline_info(&state);
        let status = get_status(&state);
        Ok(fsct_core::player::PlayerState {
            status,
            timeline,
            texts,
        })
    }

    async fn play(&self) -> Result<(), PlayerError> {
        self.send_command("play").await
    }

    async fn pause(&self) -> Result<(), PlayerError> {
        self.send_command("pause").await
    }

    async fn stop(&self) -> Result<(), PlayerError> {
        self.send_command("stop").await
    }

    async fn next_track(&self) -> Result<(), PlayerError> {
        self.send_command("next").await
    }

    async fn previous_track(&self) -> Result<(), PlayerError> {
        self.send_command("prev").await
    }
}