//! Рендер: всё, что связано с Vulkan.
//!
//! Сейчас он умеет одно — залить окно цветом и показать кадр. Но вокруг
//! этого уже стоит вся обязательная «обвязка»: цепочка показа, проход
//! рендера, синхронизация CPU и GPU, пересоздание при изменении размера.
//! Объяснение: `docs/app/vulkan-init.md` и `docs/app/frame.md`.

mod context;
mod swapchain;

use anyhow::Result;
use ash::vk;
use winit::window::Window;

use context::VulkanContext;
use swapchain::Swapchain;

/// Сколько кадров CPU может готовить, пока GPU рисует предыдущие.
/// 2 — стандартный баланс между задержкой ввода и загрузкой GPU.
const FRAMES_IN_FLIGHT: usize = 2;

/// Что получилось при попытке нарисовать кадр.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameOutcome {
    /// Кадр нарисован и отправлен на экран.
    Presented,
    /// Окно свёрнуто или цепочка устарела — кадр пропущен.
    Skipped,
}

/// Ресурсы одного «кадра в полёте».
struct FrameSync {
    command_buffer: vk::CommandBuffer,
    /// GPU сигналит, когда картинка из цепочки свободна для рисования.
    image_available: vk::Semaphore,
    /// CPU ждёт этот забор, прежде чем снова использовать буфер команд кадра.
    in_flight: vk::Fence,
}

pub struct Renderer {
    // Порядок важен только для чтения: уничтожение делает Drop вручную.
    ctx: VulkanContext,
    swapchain: Swapchain,
    render_pass: vk::RenderPass,
    framebuffers: Vec<vk::Framebuffer>,
    /// По одному семафору «рисование закончено» на каждую картинку цепочки:
    /// так показ картинки не путается с соседними кадрами.
    render_finished: Vec<vk::Semaphore>,
    command_pool: vk::CommandPool,
    frames: Vec<FrameSync>,
    frame_index: usize,
    vsync: bool,
    window_size: (u32, u32),
    needs_recreate: bool,
}

impl Renderer {
    pub fn new(window: &Window, vsync: bool) -> Result<Self> {
        let ctx = VulkanContext::new(window)?;
        let size = window.inner_size();
        let window_size = (size.width, size.height);
        let swapchain = Swapchain::new(&ctx, window_size, vsync, vk::SwapchainKHR::null())?;
        let render_pass = create_render_pass(&ctx.device, swapchain.format)?;
        let framebuffers = create_framebuffers(&ctx.device, render_pass, &swapchain)?;
        let render_finished = create_semaphores(&ctx.device, swapchain.images.len())?;

        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(ctx.queue_family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { ctx.device.create_command_pool(&pool_info, None) }?;
        let alloc = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(FRAMES_IN_FLIGHT as u32);
        let buffers = unsafe { ctx.device.allocate_command_buffers(&alloc) }?;
        let image_available = create_semaphores(&ctx.device, FRAMES_IN_FLIGHT)?;
        let frames = buffers
            .into_iter()
            .zip(image_available)
            .map(|(command_buffer, image_available)| {
                // Забор создаётся «поднятым», чтобы первый кадр не ждал вечно.
                let info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
                let in_flight = unsafe { ctx.device.create_fence(&info, None) }?;
                Ok(FrameSync {
                    command_buffer,
                    image_available,
                    in_flight,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            ctx,
            swapchain,
            render_pass,
            framebuffers,
            render_finished,
            command_pool,
            frames,
            frame_index: 0,
            vsync,
            window_size,
            needs_recreate: false,
        })
    }

    pub fn gpu_name(&self) -> &str {
        &self.ctx.gpu_name
    }

    /// Сообщает рендеру новый размер окна; цепочка пересоздастся перед следующим кадром.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.window_size = (width, height);
        self.needs_recreate = true;
    }

    /// Рисует один кадр, заливая окно цветом `color` (RGBA, 0..1).
    pub fn draw_frame(&mut self, color: [f32; 4]) -> Result<FrameOutcome> {
        // Свёрнутое окно имеет размер 0×0: рисовать некуда.
        if self.window_size.0 == 0 || self.window_size.1 == 0 {
            return Ok(FrameOutcome::Skipped);
        }
        if self.needs_recreate {
            self.recreate_swapchain()?;
        }

        let device = &self.ctx.device;
        let frame = &self.frames[self.frame_index];

        // 1. Ждём, пока GPU закончит кадр, который рисовался в этом слоте раньше.
        unsafe { device.wait_for_fences(&[frame.in_flight], true, u64::MAX) }?;

        // 2. Берём свободную картинку из цепочки.
        let acquired = unsafe {
            self.ctx.swapchain_fn.acquire_next_image(
                self.swapchain.handle,
                u64::MAX,
                frame.image_available,
                vk::Fence::null(),
            )
        };
        let image_index = match acquired {
            Ok((index, suboptimal)) => {
                self.needs_recreate |= suboptimal;
                index
            }
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                self.needs_recreate = true;
                return Ok(FrameOutcome::Skipped);
            }
            Err(e) => return Err(e.into()),
        };
        // Забор опускаем только когда точно отправим работу, иначе следующий кадр зависнет.
        unsafe { device.reset_fences(&[frame.in_flight]) }?;

        // 3. Записываем команды: начать проход рендера (он и зальёт цветом), закончить.
        let cmd = frame.command_buffer;
        unsafe {
            device.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())?;
            let begin = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            device.begin_command_buffer(cmd, &begin)?;
            let clear = [vk::ClearValue {
                color: vk::ClearColorValue { float32: color },
            }];
            let pass = vk::RenderPassBeginInfo::default()
                .render_pass(self.render_pass)
                .framebuffer(self.framebuffers[image_index as usize])
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D::default(),
                    extent: self.swapchain.extent,
                })
                .clear_values(&clear);
            device.cmd_begin_render_pass(cmd, &pass, vk::SubpassContents::INLINE);
            // Здесь в будущем будут команды рисования карты, юнитов и интерфейса.
            device.cmd_end_render_pass(cmd);
            device.end_command_buffer(cmd)?;
        }

        // 4. Отправляем команды в очередь GPU.
        let render_finished = self.render_finished[image_index as usize];
        let wait = [frame.image_available];
        let stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
        let signal = [render_finished];
        let cmds = [cmd];
        let submit = vk::SubmitInfo::default()
            .wait_semaphores(&wait)
            .wait_dst_stage_mask(&stages)
            .command_buffers(&cmds)
            .signal_semaphores(&signal);
        unsafe { device.queue_submit(self.ctx.queue, &[submit], frame.in_flight) }?;

        // 5. Просим систему показать картинку, когда рисование закончится.
        let swapchains = [self.swapchain.handle];
        let indices = [image_index];
        let present = vk::PresentInfoKHR::default()
            .wait_semaphores(&signal)
            .swapchains(&swapchains)
            .image_indices(&indices);
        match unsafe {
            self.ctx
                .swapchain_fn
                .queue_present(self.ctx.queue, &present)
        } {
            Ok(suboptimal) => self.needs_recreate |= suboptimal,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => self.needs_recreate = true,
            Err(e) => return Err(e.into()),
        }

        self.frame_index = (self.frame_index + 1) % FRAMES_IN_FLIGHT;
        Ok(FrameOutcome::Presented)
    }

    fn recreate_swapchain(&mut self) -> Result<()> {
        let device = &self.ctx.device;
        // Простой и надёжный путь: дождаться, пока GPU всё доделает.
        unsafe { device.device_wait_idle() }?;
        destroy_framebuffers(device, &mut self.framebuffers);
        destroy_semaphores(device, &mut self.render_finished);

        let old = self.swapchain.handle;
        let new = Swapchain::new(&self.ctx, self.window_size, self.vsync, old)?;
        let mut old_swapchain = std::mem::replace(&mut self.swapchain, new);
        old_swapchain.destroy(&self.ctx);

        // Формат обычно тот же, но если сменился — нужен новый проход рендера.
        if self.swapchain.format != old_swapchain.format {
            unsafe { device.destroy_render_pass(self.render_pass, None) };
            self.render_pass = create_render_pass(device, self.swapchain.format)?;
        }
        self.framebuffers = create_framebuffers(device, self.render_pass, &self.swapchain)?;
        self.render_finished = create_semaphores(device, self.swapchain.images.len())?;
        self.needs_recreate = false;
        log::debug!("Swapchain пересоздан: {:?}", self.swapchain.extent);
        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let device = &self.ctx.device;
        unsafe {
            let _ = device.device_wait_idle();
            for f in &self.frames {
                device.destroy_fence(f.in_flight, None);
                device.destroy_semaphore(f.image_available, None);
            }
            device.destroy_command_pool(self.command_pool, None);
            destroy_semaphores(device, &mut self.render_finished);
            destroy_framebuffers(device, &mut self.framebuffers);
            device.destroy_render_pass(self.render_pass, None);
        }
        self.swapchain.destroy(&self.ctx);
        // VulkanContext уничтожит устройство и экземпляр в своём Drop.
    }
}

