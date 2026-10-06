"""Teach the presenter's dma-buf import a three-plane planar export.

The colour side already exists: `CscPass::new_planar` builds the three-binding
planar pass, `bind_planes_planar` binds Y, Cb, Cr, and the CPU I420 rung proves
both on this device. The missing piece is an importer for a decoder that exports
ONE dma-buf holding three planes - the Cedar rung on TSPS, whose pictures are
linear YV12 (offsets 0 / 921600 / 1152000 at 720p, verified byte-for-byte).

Two things constrain the shape of the change, and both come from the existing
code rather than from taste:

* `get_or_import` takes the frame by value and caches imported images per
  `pool_key`, because an import costs image creates, memory imports and mappings.
  A separate import beside it would re-create three images every frame - more
  expensive than the 0.9 ms copy it removes - so the three-plane case goes through
  the same cached `import`.
* `let p = &cache.planes[&frame.pool_key]` hands out a *reference* to the cache, so
  the plane arrays must stay `Copy`: fixed length (`MAX_PLANES`), not `Vec`.

Every anchor must match exactly once, and the replaced function body is delimited
by its own signature and the next item's doc comment; drift aborts before writing.
The replacement keeps the signature's parameter ORDER (`instance, pdev, device,
ext_mem_fd, cache, frame`) - the call site passes them in that order, and the
doc comments in this file list them in a different one.
"""
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]


def target_for(root: pathlib.Path) -> pathlib.Path:
    return root / 'crates/pf-presenter/src/dmabuf.rs'


def present_for(root: pathlib.Path) -> pathlib.Path:
    return root / 'crates/pf-presenter/src/vk/present.rs'


