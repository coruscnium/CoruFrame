//! One shared-memory frame from a portal ScreenCast stream.
//!
//! The spike stops at a single frame: connect the portal fd, find the node,
//! negotiate plain (unmodifiered) video so the compositor sends MemFd, copy the
//! first buffer, and return it as RGB. DMA-BUF-only streams are rejected here
//! until an EGL download exists.

use std::os::fd::OwnedFd;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pipewire as pw;
use pw::properties::properties;
use pw::spa;

use coruframe_core::private_fs;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const FRAME_TIMEOUT: Duration = Duration::from_secs(10);

pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub format: String,
    pub buffer: String,
    pub rgb: Vec<u8>,
}

impl CapturedFrame {
    pub fn rgb_image(&self) -> Option<image::RgbImage> {
        image::RgbImage::from_raw(self.width, self.height, self.rgb.clone())
    }

    // Only the explicit `--frame PATH` spike writes a full frame; production
    // capture keeps the frame in memory and writes OCR crops only.
    pub fn save(&self, path: &Path) -> Result<()> {
        let image = image::RgbImage::from_raw(self.width, self.height, self.rgb.clone())
            .ok_or("frame buffer does not match its size")?;
        let mut file = private_fs::create_private(path)?;
        image.write_to(&mut file, image::ImageFormat::Png)?;
        Ok(())
    }
}

#[derive(Default)]
struct State {
    info: spa::param::video::VideoInfoRaw,
}

pub fn capture_one(fd: OwnedFd, node_id: u32) -> Result<CapturedFrame> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopBox::new(None)
        .map_err(|error| format!("pipewire main loop: {error}"))?;
    let context = pw::context::ContextBox::new(mainloop.loop_(), None)
        .map_err(|error| format!("pipewire context: {error}"))?;
    let core = context
        .connect_fd(fd, None)
        .map_err(|error| format!("pipewire connect_fd: {error}"))?;
    let stream = pw::stream::StreamBox::new(
        &core,
        "coruframe-capture",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(|error| format!("pipewire stream: {error}"))?;

    let slot: Arc<Mutex<Option<std::result::Result<CapturedFrame, String>>>> =
        Arc::new(Mutex::new(None));
    let process_slot = slot.clone();
    let _listener = stream
        .add_local_listener_with_user_data(State::default())
        .param_changed(|_, state, id, param| {
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Some(param) = param else {
                return;
            };
            let mut info = spa::param::video::VideoInfoRaw::new();
            if info.parse(param).is_ok() {
                state.info = info;
            }
        })
        .process(move |stream, state| {
            if process_slot
                .lock()
                .map(|slot| slot.is_some())
                .unwrap_or(true)
            {
                return;
            }
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            let Some(data) = datas.first_mut() else {
                return;
            };
            let frame = copy_frame(data, state.info).map_err(|error| error.to_string());
            if let Ok(mut slot) = process_slot.lock() {
                *slot = Some(frame);
            }
        })
        .register()
        .map_err(|error| format!("pipewire listener: {error}"))?;

    let values: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(format_object()),
    )
    .map_err(|error| format!("serialize format: {error}"))?
    .0
    .into_inner();
    let mut params = [spa::pod::Pod::from_bytes(&values).ok_or("invalid format pod")?];
    stream
        .connect(
            spa::utils::Direction::Input,
            Some(node_id),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(|error| format!("pipewire connect: {error}"))?;

    let deadline = Instant::now() + FRAME_TIMEOUT;
    while Instant::now() < deadline {
        mainloop.loop_().iterate(Duration::from_millis(50));
        if slot.lock().map(|slot| slot.is_some()).unwrap_or(true) {
            break;
        }
    }
    let result = slot.lock().ok().and_then(|mut slot| slot.take());
    match result {
        Some(Ok(frame)) => Ok(frame),
        Some(Err(error)) => Err(error.into()),
        None => Err(format!(
            "no frame within {} s; the stream may need a DMA-BUF download",
            FRAME_TIMEOUT.as_secs()
        )
        .into()),
    }
}

fn format_object() -> spa::pod::Object {
    // No VideoModifier property: a plain format is what makes KWin and xdpw
    // hand over MemFd instead of DMA-BUF.
    spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::RGBA,
            spa::param::video::VideoFormat::RGBx,
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: 1920,
                height: 1080
            },
            spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            spa::utils::Rectangle {
                width: 8192,
                height: 8192
            }
        ),
    )
}

