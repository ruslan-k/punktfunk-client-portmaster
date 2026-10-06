"""Add a three-plane planar dma-buf import to the presenter.

The colour-conversion side already exists: `CscPass::new_planar` builds the
three-binding planar pass, `bind_planes_planar` binds it, and the CPU I420 rung
proves both on this device. What is missing is an importer for a decoder that
exports ONE dma-buf holding three planes - the Cedar rung on TSPS, whose pictures
are linear YV12 (offsets 0 / 921600 / 1152000 at 720p, verified byte-for-byte).

This adds the frame type and the import only. The two-plane NV12/P010/NV24 path is
untouched and the lane wiring lands separately, so a build here changes no
behaviour.

Anchors are exact; drift aborts without writing anything.
"""
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]

# The pinned Punktfunk checkout is the first argument, as in the other patchers;
# its sources are not under the port root.
def target_for(root: pathlib.Path) -> pathlib.Path:
    return root / 'crates/pf-presenter/src/dmabuf.rs'

ANCHOR = '''#[cfg(test)]
mod tests {
    use super::*;
'''

NEW_CODE = '''/// A three-plane planar export: Y, Cb and Cr as separate single-component planes
/// of one dma-buf. Same lifetime contract as [`HwFrame`] - the guard keeps the
/// decoder surface alive until the sampling fence signals.
#[allow(dead_code)]
pub struct HwFramePlanar {
    /// Y, Cb, Cr - the order `planar_csc.frag` binds.
    pub views: [vk::ImageView; 3],
    pub color: pf_client_core::video::ColorDesc,
    pub width: u32,
    pub height: u32,
    /// Decode-complete semaphores the sampling submit must wait.
    pub sync_sems: Vec<vk::Semaphore>,
    coded_width: u32,
    coded_height: u32,
    images: [vk::Image; 3],
    generation: u64,
    _guard: DrmFrameGuard,
}

#[allow(dead_code)]
impl HwFramePlanar {
    /// UV scale cropping the coded-extent images to the visible picture.
    pub fn uv_scale(&self) -> [f32; 2] {
        crop_scale(self.width, self.height, self.coded_width, self.coded_height)
    }

    /// Plane images for the presenter's foreign-acquire barriers.
    pub fn plane_image(&self, index: usize) -> vk::Image {
        self.images[index]
    }

    /// The decoder pool this frame's surface belongs to.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Release the decoder surface. The plane images belong to the cache.
    pub fn destroy(self, _device: &ash::Device) {}
}

/// Import a three-plane planar export as three `R8` images.
///
/// `frame.planes` is `[Y, Cb, Cr]` in semantic order whatever the fourcc's memory
/// order says: the per-plane offsets carry the layout, and the CSC shader binds
/// Y, Cb, Cr in that order.
#[allow(dead_code)]
fn import_planar(
    device: &ash::Device,
    instance: &ash::Instance,
    pdev: vk::PhysicalDevice,
    ext_mem_fd: &ash::khr::external_memory_fd::Device,
    cache: &mut ModifierCache,
    frame: &DmabufFrame,
) -> Result<HwFramePlanar> {
    if frame.planes.len() != 3 {
        bail!(
            "3-plane planar needs exactly 3 planes (got {})",
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
    if !cache.importable(instance, pdev, vk::Format::R8_UNORM, modifier) {
        bail!("dmabuf modifier {modifier:#x} not importable as sampled R8_UNORM on this device");
    }

    let (cw, ch) = (
        frame.coded_width.div_ceil(2),
        frame.coded_height.div_ceil(2),
    );
    // Plane extents: luma at the coded size, both chroma planes at half.
    let extents = [
        (frame.coded_width, frame.coded_height),
        (cw, ch),
        (cw, ch),
    ];
    let mut images = [vk::Image::null(); 3];
    let mut memories = [vk::DeviceMemory::null(); 3];
    let mut created = 0usize;
    for (index, (w, h)) in extents.iter().enumerate() {
        let p = &frame.planes[index];
        match plane_image(
            device, ext_mem_fd, *w, *h, vk::Format::R8_UNORM, p.fd, p.offset, p.stride, modifier,
        ) {
            Ok((image, memory)) => {
                images[index] = image;
                memories[index] = memory;
                created = index + 1;
            }
            Err(e) => {
                // SAFETY: every image and memory this call created is unreferenced
                // by any submission yet, so the device is idle on them.
                unsafe {
                    for i in 0..created {
                        device.destroy_image(images[i], None);
                        device.free_memory(memories[i], None);
                    }
                }
                return Err(e).with_context(|| format!("planar plane {index}"));
            }
        }
    }

    let make_view = |image: vk::Image| {
        // SAFETY: `image` is owned by this call and never submitted before the view
        // exists; the create-info locals outlive the call.
        unsafe {
            device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(vk::Format::R8_UNORM)
                    .subresource_range(
                        vk::ImageSubresourceRange::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .level_count(1)
                            .layer_count(1),
                    ),
                None,
            )
        }
    };
    let mut views = [vk::ImageView::null(); 3];
    for index in 0..3 {
        match make_view(images[index]) {
            Ok(v) => views[index] = v,
            Err(e) => {
                // SAFETY: views created above and images from this call are
                // unreferenced by any submission.
                unsafe {
                    for v in views.iter().take(index) {
                        device.destroy_image_view(*v, None);
                    }
                    for i in 0..3 {
                        device.destroy_image(images[i], None);
                        device.free_memory(memories[i], None);
                    }
                }
                return Err(e).context("planar plane image view");
            }
        }
    }

    Ok(HwFramePlanar {
        views,
        color: frame.color,
        width: frame.width,
        height: frame.height,
        sync_sems: Vec::new(),
        coded_width: frame.coded_width,
        coded_height: frame.coded_height,
        images,
        generation: frame.pool_key >> 32,
        _guard: DrmFrameGuard(FrameGuard::Cedar),
    })
}

'''


def main() -> int:
    if len(sys.argv) != 2:
        print('usage: patch-presenter-planar.py <punktfunk-checkout>')
        return 2
    target = target_for(pathlib.Path(sys.argv[1]))
    if not target.is_file():
        print(f'presenter planar: {target} not found; run after the sources are pinned')
        return 1
    text = target.read_text()
    if 'fn import_planar(' in text:
        print('presenter planar: already applied')
        return 0
    if ANCHOR not in text:
        print('presenter planar: tests anchor drifted; no writes')
        return 1
    target.write_text(text.replace(ANCHOR, NEW_CODE + ANCHOR))
    print('presenter planar: added HwFramePlanar and import_planar')
    return 0


if __name__ == '__main__':
    sys.exit(main())