EDITS = [
    (
        'fourcc constants',
        'const DRM_FORMAT_NV24: u32 = 0x3432_564e;\n',
        'const DRM_FORMAT_NV24: u32 = 0x3432_564e;\n'
        "/// fourcc('Y','U','1','2'): three single-component planes, Cb before Cr.\n"
        'const DRM_FORMAT_YUV420: u32 = 0x3231_5559;\n'
        "/// fourcc('Y','V','1','2'): three single-component planes, Cr before Cb.\n"
        'const DRM_FORMAT_YVU420: u32 = 0x3231_5659;\n'
        '\n'
        '/// Plane slots a frame or cache entry can hold: two for the interleaved\n'
        '/// formats, three for a planar export. Fixed so both stay `Copy`.\n'
        'const MAX_PLANES: usize = 3;\n',
    ),
    (
        'HwFrame plane fields',
        '    pub luma_view: vk::ImageView,\n'
        '    pub chroma_view: vk::ImageView,\n',
        '    /// Plane views, `planes` of them valid: two for the interleaved formats,\n'
        '    /// three for a planar export (Y, Cb, Cr - the order the planar CSC binds).\n'
        '    pub views: [vk::ImageView; MAX_PLANES],\n'
        '    pub planes: u8,\n'
        '    /// Planar export: the CSC pass binds three planes and the direct pass\n'
        '    /// draws the planar pipeline.\n'
        '    pub planar: bool,\n',
    ),
    (
        'HwFrame image storage',
        # `images: [vk::Image; 2],` alone appears in both HwFrame and Planes, so the
        # anchor carries the following field to stay unique.
        '    images: [vk::Image; 2],\n'
        "    /// Pool generation (high half of the frame's `pool_key`).\n",
        '    images: [vk::Image; MAX_PLANES],\n'
        "    /// Pool generation (high half of the frame's `pool_key`).\n",
    ),
    (
        'HwFrame accessors',
        "    /// Plane images for the presenter's foreign-acquire barriers.\n"
        '    pub fn luma_image(&self) -> vk::Image {\n'
        '        self.images[0]\n'
        '    }\n'
        '\n'
        '    pub fn chroma_image(&self) -> vk::Image {\n'
        '        self.images[1]\n'
        '    }\n',
        "    /// Plane images for the presenter's foreign-acquire barriers.\n"
        '    pub fn plane_images(&self) -> &[vk::Image] {\n'
        '        &self.images[..usize::from(self.planes)]\n'
        '    }\n',
    ),
    (
        'Planes storage',
        '    layout: [(u32, u32); 2],\n'
        '    images: [vk::Image; 2],\n'
        '    memories: [vk::DeviceMemory; 2],\n'
        '    views: [vk::ImageView; 2],\n',
        '    layout: [(u32, u32); MAX_PLANES],\n'
        '    images: [vk::Image; MAX_PLANES],\n'
        '    memories: [vk::DeviceMemory; MAX_PLANES],\n'
        '    views: [vk::ImageView; MAX_PLANES],\n'
        '    planes: u8,\n'
        '    planar: bool,\n',
    ),
    (
        'Planes::destroy',
        '            for v in self.views {\n'
        '                device.destroy_image_view(v, None);\n'
        '            }\n'
        '            for i in self.images {\n'
        '                device.destroy_image(i, None);\n'
        '            }\n'
        '            for m in self.memories {\n'
        '                device.free_memory(m, None);\n'
        '            }\n',
        '            let planes = usize::from(self.planes);\n'
        '            for v in self.views.iter().take(planes) {\n'
        '                device.destroy_image_view(*v, None);\n'
        '            }\n'
        '            for i in self.images.iter().take(planes) {\n'
        '                device.destroy_image(*i, None);\n'
        '            }\n'
        '            for m in self.memories.iter().take(planes) {\n'
        '                device.free_memory(*m, None);\n'
        '            }\n',
    ),
    (
        'cache hit layout',
        '    let layout = [\n'
        '        frame\n'
        '            .planes\n'
        '            .first()\n'
        '            .map_or((0, 0), |p| (p.offset, p.stride)),\n'
        '        frame.planes.get(1).map_or((0, 0), |p| (p.offset, p.stride)),\n'
        '    ];\n',
        "    // Every plane's offset and stride, so a cached import is only reused for\n"
        '    // the same memory layout.\n'
        '    let mut layout = [(0u32, 0u32); MAX_PLANES];\n'
        '    for (index, p) in frame.planes.iter().take(MAX_PLANES).enumerate() {\n'
        '        layout[index] = (p.offset, p.stride);\n'
        '    }\n',
    ),
    (
        'HwFrame rebuild from cache',
        '        luma_view: p.views[0],\n'
        '        chroma_view: p.views[1],\n',
        '        views: p.views,\n'
        '        planes: p.planes,\n'
        '        planar: p.planar,\n',
    ),
    (
        'import failure diagnostic',
        '    }\n'
        '    .with_context(|| {\n'
        '        format!("create {width}x{height} {format:?} image (modifier {modifier:#018x})")\n'
        '    })?;\n',
        '    };\n'
        '    // This driver refuses the plane LAYOUT, and the same tuple is accepted in\n'
        '    // isolation, so on failure retry the create with a plain LINEAR tiling and\n'
        '    // no modifier chain: that names which half this process is refused.\n'
        '    let image = match image {\n'
        '        Ok(image) => image,\n'
        '        Err(e) => {\n'
        '            let plain = unsafe {\n'
        '                device.create_image(\n'
        '                    &vk::ImageCreateInfo::default()\n'
        '                        .image_type(vk::ImageType::TYPE_2D)\n'
        '                        .format(format)\n'
        '                        .extent(vk::Extent3D {\n'
        '                            width,\n'
        '                            height,\n'
        '                            depth: 1,\n'
        '                        })\n'
        '                        .mip_levels(1)\n'
        '                        .array_layers(1)\n'
        '                        .samples(vk::SampleCountFlags::TYPE_1)\n'
        '                        .tiling(vk::ImageTiling::LINEAR)\n'
        '                        .usage(vk::ImageUsageFlags::SAMPLED)\n'
        '                        .initial_layout(vk::ImageLayout::UNDEFINED),\n'
        '                    None,\n'
        '                )\n'
        '            };\n'
        '            let plain_rc = match &plain {\n'
        '                Ok(img) => {\n'
        '                    // SAFETY: created just above, never submitted.\n'
        '                    unsafe { device.destroy_image(*img, None) };\n'
        '                    0\n'
        '                }\n'
        '                Err(pe) => pe.as_raw(),\n'
        '            };\n'
        '            // Second retry: the same modifier chain, but with the plane byte length\n'
        '            // filled in. A zero size leaves the driver to infer a padded pitch,\n'
        '            // and it can only validate a non-zero offset once the buffer is known.\n'
        '            let sized = [vk::SubresourceLayout {\n'
        '                offset: u64::from(offset),\n'
        '                size: u64::from(width) * u64::from(height),\n'
        '                row_pitch: u64::from(stride),\n'
        '                array_pitch: 0,\n'
        '                depth_pitch: 0,\n'
        '            }];\n'
        '            let mut mi2 = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default()\n'
        '                .drm_format_modifier(modifier)\n'
        '                .plane_layouts(&sized);\n'
        '            let mut ei2 = vk::ExternalMemoryImageCreateInfo::default()\n'
        '                .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);\n'
        '            let sized_rc = match unsafe {\n'
        '                device.create_image(\n'
        '                    &vk::ImageCreateInfo::default()\n'
        '                        .push_next(&mut mi2)\n'
        '                        .push_next(&mut ei2)\n'
        '                        .image_type(vk::ImageType::TYPE_2D)\n'
        '                        .format(format)\n'
        '                        .extent(vk::Extent3D { width, height, depth: 1 })\n'
        '                        .mip_levels(1)\n'
        '                        .array_layers(1)\n'
        '                        .samples(vk::SampleCountFlags::TYPE_1)\n'
        '                        .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)\n'
        '                        .usage(vk::ImageUsageFlags::SAMPLED)\n'
        '                        .initial_layout(vk::ImageLayout::UNDEFINED),\n'
        '                    None,\n'
        '                )\n'
        '            } {\n'
        '                Ok(img) => {\n'
        '                    // SAFETY: created just above, never submitted.\n'
        '                    unsafe { device.destroy_image(img, None) };\n'
        '                    0\n'
        '                }\n'
        '                Err(pe) => pe.as_raw(),\n'
        '            };\n'
        '            tracing::warn!(target: "presenter-import-diag",\n'
        '                rc = e.as_raw(), plain_linear_rc = plain_rc, sized_layout_rc = sized_rc,\n'
        '                width, height, format = ?format, modifier, offset, stride,\n'
        '                "modifier-chain create refused; retried plain LINEAR and a sized layout");\n'
        '            tracing::warn!(target: "presenter-import-diag",\n'
        '                rc = e.as_raw(), plain_linear_rc = plain_rc, width, height,\n'
        '                format = ?format, modifier, offset, stride,\n'
        '                "modifier-chain create refused; plain LINEAR retried");\n'
        '            return Err(e).with_context(|| {\n'
        '                format!(\n'
        '                    "create {width}x{height} {format:?} image (modifier {modifier:#018x}, \\\n'
        '                     offset {offset}, stride {stride}; plain LINEAR rc {plain_rc})"\n'
        '                )\n'
        '            });\n'
        '        }\n'
        '    };\n',
    ),
]

