#![no_std]
#![no_main]
#![deny(warnings)]

extern crate alloc;
extern crate opensbi_rt;

use alloc::vec;
use device_tree::util::SliceRead;
use device_tree::{DeviceTree, Node};
use log::{info, warn, LevelFilter};
use virtio_drivers::*;
use virtio_impl::HalImpl;

mod virtio_impl;

#[no_mangle]
extern "C" fn main(_hartid: usize, device_tree_paddr: usize) {
    log::set_max_level(LevelFilter::Info);
    init_dt(device_tree_paddr);
    info!("test end");
}

fn init_dt(dtb: usize) {
    info!("device tree @ {:#x}", dtb);
    #[repr(C)]
    struct DtbHeader {
        be_magic: u32,
        be_size: u32,
    }
    let header = unsafe { &*(dtb as *const DtbHeader) };
    let magic = u32::from_be(header.be_magic);
    const DEVICE_TREE_MAGIC: u32 = 0xd00dfeed;
    assert_eq!(magic, DEVICE_TREE_MAGIC);
    let size = u32::from_be(header.be_size);
    let dtb_data = unsafe { core::slice::from_raw_parts(dtb as *const u8, size as usize) };
    let dt = DeviceTree::load(dtb_data).expect("failed to parse device tree");
    walk_dt_node(&dt.root);
}

fn walk_dt_node(dt: &Node) {
    if let Ok(compatible) = dt.prop_str("compatible") {
        if compatible == "virtio,mmio" {
            virtio_probe(dt);
        }
    }
    for child in dt.children.iter() {
        walk_dt_node(child);
    }
}

fn virtio_probe(node: &Node) {
    if let Some(reg) = node.prop_raw("reg") {
        let paddr = reg.as_slice().read_be_u64(0).unwrap();
        let size = reg.as_slice().read_be_u64(8).unwrap();
        let vaddr = paddr;
        info!("walk dt addr={:#x}, size={:#x}", paddr, size);
        let header = unsafe { &mut *(vaddr as *mut VirtIOHeader) };
        info!(
            "Detected virtio device with vendor id {:#X}, device type {:?}",
            header.vendor_id(),
            header.device_type(),
        );
        info!("Device tree node {:?}", node);
        match header.device_type() {
            DeviceType::Block => virtio_blk(header),
            DeviceType::GPU => virtio_gpu(header),
            DeviceType::Input => virtio_input(header),
            DeviceType::Network => virtio_net(header),
            DeviceType::Sound => virtio_sound(header),
            t => warn!("Unrecognized virtio device: {:?}", t),
        }
    }
}

fn virtio_blk(header: &'static mut VirtIOHeader) {
    let mut blk = VirtIOBlk::<HalImpl>::new(header).expect("failed to create blk driver");
    let mut input = vec![0xffu8; 512];
    let mut output = vec![0; 512];
    for i in 0..32 {
        for x in input.iter_mut() {
            *x = i as u8;
        }
        blk.write_block(i, &input).expect("failed to write");
        blk.read_block(i, &mut output).expect("failed to read");
        assert_eq!(input, output);
    }
    info!("virtio-blk test finished");
}

fn virtio_gpu(header: &'static mut VirtIOHeader) {
    let mut gpu = VirtIOGpu::<HalImpl>::new(header).expect("failed to create gpu driver");
    let fb = gpu.setup_framebuffer().expect("failed to get fb");
    for y in 0..768 {
        for x in 0..1024 {
            let idx = (y * 1024 + x) * 4;
            fb[idx] = x as u8;
            fb[idx + 1] = y as u8;
            fb[idx + 2] = (x + y) as u8;
        }
    }
    gpu.flush().expect("failed to flush");
    info!("virtio-gpu test finished");
}

fn virtio_input(header: &'static mut VirtIOHeader) {
    //let mut event_buf = [0u64; 32];
    let mut _input = VirtIOInput::<HalImpl>::new(header).expect("failed to create input driver");
    // loop {
    //     input.ack_interrupt().expect("failed to ack");
    //     info!("mouse: {:?}", input.mouse_xy());
    // }
    // TODO: handle external interrupt
}

fn virtio_net(header: &'static mut VirtIOHeader) {
    let mut net = VirtIONet::<HalImpl>::new(header).expect("failed to create net driver");
    let mut buf = [0u8; 0x100];
    let len = net.recv(&mut buf).expect("failed to recv");
    info!("recv: {:?}", &buf[..len]);
    net.send(&buf[..len]).expect("failed to send");
    info!("virtio-net test finished");
}

fn virtio_sound(header: &'static mut VirtIOHeader) {
    let mut sound =
        VirtIOSound::<HalImpl, T>::new(transport).expect("failed to create sound driver");
    let output_streams = sound.output_streams();
    if output_streams.len() > 0 {
        let output_stream_id = *output_streams.first().unwrap();
        let rates = sound.rates_supported(output_stream_id).unwrap();
        let formats = sound.formats_supported(output_stream_id).unwrap();
        let channel_range = sound.channel_range_supported(output_stream_id).unwrap();
        let features = sound.features_supported(output_stream_id).unwrap();

        let rate = if rates.contains(PcmRate::VIRTIO_SND_PCM_RATE_44100) {
            PcmRate::VIRTIO_SND_PCM_RATE_44100
        } else {
            PcmRate::VIRTIO_SND_PCM_RATE_32000
        };
        let format = if formats.contains(PcmFormats::VIRTIO_SND_PCM_FMT_U8) {
            PcmFormats::VIRTIO_SND_PCM_FMT_U8
        } else {
            PcmFormats::VIRTIO_SND_PCM_FMT_U32
        };
        let channel = if channel_range.contains(&2) {
            2 as u8
        } else {
            *channel_range.start()
        };
        sound
            .pcm_set_params(
                output_stream_id,
                4410 * 2,
                4410,
                features,
                channel,
                format,
                rate,
            )
            .expect("pcm_set_params error");
        sound
            .pcm_prepare(output_stream_id)
            .expect("pcm_prepare error");
        sound.pcm_start(output_stream_id).expect("pcm_start error");
        let music = include_bytes!("../Nocturne_44100Hz_u8_stereo.raw");
        info!("[sound device] music len is {} bytes.", music.len());
        // xfer buffer
        sound.pcm_xfer(output_stream_id, &music[..]).expect("pcm_xfer error");
        sound.pcm_stop(output_stream_id).expect("pcm_stop error");
        sound.pcm_release(output_stream_id).expect("pcm_release error");
    }
}
