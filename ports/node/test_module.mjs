import {runFsct, NodePlayer, PlayerStatus, CurrentTextMetadata, TimelineInfo} from './index.js'


const player = new NodePlayer();

runFsct(player)

await new Promise(resolve => setTimeout(resolve, 3000));

await player.setStatus(PlayerStatus.Playing);
await player.setTimeline(new TimelineInfo(12.0, 340.0, 1.0));
await player.setText(CurrentTextMetadata.Title, "Test Title")
await player.setText(CurrentTextMetadata.Author, "Test Artist")

await new Promise(resolve => setTimeout(resolve, 3000));

await player.setStatus(PlayerStatus.Stopped);
await player.setTimeline(new TimelineInfo(15.0, 340.0, 0.0));

await new Promise(resolve => setTimeout(resolve, 3000));
await player.setText(CurrentTextMetadata.Title, null)
await player.setText(CurrentTextMetadata.Author, null)
await player.setTimeline(null)
await player.setStatus(PlayerStatus.Unknown)
await new Promise(resolve => setTimeout(resolve, 1000));
