#![allow(unsafe_code)]
//! The camera: Media Foundation's list of video devices, and a thread per open camera that reads its frames (BGRA,
//! top row first) into the latest frame. The camera page shows the latest frame, photos save it and the video
//! recorder encodes it. Everything here runs with Windows' own calls; no drivers or FFmpeg devices are needed.

use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFAttributes, IMFMediaSource, IMFMediaType, IMFSample, IMFSourceReader,
    MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE,
    MF_MT_SUBTYPE, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    MF_VERSION, MFCreateAttributes, MFCreateMediaType, MFCreateSourceReaderFromMediaSource,
    MFEnumDeviceSources, MFMediaType_Video, MFStartup, MFVideoFormat_RGB32,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree};
use windows::core::PWSTR;

/// One camera frame: BGRA pixels, top row first. `seq` counts the frames of this camera (from 1).
pub struct Frame {
    pub w: u32,
    pub h: u32,
    pub seq: u64,
    pub bgra: Vec<u8>,
}

/// The latest frame of an open camera, shared with the reader thread.
pub type Feed = Arc<Mutex<Option<Frame>>>;

/// An open camera. Dropping it stops its reader thread.
pub struct Camera {
    stop: Arc<AtomicBool>,
    pub feed: Feed,
    /// The frame size the camera delivers.
    pub w: u32,
    pub h: u32,
}

impl Drop for Camera {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
    }
}

fn err(e: windows::core::Error) -> String {
    e.to_string()
}

/// Starts Media Foundation (counted by Windows, so calling it again is harmless).
fn startup() -> Result<(), String> {
    unsafe { MFStartup(MF_VERSION, 0).map_err(err) }
}

fn create_attributes() -> Result<IMFAttributes, String> {
    unsafe {
        let mut attrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 1).map_err(err)?;
        attrs.ok_or_else(|| "no attributes".to_string())
    }
}

/// The video capture devices, in the order `open` takes them.
fn sources() -> Result<Vec<IMFActivate>, String> {
    startup()?;
    let attrs = create_attributes()?;
    unsafe {
        attrs
            .SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
            .map_err(err)?;
        let mut list: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        MFEnumDeviceSources(&attrs, &mut list, &mut count).map_err(err)?;
        let mut out = Vec::new();
        if !list.is_null() {
            for i in 0..count as usize {
                // Each entry is handed to us (and released when dropped); the array itself is freed below.
                if let Some(activate) = (*list.add(i)).take() {
                    out.push(activate);
                }
            }
            CoTaskMemFree(Some(list as *const core::ffi::c_void));
        }
        Ok(out)
    }
}

fn friendly_name(activate: &IMFActivate) -> String {
    unsafe {
        let mut name = PWSTR::null();
        let mut len = 0u32;
        if activate
            .GetAllocatedString(&MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, &mut name, &mut len)
            .is_err()
        {
            return "Camera".into();
        }
        let text = name.to_string().unwrap_or_else(|_| "Camera".into());
        CoTaskMemFree(Some(name.0 as *const core::ffi::c_void));
        text
    }
}

/// The names of the cameras the computer has (empty when there are none or Media Foundation fails).
pub fn devices() -> Vec<String> {
    sources()
        .map(|list| list.iter().map(friendly_name).collect())
        .unwrap_or_default()
}

/// Opens the camera at this index of `devices()` and starts reading its frames.
pub fn open(index: usize) -> Result<Camera, String> {
    let feed: Feed = Arc::new(Mutex::new(None));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<Result<(u32, u32), String>>();
    let (f, s) = (Arc::clone(&feed), Arc::clone(&stop));
    let handle = std::thread::Builder::new()
        .name("rimelight-media-camera".into())
        .spawn(move || match start_reader(index) {
            Ok((reader, w, h)) => {
                let _ = tx.send(Ok((w, h)));
                read_loop(&reader, w, h, &f, &s);
            }
            Err(e) => {
                let _ = tx.send(Err(e));
            }
        })
        .map_err(|e| e.to_string())?;
    match rx.recv() {
        Ok(Ok((w, h))) => Ok(Camera { stop, feed, w, h }),
        Ok(Err(e)) => {
            let _ = handle.join();
            Err(e)
        }
        Err(_) => Err("the camera stopped unexpectedly".into()),
    }
}

/// Runs on the reader thread: the camera, a source reader that gives BGRA frames, and the frame size.
fn start_reader(index: usize) -> Result<(IMFSourceReader, u32, u32), String> {
    unsafe {
        // The reader's objects are used on this thread only, in the multi-threaded COM mode.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let list = sources()?;
    let activate = list
        .get(index)
        .ok_or("That camera is no longer connected")?;
    unsafe {
        let source: IMFMediaSource = activate.ActivateObject::<IMFMediaSource>().map_err(err)?;
        let attrs = create_attributes()?;
        attrs
            .SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)
            .map_err(err)?;
        let reader = MFCreateSourceReaderFromMediaSource(&source, &attrs).map_err(err)?;
        let mt: IMFMediaType = MFCreateMediaType().map_err(err)?;
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(err)?;
        mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
            .map_err(err)?;
        reader
            .SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, None, &mt)
            .map_err(err)?;
        let current = reader
            .GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)
            .map_err(err)?;
        // The frame size packs the width in the high 32 bits and the height in the low 32 bits.
        let size = current.GetUINT64(&MF_MT_FRAME_SIZE).map_err(err)?;
        let (w, h) = ((size >> 32) as u32, (size & 0xFFFF_FFFF) as u32);
        if w == 0 || h == 0 {
            return Err("the camera gave no picture size".into());
        }
        Ok((reader, w, h))
    }
}

/// Reads frames until stopped. A failed read waits a moment and tries again.
fn read_loop(reader: &IMFSourceReader, w: u32, h: u32, feed: &Feed, stop: &AtomicBool) {
    let mut seq = 0u64;
    let want = w as usize * h as usize * 4;
    while !stop.load(Relaxed) {
        unsafe {
            let mut sample: Option<IMFSample> = None;
            let read = reader.ReadSample(
                MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                0,
                None,
                None,
                None,
                Some(&mut sample as *mut _),
            );
            let Some(sample) = sample.filter(|_| read.is_ok()) else {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            };
            let Ok(buffer) = sample.ConvertToContiguousBuffer() else {
                continue;
            };
            let mut data: *mut u8 = std::ptr::null_mut();
            let mut len = 0u32;
            if buffer.Lock(&mut data, None, Some(&mut len)).is_err() {
                continue;
            }
            let n = want.min(len as usize);
            let bgra = if data.is_null() || n == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(data, n).to_vec()
            };
            let _ = buffer.Unlock();
            if bgra.len() < want {
                continue;
            }
            seq += 1;
            if let Ok(mut latest) = feed.lock() {
                *latest = Some(Frame { w, h, seq, bgra });
            }
        }
    }
}