fn copy_frame(
    data: &mut spa::buffer::Data,
    info: spa::param::video::VideoInfoRaw,
) -> Result<CapturedFrame> {
    let width = info.size().width;
    let height = info.size().height;
    if width == 0 || height == 0 {
        return Err("video format was not negotiated".into());
    }
    let kind = data.type_();
    if kind == spa::buffer::DataType::DmaBuf {
        return Err(
            "frame arrived as DMA-BUF; shared-memory capture is not implemented yet".into(),
        );
    }
    let (offset, size, stride) = {
        let chunk = data.chunk();
        let stride = if chunk.stride() > 0 {
            chunk.stride() as usize
        } else {
            width as usize * 4
        };
        let size = if chunk.size() > 0 {
            chunk.size() as usize
        } else {
            stride * height as usize
        };
        (chunk.offset() as usize, size, stride)
    };
    let pixels = read_pixels(data, offset, size).ok_or("frame memory is not readable")?;
    let rgb = to_rgb(
        info.format(),
        &pixels,
        width as usize,
        height as usize,
        stride,
    )?;
    Ok(CapturedFrame {
        width,
        height,
        stride,
        format: format!("{:?}", info.format()),
        buffer: buffer_name(kind),
        rgb,
    })
}

fn read_pixels(data: &mut spa::buffer::Data, offset: usize, size: usize) -> Option<Vec<u8>> {
    if size == 0 {
        return None;
    }
    if let Some(mapped) = data.data() {
        let end = offset.checked_add(size)?.min(mapped.len());
        if offset < end {
            return Some(mapped[offset..end].to_vec());
        }
    }
    // MemFd buffers through a portal fd can be advertised without a usable
    // mapped pointer; map the buffer for the duration of this copy only.
    let fd = data.fd();
    if fd < 0 {
        return None;
    }
    let length = offset.checked_add(size)?;
    // SAFETY: read-only shared mapping of the buffer fd for this copy.
    let pointer = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            length,
            libc::PROT_READ,
            libc::MAP_SHARED,
            fd,
            0,
        )
    };
    if pointer == libc::MAP_FAILED {
        return None;
    }
    let pixels =
        unsafe { std::slice::from_raw_parts((pointer as *const u8).add(offset), size) }.to_vec();
    unsafe {
        libc::munmap(pointer, length);
    }
    Some(pixels)
}

fn to_rgb(
    format: spa::param::video::VideoFormat,
    pixels: &[u8],
    width: usize,
    height: usize,
    stride: usize,
) -> Result<Vec<u8>> {
    let bgr = format == spa::param::video::VideoFormat::BGRx
        || format == spa::param::video::VideoFormat::BGRA;
    let rgb = format == spa::param::video::VideoFormat::RGBA
        || format == spa::param::video::VideoFormat::RGBx;
    if !bgr && !rgb {
        return Err(format!("unsupported video format: {format:?}").into());
    }
    let mut out = Vec::with_capacity(width * height * 3);
    for row in 0..height {
        let start = row * stride;
        let end = start + width * 4;
        let line = pixels
            .get(start..end)
            .ok_or("frame buffer is shorter than its stride")?;
        for pixel in line.as_chunks::<4>().0 {
            if bgr {
                out.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
            } else {
                out.extend_from_slice(&[pixel[0], pixel[1], pixel[2]]);
            }
        }
    }
    Ok(out)
}

fn buffer_name(kind: spa::buffer::DataType) -> String {
    if kind == spa::buffer::DataType::MemFd {
        "memfd".to_string()
    } else if kind == spa::buffer::DataType::DmaBuf {
        "dmabuf".to_string()
    } else if kind == spa::buffer::DataType::MemPtr {
        "memptr".to_string()
    } else {
        format!("{:?}", kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgrx_and_rgba_become_rgb() {
        let bgrx = [1u8, 2, 3, 0, 4, 5, 6, 0];
        assert_eq!(
            to_rgb(spa::param::video::VideoFormat::BGRx, &bgrx, 2, 1, 8).unwrap(),
            vec![3, 2, 1, 6, 5, 4]
        );
        let rgba = [1u8, 2, 3, 255, 4, 5, 6, 255];
        assert_eq!(
            to_rgb(spa::param::video::VideoFormat::RGBA, &rgba, 2, 1, 8).unwrap(),
            vec![1, 2, 3, 4, 5, 6]
        );
    }

    #[test]
    fn padding_is_skipped() {
        let rows = [
            1u8, 2, 3, 0, 4, 5, 6, 0, 9, 9, 9, 9, //
            7, 8, 9, 0, 10, 11, 12, 0, 9, 9, 9, 9,
        ];
        assert_eq!(
            to_rgb(spa::param::video::VideoFormat::BGRx, &rows, 2, 2, 12).unwrap(),
            vec![3, 2, 1, 6, 5, 4, 9, 8, 7, 12, 11, 10]
        );
    }

    #[test]
    fn a_short_row_is_an_error() {
        assert!(to_rgb(spa::param::video::VideoFormat::BGRx, &[1, 2, 3, 4], 2, 1, 8).is_err());
    }
}
