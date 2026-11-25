//! Screen and voice recording into MP4 / M4A / MP3 / WAV / FLAC: GDI screen frames encoded to H.264 (Windows' Media
//! Foundation encoder, else OpenH264, else MPEG-4), and the sound mix encoded to AAC (or the format the file name asks
//! for). Everything FFmpeg runs on one thread (its objects can't move between threads); the sound devices feed it
//! through queues. Pausing keeps the file's clock stopped: the paused time is cut out of the recording.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ffmpeg_next as ff;

use super::audio_in::{self, Capture};
use super::camera;

use rimelight_platform::{ScreenGrabber, ScreenRect};

/// A camera as the picture source: its latest frames at this size and frame rate.
pub struct CameraSource {
    pub feed: camera::Feed,
    pub w: u32,
    pub h: u32,
    pub fps: i32,
}

pub struct Options {
    /// The screen area to record (physical pixels) and frames per second; None records sound only.
    pub screen: Option<(ScreenRect, i32)>,
    /// A camera to record instead of the screen (the video of the camera page).
    pub camera: Option<CameraSource>,
    pub system_audio: bool,
    pub mic: bool,
    pub path: PathBuf,
    /// The microphone to use (its name); None for the default one.
    pub mic_name: Option<String>,
}

pub struct Recording {
    stop: Arc<AtomicBool>,
    discard: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    /// The recording's own clock in milliseconds, without the paused time (written by the recorder thread).
    clock_ms: Arc<AtomicU64>,
    level: Arc<AtomicU32>,
    handle: Option<JoinHandle<Result<PathBuf, String>>>,
    pub notes: Vec<String>,
}

impl Recording {
    /// The time recorded so far, not counting the paused time.
    pub fn elapsed_ms(&self) -> u64 {
        self.clock_ms.load(Relaxed)
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Relaxed))
    }

    /// Pauses or resumes the recording (its sound and pictures are left out while paused).
    pub fn set_paused(&self, on: bool) {
        self.pause.store(on, Relaxed);
    }

    pub fn is_paused(&self) -> bool {
        self.pause.load(Relaxed)
    }

    /// Stops, finishes the file and returns its path.
    pub fn finish(mut self) -> Result<PathBuf, String> {
        self.stop.store(true, Relaxed);
        self.handle.take().map_or(Err("not running".into()), |h| {
            h.join()
                .unwrap_or_else(|_| Err("the recorder stopped unexpectedly".into()))
        })
    }

    /// Stops and deletes the file.
    pub fn discard(mut self) {
        self.discard.store(true, Relaxed);
        self.stop.store(true, Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
    }
}

pub fn start(opts: Options) -> Result<Recording, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let discard = Arc::new(AtomicBool::new(false));
    let level = Arc::new(AtomicU32::new(0));
    let pause = Arc::new(AtomicBool::new(false));
    let clock_ms = Arc::new(AtomicU64::new(0));
    let (ready_tx, ready_rx) = mpsc::channel::<Result<Vec<String>, String>>();
    let (s, d, l) = (Arc::clone(&stop), Arc::clone(&discard), Arc::clone(&level));
    let (p, c) = (Arc::clone(&pause), Arc::clone(&clock_ms));
    let handle = std::thread::Builder::new()
        .name("rimelight-media-recorder".into())
        .spawn(move || {
            let result = run(&opts, &s, &l, &p, &c, &ready_tx);
            if d.load(Relaxed) || result.is_err() {
                let _ = std::fs::remove_file(&opts.path);
            }
            if let Err(e) = &result {
                let _ = ready_tx.send(Err(e.clone()));
            }
            result.map(|()| opts.path.clone())
        })
        .map_err(|e| e.to_string())?;
    match ready_rx.recv() {
        Ok(Ok(notes)) => Ok(Recording {
            stop,
            discard,
            pause,
            clock_ms,
            level,
            handle: Some(handle),
            notes,
        }),
        Ok(Err(e)) => {
            let _ = handle.join();
            Err(e)
        }
        Err(_) => Err("the recorder stopped unexpectedly".into()),
    }
}

/// The recording's clock: wall time minus the time spent paused.
struct Clock {
    started: Instant,
    paused_total: f64,
    paused_since: Option<f64>,
}

impl Clock {
    /// Seconds of recording so far. While paused the clock stands still.
    fn now(&mut self, paused: bool) -> f64 {
        let t = self.started.elapsed().as_secs_f64();
        if paused {
            let since = *self.paused_since.get_or_insert(t);
            since - self.paused_total
        } else {
            if let Some(since) = self.paused_since.take() {
                self.paused_total += t - since;
            }
            t - self.paused_total
        }
    }
}

