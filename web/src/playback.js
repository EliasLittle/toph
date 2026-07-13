/**
 * Wires incoming encoded frames from the call to WebCodecs decoders and
 * renders decoded video to canvas / audio to an AudioContext.
 *
 * call.remote_width() / call.remote_height() must return valid values before
 * this is called (they're set from the Hello handshake).
 */
export function startPlayback(call, canvas) {
  const ctx = canvas.getContext('2d');

  // ── Video decoder ───────────────────────────────────────────────────────────

  let sawKeyframe = false;

  const videoCfg = {
    codec: 'vp8',
    codedWidth: call.remote_width(),
    codedHeight: call.remote_height(),
  };

  const videoDecoder = new VideoDecoder({
    output: (frame) => {
      ctx.drawImage(frame, 0, 0, canvas.width, canvas.height);
      frame.close();
    },
    error: (e) => {
      console.error('[VideoDecoder]', e);
      // Defer reset to avoid re-entrancy issues inside the error callback.
      setTimeout(() => {
        try {
          videoDecoder.reset();
          videoDecoder.configure(videoCfg);
        } catch (_) {}
        sawKeyframe = false;
        // Ask the remote to send a fresh keyframe so we can restart cleanly.
        call.request_keyframe();
      }, 0);
    },
  });

  videoDecoder.configure(videoCfg);

  call.on_video((data, timestampUs, isKey) => {
    // A VP8 stream must start on a keyframe; skip until we see one.
    if (!sawKeyframe && !isKey) return;
    sawKeyframe = true;

    try {
      videoDecoder.decode(new EncodedVideoChunk({
        type: isKey ? 'key' : 'delta',
        timestamp: timestampUs,
        data,
      }));
    } catch (e) {
      console.error('[video decode]', e);
      sawKeyframe = false;
      call.request_keyframe();
    }
  });

  // ── Audio decoder + scheduler ───────────────────────────────────────────────

  // AudioContext must be created after a user gesture. When a call arrives
  // via accept() (no gesture), the context starts suspended — resume() is
  // called immediately and the browser will un-suspend on the next gesture.
  const actx = new AudioContext({ sampleRate: 48000 });
  actx.resume();

  // Self-clocking scheduler: each decoded buffer is scheduled back-to-back
  // on the AudioContext timeline. A 60 ms jitter buffer absorbs network jitter
  // without introducing noticeable delay.
  let nextTime = 0;

  const audioDecoder = new AudioDecoder({
    output: (audioData) => {
      const buf = actx.createBuffer(
        audioData.numberOfChannels,
        audioData.numberOfFrames,
        audioData.sampleRate,
      );
      // Copy decoded PCM samples into the AudioBuffer.
      for (let ch = 0; ch < audioData.numberOfChannels; ch++) {
        audioData.copyTo(buf.getChannelData(ch), { planeIndex: ch, format: 'f32-planar' });
      }
      audioData.close();

      const src = actx.createBufferSource();
      src.buffer = buf;
      src.connect(actx.destination);

      // If we've fallen behind (gap in arrival), jump nextTime forward so
      // we don't schedule a burst of buffers all at once.
      nextTime = Math.max(nextTime, actx.currentTime + 0.06);
      src.start(nextTime);
      nextTime += buf.duration;
    },
    error: (e) => console.error('[AudioDecoder]', e),
  });

  audioDecoder.configure({
    codec: 'opus',
    sampleRate: 48000,
    numberOfChannels: 1,
  });

  call.on_audio((data, timestampUs) => {
    try {
      // Opus frames are always independently decodable, so type is always 'key'.
      audioDecoder.decode(new EncodedAudioChunk({
        type: 'key',
        timestamp: timestampUs,
        data,
      }));
    } catch (e) {
      console.error('[audio decode]', e);
    }
  });

  return function stop() {
    try { videoDecoder.close(); } catch (_) {}
    try { audioDecoder.close(); } catch (_) {}
    actx.close();
  };
}
