use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::Instant,
};
pub struct Recording {
    pub size: [u32; 2],
    pub path: PathBuf,
    frames: SyncSender<(Vec<u8>, u64)>,
    started: Instant,
    final_count: Arc<AtomicU64>,
    pub done: Receiver<Result<(), String>>,
    pub count: u64,
}
impl Recording {
    pub fn start(path: &Path, size: [u32; 2]) -> Result<Self, String> {
        let size = [size[0] / 2 * 2, size[1] / 2 * 2];
        let mut child = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "rawvideo",
                "-pixel_format",
                "rgba",
                "-video_size",
                &format!("{}x{}", size[0], size[1]),
                "-framerate",
                "30",
                "-i",
                "pipe:0",
                "-an",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not start ffmpeg: {e}"))?;
        let stdin = child.stdin.take().ok_or("No recorder input")?;
        let (frames, rx) = mpsc::sync_channel::<(Vec<u8>, u64)>(8);
        let (tx, done) = mpsc::channel();
        let final_count = Arc::new(AtomicU64::new(0));
        let final_target = final_count.clone();
        std::thread::spawn(move || {
            let result = encode(child, stdin, rx, final_target);
            let _ = tx.send(result);
        });
        Ok(Self {
            size,
            path: path.into(),
            frames,
            done,
            count: 0,
            started: Instant::now(),
            final_count,
        })
    }
    pub fn finish(self) -> Receiver<Result<(), String>> {
        // The worker pads the last captured frame to the exact stop time, even
        // when the GPU has not supplied a new screenshot in the final interval.
        self.final_count.store(
            (self.started.elapsed().as_secs_f64() * 30.0).ceil() as u64,
            Ordering::Release,
        );
        self.done
    }
    pub fn frame(&mut self, img: &eframe::egui::ColorImage) {
        let bytes: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
        let bytes = if [img.width() as u32, img.height() as u32] != self.size {
            let image =
                image::RgbaImage::from_raw(img.width() as u32, img.height() as u32, bytes).unwrap();
            image::imageops::resize(
                &image,
                self.size[0],
                self.size[1],
                image::imageops::FilterType::Triangle,
            )
            .into_raw()
        } else {
            bytes
        };
        let desired = (self.started.elapsed().as_secs_f64() * 30.0).ceil() as u64;
        let repeat = desired.saturating_sub(self.count).max(1);
        if self.frames.try_send((bytes, repeat)).is_ok() {
            self.count += repeat;
        }
    }
}
fn encode(
    child: Child,
    mut stdin: std::process::ChildStdin,
    frames: Receiver<(Vec<u8>, u64)>,
    final_count: Arc<AtomicU64>,
) -> Result<(), String> {
    let mut write_error = None;
    let mut written = 0;
    let mut last = None;
    'encode: while let Ok((bytes, repeat)) = frames.recv() {
        for _ in 0..repeat {
            if let Err(e) = stdin.write_all(&bytes) {
                write_error = Some(e.to_string());
                break 'encode;
            }
            written += 1;
        }
        last = Some(bytes);
    }
    if write_error.is_none()
        && let Some(bytes) = last
    {
        for _ in written..final_count.load(Ordering::Acquire) {
            if let Err(e) = stdin.write_all(&bytes) {
                write_error = Some(e.to_string());
                break;
            }
        }
    }
    drop(stdin);
    let result = child.wait_with_output().map_err(|e| e.to_string())?;
    if result.status.success() {
        if let Some(e) = write_error {
            Err(e)
        } else {
            Ok(())
        }
    } else {
        Err(format!(
            "Recording failed: {}",
            String::from_utf8_lossy(&result.stderr)
        ))
    }
}
