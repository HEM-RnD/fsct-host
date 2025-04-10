import {runFsct, NodePlayer, PlayerStatus} from './index.js'


const player = new NodePlayer();

runFsct(player)

await new Promise(resolve => setTimeout(resolve, 3000));

await player.setStatus(PlayerStatus.Playing);
await player.setTimeline(12.0, 340.0, 1.0);

await new Promise(resolve => setTimeout(resolve, 3000));

await player.setStatus(PlayerStatus.Stopped);
await player.setTimeline(15.0, 340.0, 0.0);

await new Promise(resolve => setTimeout(resolve, 3000));