/// Where the video's frames come from.
enum FrameSrc {
    Screen(ScreenGrabber),
    Camera(camera::Feed),
}

/// Copies top-down BGRA rows into the frame (the row length is w * 4).
fn copy_rows(dst: &mut ff::frame::Video, w: usize, h: usize, px: &[u8]) {
    let row = w * 4;
    let stride = dst.stride(0);
    let out = dst.data_mut(0);
    for y in 0..h {
        if (y + 1) * row > px.len() {
            break;
        }
        out[y * stride..y * stride + row].copy_from_slice(&px[y * row..y * row + row]);
    }
}

struct VideoPart {
    src: FrameSrc,
    bgra: ff::frame::Video,
    scaler: ff::software::scaling::Context,
    enc: ff::encoder::Video,
    out_idx: usize,
    fps: i32,
    last_pts: i64,
    w: u32,
    h: u32,
}

struct AudioPart {
    enc: ff::encoder::Audio,
    out_idx: usize,
    frame_size: usize,
    written: i64,
    /// Interleaved 16-bit samples (WAV, FLAC) instead of planar 32-bit floats (AAC, MP3).
    packed_s16: bool,
}

/// The sound encoder for a file name: AAC for .m4a and video, MP3, WAV (16-bit PCM) or FLAC.
struct AudioCodec {
    id: ff::codec::Id,
    bit_rate: usize,
    sample: ff::format::Sample,
}

fn audio_codec(path: &Path) -> AudioCodec {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let planar = ff::format::Sample::F32(ff::format::sample::Type::Planar);
    let packed16 = ff::format::Sample::I16(ff::format::sample::Type::Packed);
    match ext.as_str() {
        "mp3" => AudioCodec {
            id: ff::codec::Id::MP3,
            bit_rate: 192_000,
            sample: planar,
        },
        "wav" => AudioCodec {
            id: ff::codec::Id::PCM_S16LE,
            bit_rate: 0,
            sample: packed16,
        },
        "flac" => AudioCodec {
            id: ff::codec::Id::FLAC,
            bit_rate: 0,
            sample: packed16,
        },
        _ => AudioCodec {
            id: ff::codec::Id::AAC,
            bit_rate: 160_000,
            sample: planar,
        },
    }
}

fn open_video_encoder(
    w: u32,
    h: u32,
    fps: i32,
    global: bool,
) -> Result<ff::encoder::Video, String> {
    let mut last = String::from("no H.264 encoder");
    for (name, pix) in [
        ("h264_mf", ff::format::Pixel::NV12),
        ("libopenh264", ff::format::Pixel::YUV420P),
        ("mpeg4", ff::format::Pixel::YUV420P),
    ] {
        let Some(codec) = ff::encoder::find_by_name(name) else {
            continue;
        };
        let ctx = ff::codec::context::Context::new_with_codec(codec);
        let Ok(mut enc) = ctx.encoder().video() else {
            continue;
        };
        enc.set_width(w);
        enc.set_height(h);
        enc.set_format(pix);
        enc.set_time_base((1, fps));
        enc.set_frame_rate(Some((fps, 1)));
        // About 0.12 bits per pixel per frame: 1080p at 30 fps is about 7.5 Mbit/s.
        enc.set_bit_rate(((f64::from(w) * f64::from(h) * f64::from(fps)) * 0.12) as usize);
        enc.set_gop((fps * 2) as u32);
        enc.set_max_b_frames(0);
        if global {
            enc.set_flags(ff::codec::flag::Flags::GLOBAL_HEADER);
        }
        let mut dict = ff::Dictionary::new();
        if name == "h264_mf" {
            dict.set("hw_encoding", "1");
            dict.set("scenario", "display_remoting");
        }
        match enc.open_as_with(codec, dict) {
            Ok(opened) => return Ok(opened),
            Err(e) => last = format!("{name}: {e}"),
        }
    }
    Err(format!("Could not start the video encoder ({last})"))
}

fn open_audio_encoder(global: bool, codec: &AudioCodec) -> Result<ff::encoder::Audio, String> {
    let found = ff::encoder::find(codec.id).ok_or("no encoder for this sound format")?;
    let mut enc = ff::codec::context::Context::new_with_codec(found)
        .encoder()
        .audio()
        .map_err(|e| e.to_string())?;
    enc.set_rate(audio_in::RATE as i32);
    enc.set_channel_layout(ff::ChannelLayout::STEREO);
    enc.set_format(codec.sample);
    if codec.bit_rate > 0 {
        enc.set_bit_rate(codec.bit_rate);
    }
    enc.set_time_base((1, audio_in::RATE as i32));
    if global {
        enc.set_flags(ff::codec::flag::Flags::GLOBAL_HEADER);
    }
    enc.open_as(found)
        .map_err(|e| format!("Could not start the sound encoder: {e}"))
}