/// Проход рендера с одним цветовым вложением: в начале очистить цветом,
/// в конце перевести картинку в состояние «готова к показу».
fn create_render_pass(device: &ash::Device, format: vk::Format) -> Result<vk::RenderPass> {
    let attachments = [vk::AttachmentDescription::default()
        .format(format)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)];
    let color_ref = [vk::AttachmentReference {
        attachment: 0,
        layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
    }];
    let subpasses = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color_ref)];
    // Не начинать запись в картинку, пока система не отдала её (семафор image_available).
    let dependencies = [vk::SubpassDependency {
        src_subpass: vk::SUBPASS_EXTERNAL,
        dst_subpass: 0,
        src_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
        dst_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
        src_access_mask: vk::AccessFlags::empty(),
        dst_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        dependency_flags: vk::DependencyFlags::empty(),
    }];
    let info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(&subpasses)
        .dependencies(&dependencies);
    Ok(unsafe { device.create_render_pass(&info, None) }?)
}

fn create_framebuffers(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    swapchain: &Swapchain,
) -> Result<Vec<vk::Framebuffer>> {
    swapchain
        .views
        .iter()
        .map(|&view| {
            let attachments = [view];
            let info = vk::FramebufferCreateInfo::default()
                .render_pass(render_pass)
                .attachments(&attachments)
                .width(swapchain.extent.width)
                .height(swapchain.extent.height)
                .layers(1);
            Ok(unsafe { device.create_framebuffer(&info, None) }?)
        })
        .collect()
}

fn create_semaphores(device: &ash::Device, count: usize) -> Result<Vec<vk::Semaphore>> {
    (0..count)
        .map(|_| Ok(unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) }?))
        .collect()
}

fn destroy_framebuffers(device: &ash::Device, list: &mut Vec<vk::Framebuffer>) {
    for fb in list.drain(..) {
        unsafe { device.destroy_framebuffer(fb, None) };
    }
}

fn destroy_semaphores(device: &ash::Device, list: &mut Vec<vk::Semaphore>) {
    for s in list.drain(..) {
        unsafe { device.destroy_semaphore(s, None) };
    }
}
