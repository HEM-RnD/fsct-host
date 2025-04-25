import {
    LogLevelFilter,
    setLogLevel,
    initSystemdLogger,
    FsctService,
    NodePlayer,
    PlayerStatus,
    CurrentTextMetadata
} from './index.js'


const player = new NodePlayer();
const fsctService = new FsctService();

initSystemdLogger("fsct-test");
setLogLevel(LogLevelFilter.Info);

console.log("Starting FSCT")
await fsctService.runFsct(player);

await new Promise(resolve => setTimeout(resolve, 3000));

player.setStatus(PlayerStatus.Playing);
player.setTimeline({position: 12.0, duration: 340.0, rate: 1.0});
player.setText(CurrentTextMetadata.Title, "Test Title")
player.setText(CurrentTextMetadata.Author, "Test Artist")

await new Promise(resolve => setTimeout(resolve, 3000));

player.setStatus(PlayerStatus.Stopped);
player.setTimeline({position: 15.0, duration: 340.0, rate: 0.0});

// await new Promise(resolve => setTimeout(resolve, 3000));
// player.setText(CurrentTextMetadata.Title, null)
// player.setText(CurrentTextMetadata.Author, null)
// player.setTimeline(null)
// player.setStatus(PlayerStatus.Unknown)
await new Promise(resolve => setTimeout(resolve, 1000));

console.log("Stopping FSCT")
await fsctService.stopFsct()

await new Promise(resolve => setTimeout(resolve, 10000));
console.log("Starting FSCT")
await fsctService.runFsct(player)

await new Promise(resolve => setTimeout(resolve, 3000));

player.setStatus(PlayerStatus.Playing);
player.setTimeline({position: 12.0, duration: 340.0, rate: 1.0});
player.setText(CurrentTextMetadata.Title, "Test Title")
player.setText(CurrentTextMetadata.Author, "Test Artist")

await new Promise(resolve => setTimeout(resolve, 3000));

player.setStatus(PlayerStatus.Stopped);
player.setTimeline({position: 15.0, duration: 340.0, rate: 0.0});

await new Promise(resolve => setTimeout(resolve, 3000));
player.setText(CurrentTextMetadata.Title, null)
player.setText(CurrentTextMetadata.Author, null)
player.setTimeline(null)
player.setStatus(PlayerStatus.Unknown)

await new Promise(resolve => setTimeout(resolve, 1000));

console.log("Done")