fn write_packets_video(
    v: &mut VideoPart,
    octx: &mut ff::format::context::Output,
    out_tb: ff::Rational,
) -> Result<(), String> {
    let mut pkt = ff::Packet::empty();
    while v.enc.receive_packet(&mut pkt).is_ok() {
        pkt.set_stream(v.out_idx);
        pkt.rescale_ts((1, v.fps), out_tb);
        pkt.write_interleaved(octx).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_packets_audio(
    a: &mut AudioPart,
    octx: &mut ff::format::context::Output,
    out_tb: ff::Rational,
) -> Result<(), String> {
    let mut pkt = ff::Packet::empty();
    while a.enc.receive_packet(&mut pkt).is_ok() {
        pkt.set_stream(a.out_idx);
        pkt.rescale_ts((1, audio_in::RATE as i32), out_tb);
        pkt.write_interleaved(octx).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn encode_audio_block(a: &mut AudioPart, cap: &mut Capture) -> Result<(), String> {
    let n = a.frame_size;
    let (l, r) = cap.pull(n);
    let sample = if a.packed_s16 {
        ff::format::Sample::I16(ff::format::sample::Type::Packed)
    } else {
        ff::format::Sample::F32(ff::format::sample::Type::Planar)
    };
    let mut frame = ff::frame::Audio::new(sample, n, ff::ChannelLayout::STEREO);
    frame.set_rate(audio_in::RATE);
    if a.packed_s16 {
        // Left and right alternate: four bytes per stereo frame.
        let dst = frame.data_mut(0);
        for i in 0..n {
            let le = ((l[i] * 32767.0) as i16).to_le_bytes();
            let ri = ((r[i] * 32767.0) as i16).to_le_bytes();
            dst[i * 4..i * 4 + 2].copy_from_slice(&le);
            dst[i * 4 + 2..i * 4 + 4].copy_from_slice(&ri);
        }
    } else {
        frame.plane_mut::<f32>(0)[..n].copy_from_slice(&l);
        frame.plane_mut::<f32>(1)[..n].copy_from_slice(&r);
    }
    frame.set_pts(Some(a.written));
    a.written += n as i64;
    a.enc.send_frame(&frame).map_err(|e| e.to_string())
}

fn run(
    opts: &Options,
    stop: &AtomicBool,
    level: &Arc<AtomicU32>,
    pause: &AtomicBool,
    clock_ms: &AtomicU64,
    ready: &mpsc::Sender<Result<Vec<String>, String>>,
) -> Result<(), String> {
    rimelight_media::engine::player::init()?;
    if let Some(dir) = opts.path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // The video encoder starts first: Windows' Media Foundation encoder needs this thread's COM in the multi-threaded
    // mode, and the sound devices would otherwise set it up single-threaded first.
    let mut octx =
        ff::format::output(&opts.path).map_err(|e| format!("Could not create the file: {e}"))?;
    let global = octx
        .format()
        .flags()
        .contains(ff::format::flag::Flags::GLOBAL_HEADER);

    let source = match (opts.screen, &opts.camera) {
        (_, Some(cam)) => Some((
            FrameSrc::Camera(Arc::clone(&cam.feed)),
            cam.w & !1,
            cam.h & !1,
            cam.fps,
        )),
        (Some((rect, fps)), None) => {
            let w = (rect.w.max(2) as u32) & !1;
            let h = (rect.h.max(2) as u32) & !1;
            let area = ScreenRect {
                x: rect.x,
                y: rect.y,
                w: w as i32,
                h: h as i32,
            };
            let grabber = ScreenGrabber::new(area).ok_or("Could not reach the screen")?;
            Some((FrameSrc::Screen(grabber), w, h, fps))
        }
        (None, None) => None,
    };
    let mut video = match source {
        Some((src, w, h, fps)) => {
            let enc = open_video_encoder(w, h, fps, global)?;
            let scaler = ff::software::scaling::Context::get(
                ff::format::Pixel::BGRA,
                w,
                h,
                enc.format(),
                w,
                h,
                ff::software::scaling::Flags::BILINEAR,
            )
            .map_err(|e| e.to_string())?;
            let mut ost = octx
                .add_stream(ff::encoder::find(ff::codec::Id::H264))
                .map_err(|e| e.to_string())?;
            ost.set_parameters(&enc);
            ost.set_time_base((1, fps));
            let out_idx = ost.index();
            let bgra = ff::frame::Video::new(ff::format::Pixel::BGRA, w, h);
            Some(VideoPart {
                src,
                bgra,
                scaler,
                enc,
                out_idx,
                fps,
                last_pts: -1,
                w,
                h,
            })
        }
        None => None,
    };

    let mut cap = Capture::start(opts.system_audio, opts.mic, opts.mic_name.as_deref());
    let mut notes = cap.notes.clone();
    if opts.screen.is_none() && opts.camera.is_none() && cap.is_empty() {
        return Err(format!(
            "No microphone to record from. {}",
            notes.join("; ")
        ));
    }

    let codec = audio_codec(&opts.path);
    let mut audio = if cap.is_empty() {
        None
    } else {
        let enc = open_audio_encoder(global, &codec)?;
        let frame_size = (enc.frame_size() as usize).max(1024);
        let mut ost = octx
            .add_stream(ff::encoder::find(codec.id))
            .map_err(|e| e.to_string())?;
        ost.set_parameters(&enc);
        ost.set_time_base((1, audio_in::RATE as i32));
        let packed_s16 = matches!(codec.sample, ff::format::Sample::I16(_));
        Some(AudioPart {
            enc,
            out_idx: ost.index(),
            frame_size,
            written: 0,
            packed_s16,
        })
    };

    octx.write_header()
        .map_err(|e| format!("Could not start the file: {e}"))?;
    let out_tbs: Vec<ff::Rational> = octx.streams().map(|s| s.time_base()).collect();
    if video.is_some() && audio.is_none() && (opts.system_audio || opts.mic) {
        notes.push("Recording without sound".into());
    }
    let _ = ready.send(Ok(notes));

    let mut clock = Clock {
        started: Instant::now(),
        paused_total: 0.0,
        paused_since: None,
    };
    let level_src = Arc::clone(&cap.level);
    while !stop.load(Relaxed) {
        let paused = pause.load(Relaxed);
        let eff = clock.now(paused);
        clock_ms.store((eff * 1000.0) as u64, Relaxed);
        if let Some(v) = video.as_mut() {
            // Wait for the next frame's time, then grab and encode it.
            let next = (v.last_pts + 1) as f64 / f64::from(v.fps);
            if next > eff {
                std::thread::sleep(Duration::from_secs_f64((next - eff).min(0.05)));
            }
            let eff = clock.now(paused);
            let pts = (eff * f64::from(v.fps)).floor() as i64;
            if !paused && pts > v.last_pts {
                v.last_pts = pts;
                match &mut v.src {
                    FrameSrc::Screen(grabber) => {
                        if let Some(px) = grabber.grab(true) {
                            copy_rows(&mut v.bgra, v.w as usize, v.h as usize, &px);
                        }
                    }
                    // The camera's latest frame, when it is the size the encoder was opened for (else the last one stays).
                    FrameSrc::Camera(feed) => {
                        if let Ok(guard) = feed.lock() {
                            if let Some(frame) = guard.as_ref().filter(|f| f.w == v.w && f.h == v.h)
                            {
                                copy_rows(&mut v.bgra, v.w as usize, v.h as usize, &frame.bgra);
                            }
                        }
                    }
                }
                let mut yuv = ff::frame::Video::empty();
                v.scaler.run(&v.bgra, &mut yuv).map_err(|e| e.to_string())?;
                yuv.set_pts(Some(pts));
                v.enc.send_frame(&yuv).map_err(|e| e.to_string())?;
                write_packets_video(v, &mut octx, out_tbs[v.out_idx])?;
            }
        } else {
            std::thread::sleep(Duration::from_millis(20));
        }
        if let Some(a) = audio.as_mut() {
            let paused = pause.load(Relaxed);
            let eff = clock.now(paused);
            if paused {
                // Throw away the sound heard while paused, so it doesn't play back late on resume.
                let _ = cap.pull(a.frame_size);
            } else {
                // Keep 100 ms behind the clock so late sound packets still make it in.
                let due = ((eff - 0.1) * f64::from(audio_in::RATE)) as i64;
                while a.written + a.frame_size as i64 <= due {
                    encode_audio_block(a, &mut cap)?;
                    write_packets_audio(a, &mut octx, out_tbs[a.out_idx])?;
                }
            }
        }
        level.store(level_src.load(Relaxed), Relaxed);
    }

    if let Some(v) = video.as_mut() {
        let _ = v.enc.send_eof();
        write_packets_video(v, &mut octx, out_tbs[v.out_idx])?;
    }
    if let Some(a) = audio.as_mut() {
        let _ = a.enc.send_eof();
        write_packets_audio(a, &mut octx, out_tbs[a.out_idx])?;
    }
    octx.write_trailer().map_err(|e| e.to_string())?;
    Ok(())
}

/// The file name for a new recording.
pub fn new_path(dir: &Path, prefix: &str, ext: &str) -> PathBuf {
    crate::files::free_path(dir, &crate::files::stamp(prefix), ext)
}
