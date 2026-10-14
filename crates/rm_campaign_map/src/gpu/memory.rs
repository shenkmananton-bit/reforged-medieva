//! Буферы и картинки в видеопамяти, загрузка текстур карты.
//!
//! Объектов у карты немного (пять текстур и несколько буферов), поэтому
//! память выделяется напрямую, без отдельного аллокатора — так же, как в
//! `rm_app/src/render/gpu.rs`. Когда появится `rm_render`, это уедет туда.

use ash::vk;

use super::{GpuError, GpuRefs};
use crate::bake::Texture;

/// Буфер в памяти, видимой процессору: запись — простое копирование байтов.
pub struct HostBuffer {
    pub handle: vk::Buffer,
    memory: vk::DeviceMemory,
    ptr: *mut u8,
    pub size: u64,
}

impl HostBuffer {
    pub fn new(gpu: &GpuRefs, size: u64, usage: vk::BufferUsageFlags) -> Result<Self, GpuError> {
        let device = gpu.device;
        let info = vk::BufferCreateInfo::default()
            .size(size)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let handle = unsafe { device.create_buffer(&info, None) }?;
        let req = unsafe { device.get_buffer_memory_requirements(handle) };
        let memory = allocate(
            gpu,
            req,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        unsafe { device.bind_buffer_memory(handle, memory, 0) }?;
        let ptr =
            unsafe { device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty()) }? as *mut u8;
        Ok(Self {
            handle,
            memory,
            ptr,
            size,
        })
    }

    /// Копирует байты со смещения `offset`. Не влезающее отбрасывается.
    pub fn write_bytes(&mut self, offset: u64, data: &[u8]) {
        let room = self.size.saturating_sub(offset) as usize;
        let n = data.len().min(room);
        // SAFETY: память отображена на весь буфер, диапазон проверен выше.
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), self.ptr.add(offset as usize), n);
        }
    }

    pub fn destroy(&mut self, device: &ash::Device) {
        unsafe {
            device.unmap_memory(self.memory);
            device.destroy_buffer(self.handle, None);
            device.free_memory(self.memory, None);
        }
    }
}

/// Текстура в видеопамяти (только для чтения шейдерами).
pub struct GpuImage {
    image: vk::Image,
    memory: vk::DeviceMemory,
    pub view: vk::ImageView,
}