PRESENT_EDITS = [
    (
        'foreign-acquire barriers',
        '                        for view_image in [f.luma_image(), f.chroma_image()] {\n',
        '                        for &view_image in f.plane_images() {\n',
    ),
    (
        'plane binding',
        '            Lane::Dmabuf(f) => self\n'
        '                .csc\n'
        '                .bind_planes(&self.device, f.luma_view, f.chroma_view),\n',
        '            Lane::Dmabuf(f) => {\n'
        '                if f.planar {\n'
        '                    // Three single-component planes, in the order the planar\n'
        '                    // CSC shader binds them.\n'
        '                    self.csc_planar.bind_planes_planar(\n'
        '                        &self.device,\n'
        '                        [f.views[0], f.views[1], f.views[2]],\n'
        '                        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,\n'
        '                    );\n'
        '                } else {\n'
        '                    self.csc.bind_planes(&self.device, f.views[0], f.views[1]);\n'
        '                }\n'
        '            }\n',
    ),
    (
        'CSC pass choice',
        '                        self.record_csc(\n'
        '                            false,\n'
        '                            target,\n'
        '                            f.uv_scale(),\n'
        '                            f.color,\n'
        '                            if ten_bit { 10 } else { 8 },\n'
        '                            ten_bit,\n'
        '                        );\n',
        '                        self.record_csc(\n'
        '                            f.planar,\n'
        '                            target,\n'
        '                            f.uv_scale(),\n'
        '                            f.color,\n'
        '                            if ten_bit { 10 } else { 8 },\n'
        '                            ten_bit,\n'
        '                        );\n',
    ),
]

