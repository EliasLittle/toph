/**
 * Captures camera + mic, encodes with WebCodecs, and pushes to the call.
 * Returns the raw MediaStream so the caller can stop tracks on hang-up.
 *
 * IMPORTANT: Always call frame.close() / audioData.close() after encoding —
 * failing to do so silently stalls capture within a few seconds.
 */
export async function startCapture(call, localVideoEl) {
  const stream = await navigator.mediaDevices.getUserMedia({
    video: { width: 640, height: 480, frameRate: 30 },
    audio: { echoCancellation: true, noiseSuppression: true,
             sampleRate: 48000, channelCount: 1 },
  });

  // Show local preview (muted so no echo).
  localVideoEl.srcObject = stream;

  // The remote will set this flag when their decoder needs a fresh start.
  let forceKeyframe = false;
  call.on_keyframe_request(() => { forceKeyframe = true; });

  // ── Video encoder ───────────────────────────────────────────────────────────

  let frameCount = 0;

  const videoEncoder = new VideoEncoder({
    output: (chunk) => {
      const buf = new Uint8Array(chunk.byteLength);
      chunk.copyTo(buf);
      call.send_video(buf, chunk.timestamp, chunk.type === 'key');
    },
    error: (e) => console.error('[VideoEncoder]', e),
  });

  videoEncoder.configure({
    codec: 'vp8',
    width: 640,
    height: 480,
    bitrate: 1_500_000,   // 1.5 Mbps — reduces color banding vs 600 kbps
    framerate: 30,
    latencyMode: 'realtime',
  });

  // ── Audio encoder ───────────────────────────────────────────────────────────

  const audioEncoder = new AudioEncoder({
    output: (chunk) => {
      const buf = new Uint8Array(chunk.byteLength);
      chunk.copyTo(buf);
      call.send_audio(buf, chunk.timestamp);
    },
    error: (e) => console.error('[AudioEncoder]', e),
  });

  audioEncoder.configure({
    codec: 'opus',
    sampleRate: 48000,
    numberOfChannels: 1,
    bitrate: 32_000,
  });

  // ── Video pump ──────────────────────────────────────────────────────────────

  const videoReader = new MediaStreamTrackProcessor({
    track: stream.getVideoTracks()[0],
  }).readable.getReader();

  (async () => {
    while (true) {
      const { done, value: frame } = await videoReader.read();
      if (done) break;

      // Drop frames if the encoder is behind — stale frames add latency.
      if (videoEncoder.encodeQueueSize > 2) {
        frame.close();
        continue;
      }

      // Keyframe every 5 s (~150 frames) or on explicit remote request.
      const keyFrame = forceKeyframe || frameCount % 150 === 0;
      forceKeyframe = false;
      videoEncoder.encode(frame, { keyFrame });
      frame.close();
      frameCount++;
    }
  })();

  // ── Audio pump ──────────────────────────────────────────────────────────────

  const audioReader = new MediaStreamTrackProcessor({
    track: stream.getAudioTracks()[0],
  }).readable.getReader();

  (async () => {
    while (true) {
      const { done, value: audioData } = await audioReader.read();
      if (done) break;
      audioEncoder.encode(audioData);
      audioData.close();
    }
  })();

  return stream;
}
