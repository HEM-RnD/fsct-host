use reqwest::Url;
use fsct_core::Player;
use fsct_core::player::PlayerError;

mod rest_api;

pub async fn create_rest_api_volumio_player(url: &str) -> Result<Player, PlayerError> {
    let url = Url::parse(url).map_err(|e| PlayerError::Other(e.into()))?;
    let rest_api_player = rest_api::RestApiVolumioPlayer::new(url.into()).await?;
    Ok(Player::new(rest_api_player))
}