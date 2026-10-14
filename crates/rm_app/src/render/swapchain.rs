//! Цепочка показа (swapchain): набор картинок, в которые мы по очереди рисуем
//! и которые система по очереди показывает в окне. При изменении размера окна
//! цепочку пересоздают. Объяснение: `docs/app/vulkan-init.md`.

use anyhow::Result;
use ash::vk;

use super::context::VulkanContext;

pub struct Swapchain {
    pub handle: vk::SwapchainKHR,
    pub format: vk::Format,
    pub extent: vk::Extent2D,
    pub images: Vec<vk::Image>,
    pub views: Vec<vk::ImageView>,
}

impl Swapchain {
    /// Создаёт цепочку под текущий размер окна. `old` — предыдущая цепочка
    /// при пересоздании (драйвер может переиспользовать её ресурсы).
    pub fn new(
        ctx: &VulkanContext,
        window_size: (u32, u32),
        vsync: bool,
        old: vk::SwapchainKHR,
    ) -> Result<Self> {
        let pd = ctx.physical_device;
        let caps = unsafe {
            ctx.surface_fn
                .get_physical_device_surface_capabilities(pd, ctx.surface)
        }?;
        let formats = unsafe {
            ctx.surface_fn
                .get_physical_device_surface_formats(pd, ctx.surface)
        }?;
        let modes = unsafe {
            ctx.surface_fn
                .get_physical_device_surface_present_modes(pd, ctx.surface)
        }?;

        let format = choose_surface_format(&formats);
        let present_mode = choose_present_mode(&modes, vsync);
        let extent = choose_extent(&caps, window_size);
        let image_count = choose_image_count(&caps);
        log::debug!("Swapchain: {extent:?}, {format:?}, {present_mode:?}, картинок: {image_count}");

        let info = vk::SwapchainCreateInfoKHR::default()
            .surface(ctx.surface)
            .min_image_count(image_count)
            .image_format(format.format)
            .image_color_space(format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(caps.current_transform)
            .composite_alpha(choose_composite_alpha(caps.supported_composite_alpha))
            .present_mode(present_mode)
            .clipped(true)
            .old_swapchain(old);
        let handle = unsafe { ctx.swapchain_fn.create_swapchain(&info, None) }?;
        let images = unsafe { ctx.swapchain_fn.get_swapchain_images(handle) }?;

        let views = images
            .iter()
            .map(|&image| {
                let info = vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format.format)
                    .subresource_range(color_subresource_range());
                unsafe { ctx.device.create_image_view(&info, None) }
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            handle,
            format: format.format,
            extent,
            images,
            views,
        })
    }

    /// Уничтожает картинки и саму цепочку. Вызывать, когда GPU с ней закончил.
    pub fn destroy(&mut self, ctx: &VulkanContext) {
        unsafe {
            for &view in &self.views {
                ctx.device.destroy_image_view(view, None);
            }
            ctx.swapchain_fn.destroy_swapchain(self.handle, None);
        }
        self.views.clear();
        self.images.clear();
        self.handle = vk::SwapchainKHR::null();
    }
}

pub fn color_subresource_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}

/// Предпочитаем 8-битный sRGB формат: цвета выглядят правильно без ручной гаммы.
pub fn choose_surface_format(formats: &[vk::SurfaceFormatKHR]) -> vk::SurfaceFormatKHR {
    let preferred = [vk::Format::B8G8R8A8_SRGB, vk::Format::R8G8B8A8_SRGB];
    preferred
        .iter()
        .find_map(|&want| {
            formats
                .iter()
                .find(|f| f.format == want && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR)
        })
        .or_else(|| formats.first())
        .copied()
        .unwrap_or(vk::SurfaceFormatKHR {
            format: vk::Format::B8G8R8A8_UNORM,
            color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR,
        })
}

/// FIFO (вертикальная синхронизация) есть всегда. Без vsync берём MAILBOX
/// (без разрывов картинки), иначе IMMEDIATE.
pub fn choose_present_mode(modes: &[vk::PresentModeKHR], vsync: bool) -> vk::PresentModeKHR {
    if !vsync {
        for want in [vk::PresentModeKHR::MAILBOX, vk::PresentModeKHR::IMMEDIATE] {
            if modes.contains(&want) {
                return want;
            }
        }
    }
    vk::PresentModeKHR::FIFO
}

