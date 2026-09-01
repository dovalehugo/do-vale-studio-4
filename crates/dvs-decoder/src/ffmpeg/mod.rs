//! FFmpeg FFI helpers and RAII wrappers.

#![allow(unsafe_code)]

mod d3d11va;
mod ffi;
mod raii;

#[cfg(debug_assertions)]
pub(crate) use d3d11va::ffmpeg_d3d11_device_context_ptr;
pub(crate) use d3d11va::{
    adapter_luid_from_hw_device, borrow_d3d11_decoder_surface, borrow_ffmpeg_d3d11_device,
    build_external_context_lock, clone_ffmpeg_d3d11_device_context, create_d3d11va_device,
    open_d3d11_decoder, read_frame_fields,
};
pub(crate) use raii::{
    AvCodecContext, AvFormatContext, AvFrame, AvHwDeviceRef, AvPacket, ReadPacketResult,
    ReceiveResult, SendResult, init_ffmpeg_network,
};
