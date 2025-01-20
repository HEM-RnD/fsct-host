mod platform;

use env_logger;

#[tokio::main]
async fn main() {
    env_logger::init();
    
    let platform = platform::get_platform();
    
    match platform.initialize().await {
        Ok(context) => {
            println!("Initialized platform: {}", platform.get_platform_name());
            
            // Example usage of the platform APIs
            match context.info.get_current_track().await {
                Ok(track) => {
                    println!("Current track: \n\tTitle: {} \n\tArtist: {}", track.title, track.artist);
                },
                Err(e) => eprintln!("Failed to get current track: {:?}", e),
            }
            
            match context.info.get_timeline_info().await {
                Ok(timeline) => {
                    if let Some(duration) = timeline.duration {
                        println!("\tDuration: {:.2} seconds", duration);
                    }
                    let time_diff = if timeline.is_playing {
                        timeline.update_time.elapsed().unwrap().as_secs_f64()
                    } else {
                        0.0
                    };
                    println!("\tPosition: {:.2} seconds", timeline.position + time_diff);
                    println!("\tMusic is currently {}playing", if timeline.is_playing { "" } else { "not " });
                },
                Err(e) => eprintln!("Failed to get timeline info: {:?}", e),
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
        Err(e) => eprintln!("Platform initialization error: {}", e),
    }

    if let Err(e) = platform.cleanup().await {
        eprintln!("Platform cleanup error: {}", e);
    }
}
