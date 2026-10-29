import type { Page } from "@playwright/test";

type MicrophoneWindow = Window & {
  __hyzMicrophoneTracks: MediaStreamTrack[];
};

// Generated audio covers track ownership, not microphone permission or hardware capture.
export async function installMicrophoneCapture(
  page: Page,
  { generatedAudio = false }: { generatedAudio?: boolean } = {},
) {
  await page.addInitScript((generated: boolean) => {
    const original = MediaDevices.prototype.getUserMedia;
    const tracks: MediaStreamTrack[] = [];
    let audioContext: AudioContext | undefined;
    (window as MicrophoneWindow).__hyzMicrophoneTracks = tracks;

    // WASM WebIDL bindings call the prototype method directly.
    MediaDevices.prototype.getUserMedia = async function (constraints) {
      let stream: MediaStream;
      if (generated) {
        if (!constraints?.audio || constraints.video) {
          throw new Error("Generated microphone capture requires audio only");
        }
        audioContext ??= new AudioContext();
        stream = audioContext.createMediaStreamDestination().stream;
      } else {
        stream = await original.call(this, constraints);
      }
      tracks.push(...stream.getAudioTracks());
      return stream;
    };
  }, generatedAudio);
}

export async function readMicrophoneTracks(page: Page) {
  return page.evaluate(() =>
    (window as MicrophoneWindow).__hyzMicrophoneTracks.map((track) => ({
      real: track instanceof MediaStreamTrack,
      kind: track.kind,
      readyState: track.readyState,
    })),
  );
}
