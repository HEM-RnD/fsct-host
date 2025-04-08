use async_trait::async_trait;
use reqwest::Url;
use fsct_core::definitions::TimelineInfo;
use fsct_core::player::{PlayerError, PlayerInterface, Track};

pub struct VolumioPlayer {
    url: Url,
}

impl VolumioPlayer {
    pub async fn new(url: Url) -> Result<Self, PlayerError> {
        Ok(VolumioPlayer { url })
    }

    async fn get_state(&self) -> Result<serde_json::Value, PlayerError>
    {
        let info_url = self.url.join("api/v1/getState").unwrap();
        let response = reqwest::get(info_url).await.map_err(|e| PlayerError::UnknownError(e.to_string()))?;
        let response = response.error_for_status().map_err(|e| PlayerError::UnknownError(e.to_string()))?;
        let response_text = response.text().await.map_err(|e| PlayerError::UnknownError(e.to_string()))?;
        let json_value = serde_json::from_str(&response_text).map_err(|e| PlayerError::UnknownError(e.to_string()))?;
        Ok(json_value)
    }

    async fn send_command(&self, command: &str) -> Result<(), PlayerError>
    {
        let info_url = self.url.join(format!("api/v1/commands/?cmd={command}").as_str()).unwrap();
        let response = reqwest::get(info_url).await.map_err(|e| PlayerError::UnknownError(e.to_string()))?;
        let _response = response.error_for_status().map_err(|e| PlayerError::UnknownError(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl PlayerInterface for VolumioPlayer {
    async fn get_current_track(&self) -> Result<Track, PlayerError> {
        let state = self.get_state().await?;
        let title = state["title"].as_str().unwrap_or_default();
        let artist = state["artist"].as_str().unwrap_or_default();
        Ok(Track {
            title: title.to_string(),
            artist: artist.to_string(),
        })
    }

    async fn get_timeline_info(&self) -> Result<Option<TimelineInfo>, PlayerError> {
        let state = self.get_state().await?;
        let position = state["seek"].as_u64().unwrap_or(0);
        let duration = state["duration"].as_u64().unwrap_or(0);
        let status = state["status"].as_str().unwrap_or("stop");
        let default_rate = if status == "play" { 1.0 } else { 0.0 };
        let rate = state["rate"].as_f64().unwrap_or(default_rate);
        Ok(Some(TimelineInfo {
            position: position as f64 / 1000.0,
            update_time: std::time::SystemTime::now(),
            duration: duration as f64,
            rate: rate as f32,
        }))
    }

    async fn is_playing(&self) -> Result<bool, PlayerError> {
        let state = self.get_state().await?;
        let status = state["status"].as_str().unwrap_or("stop");
        Ok(status == "play")
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