IMPORT_START = 'fn import(\n'
IMPORT_END = '/// One plane as an explicit-modifier image.'

NEW_IMPORT = '''fn import(
    instance: &ash::Instance,
    pdev: vk::PhysicalDevice,
    device: &ash::Device,
    ext_mem_fd: &ash::khr::external_memory_fd::Device,
    cache: &mut ModifierCache,
    frame: &DmabufFrame,
) -> Result<Planes> {
    // Test hook: fault every import so demotion is exercisable without a broken
    // driver. Per-frame lookup is fine — demotion silences it within three frames.
    if std::env::var_os("PUNKTFUNK_HW_FAULT").is_some_and(|v| v == "import") {
        bail!("injected import failure (PUNKTFUNK_HW_FAULT=import)");
    }
    // A planar export is three single-component planes of one dma-buf, in the order
    // the planar CSC pass binds: Y, Cb, Cr. The per-plane offsets carry the memory
    // order, so `YV12` and `I420` differ only in which offset they point at.
    let planar = matches!(frame.fourcc, DRM_FORMAT_YVU420 | DRM_FORMAT_YUV420);
    let (luma_fmt, chroma_fmt, chroma_full_res) = match frame.fourcc {
        DRM_FORMAT_NV12 => (vk::Format::R8_UNORM, vk::Format::R8G8_UNORM, false),
        DRM_FORMAT_P010 => (vk::Format::R16_UNORM, vk::Format::R16G16_UNORM, false),
        DRM_FORMAT_NV24 => (vk::Format::R8_UNORM, vk::Format::R8G8_UNORM, true),
        DRM_FORMAT_YVU420 | DRM_FORMAT_YUV420 => {
            (vk::Format::R8_UNORM, vk::Format::R8_UNORM, false)
        }
        other => bail!("hw presenter handles NV12/P010/NV24/YUV420 only (got {other:#x})"),
    };
    let wanted = if planar { 3 } else { 2 };
    if frame.planes.len() != wanted {
        bail!(
            "a {} export needs exactly {wanted} planes (got {})",
            if planar { "planar" } else { "2-plane YCbCr" },
            frame.planes.len()
        );
    }
    if frame.width == 0
        || frame.height == 0
        || frame.coded_width < frame.width
        || frame.coded_height < frame.height
    {
        bail!(
            "dmabuf extent must be nonzero with visible <= coded (got {}x{} in {}x{})",
            frame.width,
            frame.height,
            frame.coded_width,
            frame.coded_height
        );
    }
    let modifier = explicit_modifier(frame.modifier)?;
    // The export's modifier is only legal if this device can import it; a
    // foreign GPU's tiling must refuse here, not at image create.
    for fmt in [luma_fmt, chroma_fmt] {
        if !cache.importable(instance, pdev, fmt, modifier) {
            bail!("dmabuf modifier {modifier:#x} not importable as sampled {fmt:?} on this device");
        }
    }

    // Plane images take the exported extent: tiled addressing is defined over the
    // coded size, so a visible-only image would misaddress the tail rows. Chroma is
    // half the coded extent unless the format packs it full-res.
    let (cw, ch) = if chroma_full_res {
        (frame.coded_width, frame.coded_height)
    } else {
        (
            frame.coded_width.div_ceil(2),
            frame.coded_height.div_ceil(2),
        )
    };
    let mut layout = [(0u32, 0u32); MAX_PLANES];
    let mut images = [vk::Image::null(); MAX_PLANES];
    let mut memories = [vk::DeviceMemory::null(); MAX_PLANES];
    let mut created = 0usize;
    for index in 0..wanted {
        let p = &frame.planes[index];
        layout[index] = (p.offset, p.stride);
        let (w, h, fmt) = if index == 0 {
            (frame.coded_width, frame.coded_height, luma_fmt)
        } else {
            (cw, ch, chroma_fmt)
        };
        match plane_image(
            device, ext_mem_fd, w, h, fmt, p.fd, p.offset, p.stride, modifier,
        ) {
            Ok((image, memory)) => {
                images[index] = image;
                memories[index] = memory;
                created = index + 1;
            }
            Err(e) => {
                // SAFETY: every image and memory created so far is unreferenced by
                // any submission, so the GPU is idle on them.
                unsafe {
                    for i in 0..created {
                        device.destroy_image(images[i], None);
                        device.free_memory(memories[i], None);
                    }
                }
                return Err(e).with_context(|| format!("plane {index}"));
            }
        }
    }

    let view = |image, format| {
        // SAFETY: `image` is owned by this function; the create-info locals
        // outlive the call.
        unsafe {
            device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .subresource_range(
                        vk::ImageSubresourceRange::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .level_count(1)
                            .layer_count(1),
                    ),
                None,
            )
        }
        .context("plane image view")
    };
    // SAFETY: the images and memories were created in this call and never
    // submitted, so the GPU is idle on them.
    let destroy_images = |views: &[vk::ImageView]| unsafe {
        for v in views {
            device.destroy_image_view(*v, None);
        }
        for i in 0..created {
            device.destroy_image(images[i], None);
            device.free_memory(memories[i], None);
        }
    };
    let mut views = [vk::ImageView::null(); MAX_PLANES];
    for index in 0..wanted {
        let fmt = if index == 0 { luma_fmt } else { chroma_fmt };
        match view(images[index], fmt) {
            Ok(v) => views[index] = v,
            Err(e) => {
                destroy_images(&views[..index]);
                return Err(e);
            }
        }
    }

    Ok(Planes {
        generation: frame.pool_key >> 32,
        coded_width: frame.coded_width,
        coded_height: frame.coded_height,
        fourcc: frame.fourcc,
        modifier: frame.modifier,
        layout,
        images,
        memories,
        views,
        planes: wanted as u8,
        planar,
    })
}

'''


