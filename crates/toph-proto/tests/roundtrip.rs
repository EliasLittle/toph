// Phase 1 exit criterion: two in-process endpoints exchange video and audio
// frames in both directions. Run with: cargo test -p toph-proto

use toph_proto::{
    AudioCodec, AudioParams, Hello, MediaFrame, Session, VideoCodec, VideoParams,
};

fn test_hello() -> Hello {
    Hello {
        video: VideoParams { codec: VideoCodec::Vp8, width: 640, height: 480 },
        audio: AudioParams { codec: AudioCodec::Opus, sample_rate: 48000, channels: 1 },
    }
}

fn make_video_frame(seq: u64, is_key: bool) -> MediaFrame {
    MediaFrame {
        timestamp_us: seq * 33_333, // ~30fps
        is_key,
        data: vec![seq as u8; 128],
    }
}

fn make_audio_frame(seq: u64) -> MediaFrame {
    MediaFrame {
        timestamp_us: seq * 20_000, // 50Hz = 20ms chunks
        is_key: false,
        data: vec![seq as u8; 40],
    }
}

#[tokio::test]
async fn two_endpoints_exchange_frames() {
    const N: usize = 20; // frames to send each way per media type

    let session_a = Session::spawn().await.expect("session A");
    let session_b = Session::spawn().await.expect("session B");

    let ticket = session_a.ticket().await.expect("ticket");

    // Connect B → A and accept on A, concurrently.
    let (call_a, call_b) = tokio::join!(
        session_a.accept(test_hello()),
        session_b.connect(&ticket, test_hello()),
    );
    let mut call_a = call_a.expect("accept");
    let mut call_b = call_b.expect("connect");

    // Verify that both sides received each other's Hello.
    assert!(matches!(call_a.remote_hello.video.codec, VideoCodec::Vp8));
    assert!(matches!(call_b.remote_hello.video.codec, VideoCodec::Vp8));

    // Send N video frames A→B and B→A concurrently, then receive them.
    let send_video_a = async {
        for i in 0..N as u64 {
            call_a.send.video.send(&make_video_frame(i, i == 0)).await.unwrap();
        }
    };
    let recv_video_b = async {
        let mut received = Vec::new();
        for _ in 0..N {
            let f = call_b.recv.video.recv().await.unwrap().unwrap();
            received.push(f);
        }
        received
    };
    let ((), frames_b) = tokio::join!(send_video_a, recv_video_b);
    assert_eq!(frames_b.len(), N);
    assert!(frames_b[0].is_key);
    for (i, f) in frames_b.iter().enumerate() {
        assert_eq!(f.data[0], i as u8);
    }

    // Send N audio frames B→A concurrently.
    let send_audio_b = async {
        for i in 0..N as u64 {
            call_b.send.audio.send(&make_audio_frame(i)).await.unwrap();
        }
    };
    let recv_audio_a = async {
        let mut received = Vec::new();
        for _ in 0..N {
            let f = call_a.recv.audio.recv().await.unwrap().unwrap();
            received.push(f);
        }
        received
    };
    let ((), frames_a) = tokio::join!(send_audio_b, recv_audio_a);
    assert_eq!(frames_a.len(), N);
    for (i, f) in frames_a.iter().enumerate() {
        assert_eq!(f.data[0], i as u8);
    }

    // Verify keyframe request control message.
    call_b.send.control.request_keyframe().await.unwrap();
    let ctrl = call_a.recv.control.recv().await.unwrap().unwrap();
    assert!(matches!(ctrl, toph_proto::ControlMessage::KeyframeRequest));
}
