//! Мелкие помощники для памяти видеокарты: буферы и буфер глубины.
//!
//! Объектов немного (несколько буферов и одна картинка глубины), поэтому
//! память выделяется напрямую, без отдельного аллокатора. Когда появится
//! `rm_render` с `gpu-allocator`, это уедет туда (docs/adr/crate-map.md).

use anyhow::{Result, anyhow};
use ash::vk;

use super::context::VulkanContext;

/// Буфер в памяти, видимой процессору. Отображён в адресное пространство
/// программы всё время жизни, поэтому запись — это просто копирование байтов.
pub struct HostBuffer {
    pub handle: vk::Buffer,
    memory: vk::DeviceMemory,
    ptr: *mut u8,
    pub size: u64,
}

impl HostBuffer {
    pub fn new(ctx: &VulkanContext, size: u64, usage: vk::BufferUsageFlags) -> Result<Self> {
        let device = &ctx.device;
        let info = vk::BufferCreateInfo::default()
            .size(size)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let handle = unsafe { device.create_buffer(&info, None) }?;
        let req = unsafe { device.get_buffer_memory_requirements(handle) };
        let memory = allocate(
            ctx,
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

    /// Копирует `data` в буфер со смещения `offset` (в байтах). Не влезающее отбрасывается.
    /// Возвращает, сколько элементов поместилось.
    pub fn write<T: Copy>(&mut self, offset: u64, data: &[T]) -> usize {
        let elem = std::mem::size_of::<T>() as u64;
        let room = self.size.saturating_sub(offset) / elem;
        let count = (data.len() as u64).min(room) as usize;
        // SAFETY: память отображена на весь размер буфера, диапазон проверен выше,
        // T — простые данные без указателей (Copy, repr(C)).
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr() as *const u8,
                self.ptr.add(offset as usize),
                count * elem as usize,
            );
        }
        count
    }

    pub fn destroy(&mut self, device: &ash::Device) {
        unsafe {
            device.unmap_memory(self.memory);
            device.destroy_buffer(self.handle, None);
            device.free_memory(self.memory, None);
        }
    }
}

/// Буфер глубины: какая поверхность ближе к камере в каждом пикселе.
pub struct DepthImage {
    image: vk::Image,
    memory: vk::DeviceMemory,
    pub view: vk::ImageView,
}

impl DepthImage {
    pub fn new(ctx: &VulkanContext, format: vk::Format, extent: vk::Extent2D) -> Result<Self> {
        let device = &ctx.device;
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D {
                width: extent.width,
                height: extent.height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let image = unsafe { device.create_image(&info, None) }?;
        let req = unsafe { device.get_image_memory_requirements(image) };
        let memory = allocate(ctx, req, vk::MemoryPropertyFlags::DEVICE_LOCAL)?;
        unsafe { device.bind_image_memory(image, memory, 0) }?;
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(format)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: depth_aspect(format),
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            });
        let view = unsafe { device.create_image_view(&view_info, None) }?;
        Ok(Self {
            image,
            memory,
            view,
        })
    }

    pub fn destroy(&mut self, device: &ash::Device) {
        unsafe {
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
    }
}

/// Первый формат глубины, который видеокарта умеет использовать как вложение.
/// Форматы со stencil идут первыми: через stencil карта кампании рисует
/// границы фракций (хак H-03, `docs/campaign-map/render.md`). Хотя бы один
/// из двух первых Vulkan обязан поддерживать.
pub fn pick_depth_format(ctx: &VulkanContext) -> Result<vk::Format> {
    [
        vk::Format::D24_UNORM_S8_UINT,
        vk::Format::D32_SFLOAT_S8_UINT,
        vk::Format::D32_SFLOAT,
        vk::Format::X8_D24_UNORM_PACK32,
        vk::Format::D16_UNORM,
    ]
    .into_iter()
    .find(|&f| {
        let props = unsafe {
            ctx.instance
                .get_physical_device_format_properties(ctx.physical_device, f)
        };
        props
            .optimal_tiling_features
            .contains(vk::FormatFeatureFlags::DEPTH_STENCIL_ATTACHMENT)
    })
    .ok_or_else(|| anyhow!("видеокарта не поддерживает буфер глубины"))
}

/// Есть ли в формате глубины stencil.
pub fn has_stencil(format: vk::Format) -> bool {
    matches!(
        format,
        vk::Format::D24_UNORM_S8_UINT
            | vk::Format::D32_SFLOAT_S8_UINT
            | vk::Format::D16_UNORM_S8_UINT
    )
}

fn depth_aspect(format: vk::Format) -> vk::ImageAspectFlags {
    if has_stencil(format) {
        vk::ImageAspectFlags::DEPTH | vk::ImageAspectFlags::STENCIL
    } else {
        vk::ImageAspectFlags::DEPTH
    }
}

fn allocate(
    ctx: &VulkanContext,
    req: vk::MemoryRequirements,
    flags: vk::MemoryPropertyFlags,
) -> Result<vk::DeviceMemory> {
    let props = unsafe {
        ctx.instance
            .get_physical_device_memory_properties(ctx.physical_device)
    };
    let type_index = (0..props.memory_type_count)
        .find(|&i| {
            req.memory_type_bits & (1 << i) != 0
                && props.memory_types[i as usize]
                    .property_flags
                    .contains(flags)
        })
        .ok_or_else(|| anyhow!("нет подходящей видеопамяти ({flags:?})"))?;
    let info = vk::MemoryAllocateInfo::default()
        .allocation_size(req.size)
        .memory_type_index(type_index);
    Ok(unsafe { ctx.device.allocate_memory(&info, None) }?)
}