/// Размер картинок. Обычно его диктует окно (`current_extent`); значение
/// `u32::MAX` означает «выбирайте сами» — тогда берём размер окна в пределах.
pub fn choose_extent(caps: &vk::SurfaceCapabilitiesKHR, window_size: (u32, u32)) -> vk::Extent2D {
    if caps.current_extent.width != u32::MAX {
        return caps.current_extent;
    }
    vk::Extent2D {
        width: window_size
            .0
            .clamp(caps.min_image_extent.width, caps.max_image_extent.width),
        height: window_size
            .1
            .clamp(caps.min_image_extent.height, caps.max_image_extent.height),
    }
}

/// На одну картинку больше минимума, чтобы не ждать драйвер. 0 в максимуме = без ограничений.
pub fn choose_image_count(caps: &vk::SurfaceCapabilitiesKHR) -> u32 {
    let want = caps.min_image_count + 1;
    if caps.max_image_count > 0 {
        want.min(caps.max_image_count)
    } else {
        want
    }
}

fn choose_composite_alpha(supported: vk::CompositeAlphaFlagsKHR) -> vk::CompositeAlphaFlagsKHR {
    [
        vk::CompositeAlphaFlagsKHR::OPAQUE,
        vk::CompositeAlphaFlagsKHR::INHERIT,
        vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
        vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
    ]
    .into_iter()
    .find(|&m| supported.contains(m))
    .unwrap_or(vk::CompositeAlphaFlagsKHR::OPAQUE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(format: vk::Format) -> vk::SurfaceFormatKHR {
        vk::SurfaceFormatKHR {
            format,
            color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR,
        }
    }

    #[test]
    fn prefers_srgb_format() {
        let list = [
            fmt(vk::Format::B8G8R8A8_UNORM),
            fmt(vk::Format::B8G8R8A8_SRGB),
        ];
        assert_eq!(
            choose_surface_format(&list).format,
            vk::Format::B8G8R8A8_SRGB
        );
        let other = [fmt(vk::Format::A2B10G10R10_UNORM_PACK32)];
        assert_eq!(
            choose_surface_format(&other).format,
            vk::Format::A2B10G10R10_UNORM_PACK32
        );
    }

    #[test]
    fn present_mode_choice() {
        let all = [
            vk::PresentModeKHR::FIFO,
            vk::PresentModeKHR::IMMEDIATE,
            vk::PresentModeKHR::MAILBOX,
        ];
        assert_eq!(choose_present_mode(&all, true), vk::PresentModeKHR::FIFO);
        assert_eq!(
            choose_present_mode(&all, false),
            vk::PresentModeKHR::MAILBOX
        );
        assert_eq!(
            choose_present_mode(&[vk::PresentModeKHR::FIFO], false),
            vk::PresentModeKHR::FIFO
        );
    }

    #[test]
    fn extent_follows_surface_or_clamps_window() {
        let mut caps = vk::SurfaceCapabilitiesKHR {
            current_extent: vk::Extent2D {
                width: 800,
                height: 600,
            },
            min_image_extent: vk::Extent2D {
                width: 1,
                height: 1,
            },
            max_image_extent: vk::Extent2D {
                width: 4096,
                height: 4096,
            },
            ..Default::default()
        };
        assert_eq!(
            choose_extent(&caps, (1000, 1000)),
            vk::Extent2D {
                width: 800,
                height: 600
            }
        );
        caps.current_extent = vk::Extent2D {
            width: u32::MAX,
            height: u32::MAX,
        };
        assert_eq!(
            choose_extent(&caps, (9000, 300)),
            vk::Extent2D {
                width: 4096,
                height: 300
            }
        );
    }

    #[test]
    fn image_count_respects_limits() {
        let mut caps = vk::SurfaceCapabilitiesKHR {
            min_image_count: 2,
            max_image_count: 0,
            ..Default::default()
        };
        assert_eq!(choose_image_count(&caps), 3);
        caps.max_image_count = 2;
        assert_eq!(choose_image_count(&caps), 2);
    }
}