impl GpuImage {
    pub fn destroy(&mut self, device: &ash::Device) {
        unsafe {
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
    }
}

/// Загружает текстуры в видеопамять одной отправкой команд.
///
/// Картинки с процессора нельзя положить прямо в «быструю» память
/// видеокарты, поэтому байты сначала копируются в промежуточный буфер
/// (staging), а уже видеокарта переносит их в картинки и переводит
/// картинки в состояние «для чтения шейдером».
pub fn upload_textures(
    gpu: &GpuRefs,
    textures: &[(&Texture, vk::Format)],
) -> Result<Vec<GpuImage>, GpuError> {
    let device = gpu.device;
    // Раскладка всех уровней всех текстур в одном staging-буфере.
    // Смещения выровнены на 16 байт — с запасом для любых форматов.
    let mut offsets = Vec::new();
    let mut total = 0u64;
    for (tex, _) in textures {
        let mut levels = Vec::new();
        for level in &tex.levels {
            total = total.next_multiple_of(16);
            levels.push(total);
            total += level.len() as u64;
        }
        offsets.push(levels);
    }
    let mut staging = HostBuffer::new(gpu, total.max(16), vk::BufferUsageFlags::TRANSFER_SRC)?;
    for ((tex, _), levels) in textures.iter().zip(&offsets) {
        for (data, &offset) in tex.levels.iter().zip(levels) {
            staging.write_bytes(offset, data);
        }
    }

    let mut images = Vec::new();
    let result = (|| {
        for (tex, format) in textures {
            images.push(create_image(gpu, tex, *format)?);
        }
        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(gpu.queue_family)
            .flags(vk::CommandPoolCreateFlags::TRANSIENT);
        let pool = unsafe { device.create_command_pool(&pool_info, None) }?;
        let submit = (|| {
            let alloc = vk::CommandBufferAllocateInfo::default()
                .command_pool(pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(1);
            let cmd = unsafe { device.allocate_command_buffers(&alloc) }?[0];
            let begin = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            unsafe { device.begin_command_buffer(cmd, &begin) }?;
            for (((tex, _), levels), img) in textures.iter().zip(&offsets).zip(&images) {
                record_upload(device, cmd, staging.handle, tex, levels, img.image);
            }
            unsafe { device.end_command_buffer(cmd) }?;
            let cmds = [cmd];
            let info = vk::SubmitInfo::default().command_buffers(&cmds);
            unsafe {
                device.queue_submit(gpu.queue, &[info], vk::Fence::null())?;
                // Загрузка один раз при старте, поэтому просто ждём.
                device.queue_wait_idle(gpu.queue)?;
            }
            Ok::<(), GpuError>(())
        })();
        unsafe { device.destroy_command_pool(pool, None) };
        submit
    })();
    staging.destroy(device);
    match result {
        Ok(()) => Ok(images),
        Err(e) => {
            for mut img in images {
                img.destroy(device);
            }
            Err(e)
        }
    }
}

fn create_image(gpu: &GpuRefs, tex: &Texture, format: vk::Format) -> Result<GpuImage, GpuError> {
    let device = gpu.device;
    let levels = tex.levels.len() as u32;
    let info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(format)
        .extent(vk::Extent3D {
            width: tex.size,
            height: tex.size,
            depth: 1,
        })
        .mip_levels(levels)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED);
    let image = unsafe { device.create_image(&info, None) }?;
    let req = unsafe { device.get_image_memory_requirements(image) };
    let memory = match allocate(gpu, req, vk::MemoryPropertyFlags::DEVICE_LOCAL) {
        Ok(m) => m,
        Err(e) => {
            unsafe { device.destroy_image(image, None) };
            return Err(e);
        }
    };
    unsafe { device.bind_image_memory(image, memory, 0) }?;
    let view_info = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(format)
        .subresource_range(color_range(levels));
    let view = unsafe { device.create_image_view(&view_info, None) }?;
    Ok(GpuImage {
        image,
        memory,
        view,
    })
}

fn color_range(levels: u32) -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: levels,
        base_array_layer: 0,
        layer_count: 1,
    }
}

fn record_upload(
    device: &ash::Device,
    cmd: vk::CommandBuffer,
    staging: vk::Buffer,
    tex: &Texture,
    offsets: &[u64],
    image: vk::Image,
) {
    let range = color_range(tex.levels.len() as u32);
    let to_dst = vk::ImageMemoryBarrier::default()
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .src_access_mask(vk::AccessFlags::empty())
        .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(range);
    let regions: Vec<vk::BufferImageCopy> = offsets
        .iter()
        .enumerate()
        .map(|(level, &offset)| {
            let s = tex.level_size(level);
            vk::BufferImageCopy {
                buffer_offset: offset,
                buffer_row_length: 0,
                buffer_image_height: 0,
                image_subresource: vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: level as u32,
                    base_array_layer: 0,
                    layer_count: 1,
                },
                image_offset: vk::Offset3D::default(),
                image_extent: vk::Extent3D {
                    width: s,
                    height: s,
                    depth: 1,
                },
            }
        })
        .collect();
    let to_read = vk::ImageMemoryBarrier::default()
        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .dst_access_mask(vk::AccessFlags::SHADER_READ)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(range);
    unsafe {
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[to_dst],
        );
        device.cmd_copy_buffer_to_image(
            cmd,
            staging,
            image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &regions,
        );
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[to_read],
        );
    }
}

fn allocate(
    gpu: &GpuRefs,
    req: vk::MemoryRequirements,
    flags: vk::MemoryPropertyFlags,
) -> Result<vk::DeviceMemory, GpuError> {
    let props = unsafe {
        gpu.instance
            .get_physical_device_memory_properties(gpu.physical_device)
    };
    let type_index = (0..props.memory_type_count)
        .find(|&i| {
            req.memory_type_bits & (1 << i) != 0
                && props.memory_types[i as usize]
                    .property_flags
                    .contains(flags)
        })
        .ok_or(GpuError::NoMemoryType)?;
    let info = vk::MemoryAllocateInfo::default()
        .allocation_size(req.size)
        .memory_type_index(type_index);
    Ok(unsafe { gpu.device.allocate_memory(&info, None) }?)
}