def replace_once(text: str, old: str, new: str, what: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(
            f'presenter planar: anchor for {what} matched {count} times, expected 1 '
            '- the pinned revision drifted; regenerate the anchors'
        )
    return text.replace(old, new)


def replace_region(text: str, start: str, end: str, new: str, what: str) -> str:
    first = text.find(start)
    if first < 0 or text.count(start) != 1:
        raise SystemExit(f'presenter planar: region start for {what} not unique; no writes')
    last = text.find(end, first)
    if last < 0:
        raise SystemExit(f'presenter planar: region end for {what} missing; no writes')
    return text[:first] + new + text[last:]


def main() -> int:
    if len(sys.argv) != 2:
        print('usage: patch-presenter-planar.py <punktfunk-checkout>')
        return 2
    root = pathlib.Path(sys.argv[1])
    dmabuf = target_for(root)
    present = present_for(root)
    if not dmabuf.is_file() or not present.is_file():
        print(f'presenter planar: sources not found under {root}; run after they are pinned')
        return 1
    text = dmabuf.read_text()
    if 'MAX_PLANES' in text:
        print('presenter planar: already applied')
        return 0
    for what, old, new in EDITS:
        text = replace_once(text, old, new, what)
    text = replace_region(text, IMPORT_START, IMPORT_END, NEW_IMPORT, 'import body')
    dmabuf.write_text(text)

    view = present.read_text()
    for what, old, new in PRESENT_EDITS:
        view = replace_once(view, old, new, what)
    present.write_text(view)
    print('presenter planar: HwFrame/Planes generalised, planar import wired')
    return 0


if __name__ == '__main__':
    sys.exit(main())
